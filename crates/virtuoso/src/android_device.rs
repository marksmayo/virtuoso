//! An Android device driven through the agent: the [`Device`] the runner uses.
//!
//! Device commands (app control, settings) run through the agent's `exec`, as the shell user,
//! which saves the ~30 ms an `adb shell` round trip costs.

use crate::android::{android_permissions, runtime_permissions, ungranted_runtime_permissions, Adb, AndroidAgent};
use crate::device::{Device, Key, LaunchOptions, Lift, Orientation, Settled};
use crate::screen::{ScreenReply, ScreenTree, WINDOW_TYPE_IME};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

static TRACE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("VIRTUOSO_DEBUG").is_some());

/// How long after `am start -W` returns to wait for a window of the app itself. An app's task
/// may show another package's activity on top (Settings' search), so this does not fail.
const APP_WINDOW_WAIT: Duration = Duration::from_secs(3);
/// The longest one request waits on the agent for a UI event before answering anyway.
const CHANGE_WAIT: Duration = Duration::from_millis(200);

fn has_keyboard(windows: &[Value]) -> bool {
    windows.iter().any(|w| w["type"].as_i64() == Some(WINDOW_TYPE_IME))
}

/// How long a keyboard may take to go after Back: a software-rendered emulator (Windows CI)
/// took more than 2 s.
const KEYBOARD_CLOSE_LIMIT: Duration = Duration::from_secs(5);

/// Closes the keyboard with Back, once: `keyboard(after, deadline)` says whether one is shown
/// (waiting, past the window change `after`, for another), and the keyboard gets `limit` from
/// the press to go. Back again would leave the screen if it had just gone.
pub fn close_keyboard(
    mut keyboard: impl FnMut(Option<u64>, Instant) -> Result<(bool, Option<u64>)>,
    mut back: impl FnMut() -> Result<()>,
    limit: Duration,
) -> Result<()> {
    let (shown, mut seq) = keyboard(None, Instant::now() + limit)?;
    if !shown {
        return Ok(());
    }
    back()?;
    let deadline = Instant::now() + limit;
    loop {
        let (shown, now_seq) = keyboard(seq, deadline)?;
        if !shown {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("the keyboard is still shown");
        }
        seq = now_seq;
    }
}

/// After the last UI event, how long the UI must stay quiet to count as settled.
const SETTLE_QUIET_MS: u64 = 80;

/// Where to tap to answer a system error dialog on screen ("X isn't responding": Wait;
/// "X keeps stopping" / "X has stopped": Close app), or None when there is none. A freshly
/// booted emulator often shows one over everything, and it would swallow the flow's taps.
pub fn error_dialog_button(s: &ScreenTree) -> Option<(i32, i32)> {
    let system = |i: usize| &*s.elements[i].package == "android";
    let is_error = (0..s.elements.len()).filter(|&i| system(i)).any(|i| {
        let t = s.elements[i].text.to_lowercase();
        t.contains("isn't responding") || t.contains("keeps stopping") || t.contains("has stopped") || t.contains("not responding")
    });
    if !is_error {
        return None;
    }
    let button = |label: &str| (0..s.elements.len()).find(|&i| system(i) && s.elements[i].text.eq_ignore_ascii_case(label));
    button("Wait").or_else(|| button("Close app")).or_else(|| button("OK")).map(|i| s.elements[i].bounds.center())
}

/// Whether a failed request means the agent is gone: the connection broke, as opposed to the
/// agent answering with an error, or a reply that is only slow.
fn agent_lost(e: &anyhow::Error) -> bool {
    use std::io::ErrorKind;
    e.chain().filter_map(|c| c.downcast_ref::<std::io::Error>()).any(|io| !matches!(io.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock))
}

/// The lines of `dumpsys package` that hold permission states, and the user headers that end
/// a section.
const PERMISSION_LINES: &str = "runtime permissions:|: granted=|^ *User [0-9]";

/// `s` as one shell word.
fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/=:,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

impl Drop for AndroidDevice {
    fn drop(&mut self) {
        // An action held back for a settle that never came still happens.
        let _ = self.flush();
    }
}

/// An agent request: one function builds them all, not a map literal at each call site.
/// The fields are moved in, not cloned (an inputText's whole text, a launch script).
fn req<const N: usize>(op: &str, fields: [(&str, Value); N]) -> Value {
    let mut m = serde_json::Map::with_capacity(N + 1);
    m.insert("op".into(), Value::from(op));
    for (k, v) in fields {
        m.insert(k.into(), v);
    }
    Value::Object(m)
}

/// A package or permission name: nothing a shell would read as more than a word.
fn plain_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// A shell command printing the app's runtime permissions (`ungranted`: those not granted
/// yet), one a line: one sed over `dumpsys package`, where a grep, a sed, a grep and a sed
/// were four processes narrowing the same text. Each runtime section runs from its header to
/// the next user's.
pub fn permission_names(app: &str, ungranted: bool) -> String {
    let state = if ungranted { "false" } else { "" };
    format!("dumpsys package {app} | sed -n '/runtime permissions:/,/^ *User [0-9]/s/^ *\\([^:]*\\): granted={state}.*/\\1/p'")
}

/// The one device shell script that stops (or clears), grants and starts `app` at `activity`:
/// each as a command of its own would cost a round trip and a process start. `app` is a
/// plain name (the caller checks). With no `activity` known yet, the script looks up the
/// app's launcher activity first, prints it on a line `launcher <activity>` for the caller to
/// keep, and exits 3 (saying so) when there is none.
pub fn launch_script(app: &str, activity: Option<&str>, o: &LaunchOptions) -> String {
    // `cmd` straight away: /system/bin/am and pm are shell scripts that only run `cmd activity`
    // and `cmd package` (API 24+; the agent needs 26), a shell started for each call.
    let mut script = String::new();
    if activity.is_none() {
        // resolve-activity --brief prints the component last; with none, its last word has no
        // '/'. The shell's own words, not a grep and a tail: two processes fewer.
        script.push_str(&format!(
            "set -- $(cmd package resolve-activity --brief -c android.intent.category.LAUNCHER {app}); eval a=\\${{$#}}; case $a in */*) ;; *) echo '{}'; exit 3;; esac; echo \"launcher $a\"; ",
            not_launchable(app)
        ));
    }
    let clear_failed = format!("{{ echo 'clearing {app} failed'; exit 1; }}");
    // As in Maestro, an app is launched with every permission unless the flow says otherwise.
    let grant_all = o.permissions.is_empty();
    if o.clear_state && grant_all {
        // The app's runtime permissions are listed while its data is cleared (the list is the
        // manifest's, which a clear leaves alone): one after the other they took ~440 ms on an
        // emulator, side by side ~290 ms. The clear revokes them all, so all are granted.
        script.push_str(&format!(
            "cmd package clear {app} >/dev/null & c=$!; ps=$({}); wait $c || {clear_failed}; ",
            permission_names(app, false)
        ));
    } else if o.clear_state {
        script.push_str(&format!("cmd package clear {app} >/dev/null || {clear_failed}; "));
    } else if o.stop_app {
        script.push_str(&format!("pidof {app} >/dev/null && cmd activity force-stop {app}; "));
    }
    if grant_all {
        let ungranted = format!("$({})", permission_names(app, true));
        let list = if o.clear_state { "$ps" } else { &ungranted };
        // The grants are independent: side by side, each one's process start overlaps the
        // others' (nine took ~270 ms in a row on an emulator, ~110 ms together).
        script.push_str(&format!("for p in {list}; do cmd package grant {app} $p 2>/dev/null & done; wait; "));
    }
    let mut start: Vec<String> = ["cmd", "activity", "start", "-W", "-n"].iter().map(|s| s.to_string()).collect();
    // The looked-up one is a component name, a single word: unquoted, the shell expands it.
    start.push(activity.map_or("$a".to_string(), sh_quote));
    start.extend(["-a", "android.intent.action.MAIN", "-c", "android.intent.category.LAUNCHER", "-f", "0x10200000"].iter().map(|s| sh_quote(s)));
    for (k, v) in &o.arguments {
        let flag = match v.as_str() {
            "true" | "false" => "--ez",
            v if v.parse::<i32>().is_ok() && (v == "0" || !v.starts_with('0')) => "--ei",
            _ => "--es",
        };
        start.extend([sh_quote(flag), sh_quote(k), sh_quote(v)]);
    }
    script.push_str(&start.join(" "));
    script
}

fn not_launchable(app: &str) -> String {
    format!("{app} is not installed, or has no launcher activity")
}

/// A held action's reply: an error when some of a `type`'s text could not be typed, or an
/// `exec`'s program failed.
fn action_done(action: &Value, reply: &Value) -> Result<()> {
    match action["op"].as_str() {
        Some("type") => {
            let skipped = reply["skipped"].as_i64().unwrap_or(0);
            if skipped > 0 {
                bail!("{skipped} characters of '{}' could not be typed: is a text field focused?", action["text"].as_str().unwrap_or(""));
            }
        }
        Some("exec") => {
            let code = reply["code"].as_i64().unwrap_or(-1);
            if code != 0 {
                let argv: Vec<&str> = action["argv"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                bail!("{} failed ({code}): {}", argv.join(" "), reply["out"].as_str().unwrap_or("").trim());
            }
        }
        _ => {}
    }
    Ok(())
}

/// A settle request: done once no UI event has come for SETTLE_QUIET_MS (or none at all for
/// `no_change`), at most `timeout`.
fn settle_req(no_change: Duration, timeout: Duration) -> Value {
    let mut settle = json!({
        "op": "settle",
        "quiet": SETTLE_QUIET_MS,
        "nochange": no_change.as_millis() as u64,
        "timeout": timeout.as_millis() as u64,
    });
    // The settle's workings (events, probes, which packages sent events) for the trace.
    if *TRACE {
        settle["trace"] = json!(true);
    }
    settle
}

/// Permission changes gathered for one device script (`set_permissions`).
#[derive(Default)]
struct Batch {
    script: String,
    /// The changes in it (`all` counts as many).
    count: usize,
    /// The last change, sent as an exec of its own when it is the only one.
    lone: Option<(&'static str, String)>,
}

impl Batch {
    /// Runs what is gathered. A permission the app does not request cannot be changed; that is
    /// not an error.
    fn run(&mut self, dev: &mut AndroidDevice, app: &str) -> Result<()> {
        match (self.count, self.lone.take()) {
            (0, _) => {}
            (1, Some((verb, p))) => {
                let _ = dev.exec(&["cmd", "package", verb, app, &p])?;
            }
            _ => {
                let _ = dev.exec(&["sh", "-c", &self.script])?;
            }
        }
        self.script.clear();
        self.count = 0;
        Ok(())
    }
}

/// What a reply with a settle in it says, read past its screen (when it brings one) without
/// building it into values.
#[derive(serde::Deserialize, Default)]
struct Outcome {
    #[serde(default)]
    status: i64,
    #[serde(default)]
    changed: bool,
    /// A `tapIf` read found the lookup's screen and tapped.
    #[serde(default)]
    tapped: bool,
    /// A held action's: a `type`'s characters not typed, an `exec`'s code and output.
    #[serde(default)]
    skipped: i64,
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    out: Option<String>,
}

impl Outcome {
    fn settled(&self) -> Settled {
        Settled { changed: self.changed, timed_out: self.status == 2 }
    }
}

/// How many error dialogs one screen read answers before it gives up.
const MAX_ERROR_DIALOGS: usize = 3;

pub struct AndroidDevice {
    adb: Adb,
    agent: AndroidAgent,
    width: i32,
    height: i32,
    rotation: i32,
    launchers: HashMap<String, String>,
    recording: Option<(Child, PathBuf)>,
    /// An action (tap, key, swipe, press, drag, typing) not sent yet: it goes with the settle
    /// or read that follows it, in one round trip, or on its own before any other request.
    pending: Option<Value>,
    /// Requests sent to the agent so far.
    requests: u64,
    /// The agent's event count at the last request, when it was a screen read: what the
    /// caller holds is current until the count moves.
    last_seq: Option<u64>,
    /// A keyboard await_keyboard saw up, with the request count then and the event count the
    /// agent gave: hide_keyboard's first look, when no request went in between.
    keyboard_up: Option<(u64, Option<u64>)>,
    /// The next step starts with a lookup: settles bring the screen back (`next_step_reads`).
    read_next: bool,
    /// The screen the last settle brought, with the request count then and its event count:
    /// the next read's, when no request went in between.
    kept: Option<(u64, ScreenTree, Option<u64>)>,
    /// The tree the last screen() gave out, with the request count then: while no request goes
    /// in between and no event comes (`last_seq`), the next read is that tree again.
    last_read: Option<(u64, ScreenTree)>,
}

impl AndroidDevice {
    pub fn connect(serial: &str) -> Result<Self> {
        Self::connect_with(serial, false)
    }

    /// Connects, starting the agent; `animations_off` turns the device's animations off on
    /// the way (see [`AndroidDevice::disable_animations`]).
    pub fn connect_with(serial: &str, animations_off: bool) -> Result<Self> {
        let adb = Adb::new(serial);
        let agent = AndroidAgent::start_with(&adb, animations_off)?;
        let display = agent.display();
        Ok(AndroidDevice {
            adb,
            agent,
            width: display.width,
            height: display.height,
            rotation: display.rotation,
            launchers: HashMap::new(),
            recording: None,
            pending: None,
            requests: 0,
            last_seq: None,
            keyboard_up: None,
            read_next: false,
            kept: None,
            last_read: None,
        })
    }

    /// Turns window, transition and animator animations off, as test runners do: screens
    /// change at once, so each step can start sooner and settle detection has less to wait out.
    pub fn disable_animations(&mut self) -> Result<()> {
        const KEYS: &str = "window_animation_scale transition_animation_scale animator_duration_scale";
        let now = self.exec_ok(&["sh", "-c", &format!("for k in {KEYS}; do cmd settings get global $k; done")])?;
        if now.split_whitespace().all(|v| v == "0" || v == "0.0") {
            return Ok(());
        }
        self.exec_ok(&["sh", "-c", &format!("for k in {KEYS}; do cmd settings put global $k 0; done")])?;
        Ok(())
    }

    pub fn adb(&self) -> &Adb {
        &self.adb
    }

    /// How many requests have gone to the agent: each costs device CPU, a polling loop many.
    pub fn requests(&self) -> u64 {
        self.requests
    }

    /// Holds an action back so the next settle can travel with it.
    fn act(&mut self, req: Value) -> Result<()> {
        self.flush()?;
        self.pending = Some(req);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        match self.pending.take() {
            Some(action) => {
                let r = self.call_now(action.clone())?;
                action_done(&action, &r)
            }
            None => Ok(()),
        }
    }

    /// One agent request, after any action held back.
    fn call(&mut self, req: Value) -> Result<Value> {
        let req = self.carry(req)?;
        self.call_now(req)
    }

    /// `req`, with an action held back: a read (windows, screen) goes as the action's `then`,
    /// which the agent runs right after it, as it would a request of its own, one round trip
    /// sooner (the replies' fields do not overlap). Anything else sends the action first.
    fn carry(&mut self, req: Value) -> Result<Value> {
        match self.pending.take() {
            // Not typing or a program: their replies say what failed, which the read's caller
            // would not look at.
            Some(mut action) if matches!(req["op"].as_str(), Some("windows" | "screen")) && !matches!(action["op"].as_str(), Some("type" | "exec")) => {
                action["then"] = req;
                Ok(action)
            }
            Some(action) => {
                self.pending = Some(action);
                self.flush()?;
                Ok(req)
            }
            None => Ok(req),
        }
    }

    /// One agent request. When the agent is gone (it crashed, or Android killed it) it is
    /// started again and the request sent once more.
    fn call_now(&mut self, req: Value) -> Result<Value> {
        match self.call_once(req.clone()) {
            Err(e) if agent_lost(&e) => {
                if *TRACE {
                    eprintln!("[trace] agent lost ({e:#}): restarting it");
                }
                self.agent.restart().map_err(|start| anyhow!("{e:#}; restarting the agent failed: {start:#}"))?;
                self.call_once(req)
            }
            r => r,
        }
    }

    /// `call_now` for a reply parsed by the caller: its bytes.
    fn call_raw_now(&mut self, req: &Value) -> Result<Vec<u8>> {
        let carried;
        let req = if self.pending.is_some() {
            carried = self.carry(req.clone())?;
            &carried
        } else {
            req
        };
        self.requests += 1;
        self.last_seq = None;
        let t = Instant::now();
        let r = match self.agent.client().call_raw(req) {
            Err(e) if agent_lost(&e) => {
                self.agent.restart().map_err(|start| anyhow!("{e:#}; restarting the agent failed: {start:#}"))?;
                self.agent.client().call_raw(req)
            }
            r => r,
        };
        if *TRACE {
            let summary = match &r {
                Ok(b) => format!("<{} bytes>", b.len()),
                Err(e) => format!("error: {e}"),
            };
            eprintln!("[trace] {:>7.1} ms {req} -> {summary}", t.elapsed().as_secs_f64() * 1000.0);
        }
        r
    }

    fn call_once(&mut self, req: Value) -> Result<Value> {
        self.requests += 1;
        self.last_seq = None;
        if !*TRACE {
            return self.agent.client().call(req);
        }
        let t = Instant::now();
        let r = self.agent.client().call(req.clone());
        let summary = match &r {
            Ok(v) => {
                let mut v = v.clone();
                for big in ["nodes", "png", "out", "windows"] {
                    if let Some(x) = v.get_mut(big) {
                        *x = Value::String(format!("<{} bytes>", x.to_string().len()));
                    }
                }
                v.to_string()
            }
            Err(e) => format!("error: {e}"),
        };
        let mut req_s = req.to_string();
        req_s.truncate(160);
        eprintln!("[trace] {:>7.1} ms {req_s} -> {summary}", t.elapsed().as_secs_f64() * 1000.0);
        r
    }

    /// A settle request, bringing the screen it settled on back when the next step reads it.
    fn settle_req(&self, no_change: Duration, timeout: Duration) -> Value {
        let mut settle = settle_req(no_change, timeout);
        if self.read_next {
            settle["read"] = json!(true);
        }
        settle
    }

    fn refresh_info(&mut self) -> Result<()> {
        let info = self.call(req("info", []))?;
        self.width = info["width"].as_i64().unwrap_or(0) as i32;
        self.height = info["height"].as_i64().unwrap_or(0) as i32;
        self.rotation = info["rotation"].as_i64().unwrap_or(0) as i32;
        Ok(())
    }

    /// Runs a program on the device as the shell user; no shell parses `argv`.
    /// One round trip to the agent that does nothing on the device.
    pub fn ping(&mut self) -> Result<()> {
        self.call(req("ping", [])).map(|_| ())
    }

    /// The text on the device clipboard.
    pub fn clipboard(&mut self) -> Result<String> {
        let r = self.call(req("clipboard", []))?;
        Ok(r["text"].as_str().unwrap_or("").to_string())
    }

    /// (Device commands call `cmd <service>` straight away: am, pm and settings are shell
    /// scripts that only run it, a shell started for nothing.)
    pub fn exec(&mut self, argv: &[&str]) -> Result<(i64, String)> {
        let mut r = self.call(req("exec", [("argv", json!(argv))]))?;
        // The output taken out of the reply, not copied (a launch's filtered dumpsys).
        let out = match r["out"].take() {
            Value::String(out) => out,
            _ => String::new(),
        };
        Ok((r["code"].as_i64().unwrap_or(-1), out))
    }

    /// `exec`, failing with the program's output when it fails.
    fn exec_ok(&mut self, argv: &[&str]) -> Result<String> {
        let (code, out) = self.exec(argv)?;
        if code != 0 {
            bail!("{} failed ({code}): {}", argv.join(" "), out.trim());
        }
        Ok(out)
    }


    /// Waits (a little) until a window of `app` is the active one.
    /// `first`: the windows (and event count) already in hand for the first look.
    fn wait_for_app(&mut self, app: &str, mut first: Option<(Vec<Value>, Option<u64>)>) -> Result<()> {
        let deadline = Instant::now() + APP_WINDOW_WAIT;
        let mut seq = None;
        loop {
            let (w, now_seq) = match first.take() {
                Some(seen) => seen,
                None => self.windows(seq, deadline)?,
            };
            if w.iter().any(|w| w["pkg"] == app && w["active"] == true) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            // A system window may be an error dialog standing in the way: a full read answers it.
            if w.iter().any(|w| w["pkg"] == "android") {
                self.screen()?;
            }
            seq = now_seq;
        }
    }

    /// The windows on screen, without their nodes, and the agent's event count. With `after`
    /// (an earlier count) the agent first waits for an event after it, at most until
    /// `deadline` or CHANGE_WAIT.
    fn windows(&mut self, after: Option<u64>, deadline: Instant) -> Result<(Vec<Value>, Option<u64>)> {
        let mut req = req("windows", []);
        if let Some(seq) = after {
            let wait = deadline.saturating_duration_since(Instant::now()).min(CHANGE_WAIT);
            req["after"] = json!(seq);
            req["waitMs"] = json!(wait.as_millis() as u64);
        }
        let mut r = self.call(req)?;
        let seq = r["seq"].as_u64();
        let windows = match r["windows"].take() {
            Value::Array(w) => w,
            _ => Vec::new(),
        };
        Ok((windows, seq))
    }

    /// The app's runtime permissions not granted yet. `dumpsys package` prints ~200 KB; only
    /// the permission lines (and the user headers that end a section) leave the device.
    pub fn ungranted_permissions(&mut self, app: &str) -> Result<Vec<String>> {
        if !plain_name(app) {
            bail!("unexpected app id {app}");
        }
        let dump = self.exec_ok(&["sh", "-c", &format!("dumpsys package {app} | grep -E '{PERMISSION_LINES}'")])?;
        Ok(ungranted_runtime_permissions(&dump))
    }

    fn setting(&mut self, ns: &str, key: &str) -> Result<String> {
        Ok(self.exec_ok(&["cmd", "settings", "get", ns, key])?.trim().to_string())
    }
}

impl Device for AndroidDevice {
    fn platform(&self) -> &'static str {
        "android"
    }

    fn id(&self) -> String {
        self.adb.serial.clone()
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        let mut answered = 0;
        loop {
            let last = self.last_read.take().filter(|(at, _)| *at == self.requests);
            let (tree, seq) = match self.kept.take().filter(|(at, ..)| *at == self.requests) {
                Some((_, tree, seq)) => (tree, seq),
                None => {
                    // Nothing sent since the last read: the agent says whether an event came
                    // since (no dump, no parse), else sends the read in the same reply.
                    let since = last.as_ref().and(self.last_seq);
                    let body = match since {
                        Some(seq) => self.call_raw_now(&req("screen", [("after", json!(seq)), ("waitMs", json!(0))]))?,
                        None => self.call_raw_now(&req("screen", []))?,
                    };
                    let reply = ScreenReply::parse(&body, self.width, self.height)?;
                    match last {
                        Some((_, tree)) if reply.unchanged => (tree, since),
                        _ => (reply.tree, reply.seq),
                    }
                }
            };
            self.last_seq = seq;
            self.last_read = Some((self.requests, tree.clone()));
            match error_dialog_button(&tree) {
                Some((x, y)) if answered < MAX_ERROR_DIALOGS => {
                    answered += 1;
                    self.tap(x, y)?;
                    self.settle(Duration::from_millis(300), Duration::from_secs(3))?;
                }
                _ => return Ok(tree),
            }
        }
    }

    /// The size a read would give (a reply is parsed against this same size), without one.
    /// setOrientation updates it.
    fn screen_rect(&mut self) -> Result<crate::screen::Rect> {
        Ok(crate::screen::Rect { left: 0, top: 0, right: self.width, bottom: self.height })
    }

    fn fresh_screen(&mut self) -> Result<ScreenTree> {
        let body = self.call_raw_now(&req("screen", [("fresh", json!(true))]))?;
        let reply = ScreenReply::parse(&body, self.width, self.height)?;
        self.last_seq = reply.seq;
        Ok(reply.tree)
    }

    fn screen_after_change(&mut self, current: ScreenTree, wait: Duration) -> Result<ScreenTree> {
        let Some(seq) = self.last_seq else {
            // Something else happened since `current` was read: read afresh.
            return self.screen();
        };
        let body = self.call_raw_now(&req("screen", [("after", json!(seq)), ("waitMs", json!(wait.as_millis() as u64))]))?;
        let reply = ScreenReply::parse(&body, self.width, self.height)?;
        if reply.unchanged {
            self.last_seq = Some(seq);
            return Ok(current);
        }
        self.last_seq = reply.seq;
        if error_dialog_button(&reply.tree).is_some() {
            return self.screen();
        }
        Ok(reply.tree)
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<()> {
        self.act(req("tap", [("x", json!(x)), ("y", json!(y))]))
    }

    /// Each tap but the last goes out at once: held back, it went only with the next one, the
    /// two too close together for a double tap. The last travels with the settle after it.
    fn taps(&mut self, x: i32, y: i32, count: u32, interval: Duration) -> Result<()> {
        for n in 0..count.max(1) {
            if n > 0 {
                self.flush()?;
                std::thread::sleep(interval);
            }
            self.tap(x, y)?;
        }
        Ok(())
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.act(req("press", [("x", json!(x)), ("y", json!(y)), ("ms", json!(ms))]))
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, end: Lift) -> Result<()> {
        let (fling, cancel) = (end == Lift::Fling, end == Lift::Cancel);
        self.act(req(
            "swipe",
            [("x1", json!(from.0)), ("y1", json!(from.1)), ("x2", json!(to.0)), ("y2", json!(to.1)), ("ms", json!(ms)), ("fling", json!(fling)), ("cancel", json!(cancel))],
        ))
    }

    fn press_key(&mut self, key: Key) -> Result<()> {
        self.act(req("key", [("code", json!(keycode(key)))]))
    }

    /// Held, like a tap: the settle after it travels with it. What could not be typed fails
    /// the step when the reply comes (after that settle).
    fn input_text(&mut self, text: &str) -> Result<()> {
        self.act(req("type", [("text", json!(text))]))
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        self.act(req("erase", [("count", json!(chars))]))
    }

    /// Held: the settle that follows (waitForAnimationToEnd) carries it.
    fn mark(&mut self) -> Result<()> {
        self.act(req("mark", []))
    }

    /// The agent reads the screen afresh and, when it is the lookup's (the same fingerprint),
    /// taps and settles in the same request; else it sends the read, untapped. The fresh read
    /// before the tap still happens, on the device, where shipping it to compare cost a round
    /// trip and a whole dump.
    fn tap_if_unchanged_and_settle(&mut self, screen: &ScreenTree, x: i32, y: i32, no_change: Duration, timeout: Duration) -> Result<std::result::Result<Settled, ScreenTree>> {
        self.flush()?;
        let check = json!({"fp": format!("{:x}", screen.fingerprint()), "x": x, "y": y});
        let then = self.settle_req(no_change, timeout);
        let body = self.call_raw_now(&req("screen", [("fresh", json!(true)), ("tapIf", check), ("then", then)]))?;
        let reply = ScreenReply::parse(&body, self.width, self.height)?;
        let outcome: Outcome = serde_json::from_slice(&body)?;
        if !outcome.tapped {
            self.last_seq = reply.seq;
            return Ok(Err(reply.tree));
        }
        if reply.has_screen {
            self.kept = Some((self.requests, reply.tree, reply.seq));
        }
        Ok(Ok(outcome.settled()))
    }

    fn next_step_reads(&mut self, reads: bool) {
        self.read_next = reads;
    }

    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let settle = self.settle_req(no_change, timeout);
        if self.read_next {
            // The screen comes back with the settle: parsed straight into a tree, and kept.
            let action = self.pending.take();
            let req = match &action {
                Some(action) => {
                    let mut action = action.clone();
                    action["then"] = settle;
                    action
                }
                None => settle,
            };
            let body = self.call_raw_now(&req)?;
            let reply = ScreenReply::parse(&body, self.width, self.height)?;
            let outcome: Outcome = serde_json::from_slice(&body)?;
            if let Some(action) = &action {
                action_done(action, &json!({"skipped": outcome.skipped, "code": outcome.code, "out": outcome.out}))?;
            }
            if reply.has_screen {
                self.kept = Some((self.requests, reply.tree, reply.seq));
            }
            return Ok(outcome.settled());
        }
        let r = match self.pending.take() {
            Some(mut action) => {
                action["then"] = settle;
                let r = self.call_now(action.clone())?;
                action_done(&action, &r)?;
                r
            }
            None => self.call(settle)?,
        };
        Ok(Settled { changed: r["changed"] == true, timed_out: r["status"] == 2 })
    }

    fn launch_app(&mut self, app: &str, o: &LaunchOptions) -> Result<()> {
        if !plain_name(app) {
            bail!("unexpected app id {app}");
        }
        if o.clear_keychain {
            self.clear_keychain()?;
        }
        // The launcher activity is looked up in the launch's own script the first time.
        let script = launch_script(app, self.launchers.get(app).map(String::as_str), o);
        // The first look at the windows goes with it: the agent reads them as soon as the
        // start has returned, which is when that look came anyway, one round trip later.
        let mut r = self.call(json!({"op": "exec", "argv": ["sh", "-c", script], "mark": true, "then": {"op": "windows"}}))?;
        let out = r["out"].as_str().unwrap_or("");
        if let Some(a) = out.lines().find_map(|l| l.strip_prefix("launcher ")) {
            self.launchers.insert(app.to_string(), a.trim().to_string());
        }
        if r["code"].as_i64() == Some(3) && out.contains(&not_launchable(app)) {
            bail!("{}", not_launchable(app));
        }
        if r["code"].as_i64() != Some(0) {
            bail!("launching {app} failed ({}): {}", r["code"], r["out"].as_str().unwrap_or("").trim());
        }
        let first = match r["windows"].take() {
            Value::Array(w) => Some((w, r["seq"].as_u64())),
            _ => None,
        };
        self.wait_for_app(app, first)?;
        self.settle(Duration::ZERO, Duration::from_secs(10))?;
        Ok(())
    }

    /// One request: the exec marks the settle's start and the settle goes with it. A failed
    /// force-stop is reported after that settle.
    fn stop_app(&mut self, app: &str) -> Result<()> {
        self.act(json!({"op": "exec", "mark": true, "argv": ["cmd", "activity", "force-stop", app]}))?;
        self.settle(Duration::from_millis(300), Duration::from_secs(5))?;
        Ok(())
    }

    fn kill_app(&mut self, app: &str) -> Result<()> {
        // `am kill` leaves a foreground app running; killing it as the system would is a stop.
        self.stop_app(app)
    }

    fn clear_state(&mut self, app: &str) -> Result<()> {
        self.exec_ok(&["cmd", "package", "clear", app]).map(|_| ())
    }

    fn open_link(&mut self, link: &str, _app: Option<&str>, browser: bool) -> Result<()> {
        let mut argv = vec!["cmd", "activity", "start", "-a", "android.intent.action.VIEW", "-d", link];
        if browser {
            argv.extend(["-c", "android.intent.category.BROWSABLE"]);
        }
        // Held, marking the settle's start: the runner's settle goes with it. A failed start
        // is reported after that settle.
        self.act(json!({"op": "exec", "mark": true, "argv": argv}))
    }

    /// The windows say whether a keyboard is up (a few ms); only when none is does a field's
    /// focus need a whole read past the cache (the keyboard's keys and all).
    fn await_keyboard(&mut self, wait: Duration, poll: Duration) -> Result<()> {
        let (w, seq) = self.windows(None, Instant::now())?;
        if has_keyboard(&w) {
            self.keyboard_up = Some((self.requests, seq));
            return Ok(());
        }
        crate::device::await_keyboard_reading(self, wait, poll)
    }

    fn hide_keyboard(&mut self) -> Result<()> {
        let seen = self.keyboard_up.take().filter(|&(at, _)| at == self.requests);
        let dev = std::cell::RefCell::new(self);
        let mut seen = seen.map(|(_, seq)| seq);
        close_keyboard(
            |after, deadline| {
                if let Some(seq) = seen.take() {
                    return Ok((true, seq));
                }
                let (w, seq) = dev.borrow_mut().windows(after, deadline)?;
                Ok((has_keyboard(&w), seq))
            },
            || dev.borrow_mut().press_key(Key::Back),
            KEYBOARD_CLOSE_LIMIT,
        )
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        self.flush()?;
        self.requests += 1;
        self.last_seq = None;
        let (_, png) = self.agent.client().call_with_blob(req("screenshot", []))?;
        if png.is_empty() {
            bail!("the agent sent no screenshot");
        }
        Ok(png)
    }

    fn set_orientation(&mut self, o: Orientation) -> Result<()> {
        let rot = match o {
            Orientation::Portrait => 0,
            Orientation::LandscapeLeft => 1,
            Orientation::UpsideDown => 2,
            Orientation::LandscapeRight => 3,
        };
        self.exec_ok(&["cmd", "settings", "put", "system", "accelerometer_rotation", "0"])?;
        self.exec_ok(&["cmd", "settings", "put", "system", "user_rotation", &rot.to_string()])?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.refresh_info()?;
            if self.rotation == rot {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("the screen did not rotate");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The agent sets the test providers' location and sends it again every 2 s while this
    /// host stays: sent once, an app that began watching a moment later never got it.
    fn set_location(&mut self, lat: f64, lon: f64) -> Result<()> {
        self.call(req("location", [("lat", json!(lat)), ("lon", json!(lon))])).map(|_| ())
    }

    fn set_airplane_mode(&mut self, on: Option<bool>) -> Result<()> {
        let on = match on {
            Some(v) => v,
            None => self.setting("global", "airplane_mode_on")? != "1",
        };
        self.exec_ok(&["cmd", "connectivity", "airplane-mode", if on { "enable" } else { "disable" }]).map(|_| ())
    }

    /// Every change in one device script, each entry's side by side (a process start each,
    /// overlapped) and the entries in order; `all` lists the app's runtime permissions on the
    /// device. One change alone is an exec of its own (a shell would only add its start), and so
    /// is a name a shell could read as more than a word.
    fn set_permissions(&mut self, app: &str, perms: &[(String, String)]) -> Result<()> {
        let plain_app = plain_name(app);
        let mut batch = Batch::default();
        for (name, value) in perms {
            let all = name.eq_ignore_ascii_case("all");
            let verb = match value.to_ascii_lowercase().as_str() {
                "allow" => "grant",
                "deny" | "unset" => "revoke",
                other => {
                    batch.run(self, app)?;
                    bail!("permission value `{other}` is not allow, deny or unset")
                }
            };
            if all && plain_app {
                batch.script.push_str(&format!("for p in $({}); do cmd package {verb} {app} $p >/dev/null 2>&1 & done; wait; ", permission_names(app, false)));
                batch.count += 2;
                continue;
            }
            let list = if all {
                let dump = self.exec_ok(&["sh", "-c", &format!("dumpsys package {} | grep -E '{PERMISSION_LINES}'", sh_quote(app))])?;
                runtime_permissions(&dump)
            } else {
                android_permissions(name)
            };
            if list.is_empty() && !all {
                batch.run(self, app)?;
                bail!("`{name}` is not a permission virtuoso knows");
            }
            for p in list {
                if plain_app && plain_name(&p) {
                    batch.script.push_str(&format!("cmd package {verb} {app} {p} >/dev/null 2>&1 & "));
                    batch.count += 1;
                    batch.lone = Some((verb, p));
                } else {
                    batch.run(self, app)?;
                    // A permission the app does not request cannot be changed; not an error.
                    let _ = self.exec(&["cmd", "package", verb, app, &p]);
                }
            }
            if batch.count > 0 {
                batch.script.push_str("wait; ");
            }
        }
        batch.run(self, app)
    }

    fn add_media(&mut self, files: &[&Path]) -> Result<()> {
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let ext = f.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            let video = matches!(ext.as_deref(), Some("mp4" | "mov" | "3gp" | "webm" | "mkv"));
            let remote = format!("/sdcard/{}/{name}", if video { "Movies" } else { "Pictures" });
            self.adb.run(&["push", &f.to_string_lossy(), &remote])?;
            let _ = self.exec(&["cmd", "activity", "broadcast", "-a", "android.intent.action.MEDIA_SCANNER_SCAN_FILE", "-d", &format!("file://{remote}")]);
        }
        Ok(())
    }

    fn drag(&mut self, from: (i32, i32), to: (i32, i32), hold_ms: u64, move_ms: u64) -> Result<()> {
        self.act(req("drag", [("x1", json!(from.0)), ("y1", json!(from.1)), ("x2", json!(to.0)), ("y2", json!(to.1)), ("hold", json!(hold_ms)), ("ms", json!(move_ms))]))
    }

    fn set_clipboard(&mut self, text: &str) -> Result<()> {
        self.call(req("clipboard", [("text", json!(text))])).map(|_| ())
    }

    fn open_notifications(&mut self) -> Result<()> {
        // An app still starting takes the focus back and the shade closes again: ask until the
        // shade has the focus.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            self.exec_ok(&["cmd", "statusbar", "expand-notifications"])?;
            std::thread::sleep(Duration::from_millis(250));
            let focus = self.exec_ok(&["sh", "-c", "dumpsys window | grep mCurrentFocus"])?;
            if focus.contains("NotificationShade") {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                bail!("the notification shade did not open (focus: {})", focus.trim());
            }
        }
    }

    fn set_dark_mode(&mut self, on: Option<bool>) -> Result<()> {
        let on = match on {
            Some(v) => v,
            None => !self.dark_mode()?,
        };
        self.exec_ok(&["cmd", "uimode", "night", if on { "yes" } else { "no" }]).map(|_| ())
    }

    fn dark_mode(&mut self) -> Result<bool> {
        Ok(self.exec_ok(&["cmd", "uimode", "night"])?.to_ascii_lowercase().contains("yes"))
    }

    fn start_recording(&mut self, path: &Path) -> Result<()> {
        if self.recording.is_some() {
            bail!("a recording is already running");
        }
        let child = self
            .adb
            .command()
            .args(["shell", "screenrecord", "--bit-rate", "4000000", "/sdcard/virtuoso-recording.mp4"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        self.recording = Some((child, path.to_path_buf()));
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<()> {
        let (mut child, path) = self.recording.take().ok_or_else(|| anyhow!("no recording is running"))?;
        let _ = self.exec(&["pkill", "-INT", "screenrecord"]);
        let _ = child.wait();
        // The adb shell ends when screenrecord has written the file and exited; what is left is
        // to make sure it is gone (one look, then up to 1 s), not a fixed 300 ms after it.
        if self.exec(&["pidof", "screenrecord"]).is_ok_and(|(code, _)| code == 0) {
            let _ = self.exec(&["sh", "-c", "for i in $(seq 50); do pidof screenrecord >/dev/null || exit 0; sleep 0.02; done"]);
        }
        self.adb.run(&["pull", "/sdcard/virtuoso-recording.mp4", &path.to_string_lossy()])?;
        let _ = self.exec(&["rm", "/sdcard/virtuoso-recording.mp4"]);
        Ok(())
    }
}

/// Android KeyEvent codes.
pub fn keycode(k: Key) -> i32 {
    match k {
        Key::Enter => 66,
        Key::Backspace => 67,
        Key::Back => 4,
        Key::Home => 3,
        Key::Lock | Key::Power => 26,
        Key::Tab => 61,
        Key::Escape => 111,
        Key::Space => 62,
        Key::Menu => 82,
        Key::Search => 84,
        Key::VolumeUp => 24,
        Key::VolumeDown => 25,
        Key::Camera => 27,
        Key::DpadUp => 19,
        Key::DpadDown => 20,
        Key::DpadLeft => 21,
        Key::DpadRight => 22,
        Key::DpadCenter => 23,
        Key::MediaPlayPause => 85,
        Key::MediaStop => 86,
        Key::MediaNext => 87,
        Key::MediaPrevious => 88,
        Key::MediaRewind => 89,
        Key::MediaFastForward => 90,
    }
}
