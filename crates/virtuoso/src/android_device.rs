//! An Android device driven through the agent: the [`Device`] the runner uses.
//!
//! Device commands (app control, settings) run through the agent's `exec`, as the shell user,
//! which saves the ~30 ms an `adb shell` round trip costs.

use crate::android::{android_permissions, runtime_permissions, ungranted_runtime_permissions, Adb, AndroidAgent};
use crate::device::{Device, Key, LaunchOptions, Orientation, Settled};
use crate::screen::{ScreenReply, ScreenTree};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

static TRACE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("VIRTUOSO_DEBUG").is_some());

/// TYPE_INPUT_METHOD in AccessibilityWindowInfo.
const WINDOW_TYPE_IME: i64 = 2;
/// How long after `am start -W` returns to wait for a window of the app itself. An app's task
/// may show another package's activity on top (Settings' search), so this does not fail.
const APP_WINDOW_WAIT: Duration = Duration::from_secs(3);
/// The longest one request waits on the agent for a UI event before answering anyway.
const CHANGE_WAIT: Duration = Duration::from_millis(200);

fn has_keyboard(windows: &[Value]) -> bool {
    windows.iter().any(|w| w["type"].as_i64() == Some(WINDOW_TYPE_IME))
}

/// After the last UI event, how long the UI must stay quiet to count as settled.
const SETTLE_QUIET_MS: u64 = 80;

/// Where to tap to answer a system error dialog on screen ("X isn't responding": Wait;
/// "X keeps stopping" / "X has stopped": Close app), or None when there is none. A freshly
/// booted emulator often shows one over everything, and it would swallow the flow's taps.
pub fn error_dialog_button(s: &ScreenTree) -> Option<(i32, i32)> {
    let system = |i: usize| s.elements[i].package == "android";
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
fn req(op: &str, fields: &[(&str, Value)]) -> Value {
    let mut m = serde_json::Map::with_capacity(fields.len() + 1);
    m.insert("op".into(), Value::from(op));
    for (k, v) in fields {
        m.insert((*k).into(), v.clone());
    }
    Value::Object(m)
}

/// A package or permission name: nothing a shell would read as more than a word.
fn plain_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
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
    /// A tap, key or swipe not sent yet: it goes with the settle that follows it, in one round
    /// trip, or on its own before any other request.
    pending: Option<Value>,
    /// Requests sent to the agent so far.
    requests: u64,
    /// The agent's event count at the last request, when it was a screen read: what the
    /// caller holds is current until the count moves.
    last_seq: Option<u64>,
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
        })
    }

    /// Turns window, transition and animator animations off, as test runners do: screens
    /// change at once, so each step can start sooner and settle detection has less to wait out.
    pub fn disable_animations(&mut self) -> Result<()> {
        const KEYS: &str = "window_animation_scale transition_animation_scale animator_duration_scale";
        let now = self.exec_ok(&["sh", "-c", &format!("for k in {KEYS}; do settings get global $k; done")])?;
        if now.split_whitespace().all(|v| v == "0" || v == "0.0") {
            return Ok(());
        }
        self.exec_ok(&["sh", "-c", &format!("for k in {KEYS}; do settings put global $k 0; done")])?;
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
            Some(action) => self.call_now(action).map(|_| ()),
            None => Ok(()),
        }
    }

    /// One agent request, after any action held back.
    fn call(&mut self, req: Value) -> Result<Value> {
        self.flush()?;
        self.call_now(req)
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
        self.flush()?;
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

    fn refresh_info(&mut self) -> Result<()> {
        let info = self.call(req("info", &[]))?;
        self.width = info["width"].as_i64().unwrap_or(0) as i32;
        self.height = info["height"].as_i64().unwrap_or(0) as i32;
        self.rotation = info["rotation"].as_i64().unwrap_or(0) as i32;
        Ok(())
    }

    /// Runs a program on the device as the shell user; no shell parses `argv`.
    /// One round trip to the agent that does nothing on the device.
    pub fn ping(&mut self) -> Result<()> {
        self.call(req("ping", &[])).map(|_| ())
    }

    /// The text on the device clipboard.
    pub fn clipboard(&mut self) -> Result<String> {
        let r = self.call(req("clipboard", &[]))?;
        Ok(r["text"].as_str().unwrap_or("").to_string())
    }

    pub fn exec(&mut self, argv: &[&str]) -> Result<(i64, String)> {
        let r = self.call(req("exec", &[("argv", json!(argv))]))?;
        Ok((r["code"].as_i64().unwrap_or(-1), r["out"].as_str().unwrap_or("").to_string()))
    }

    /// `exec`, failing with the program's output when it fails.
    fn exec_ok(&mut self, argv: &[&str]) -> Result<String> {
        let (code, out) = self.exec(argv)?;
        if code != 0 {
            bail!("{} failed ({code}): {}", argv.join(" "), out.trim());
        }
        Ok(out)
    }


    fn launcher_activity(&mut self, app: &str) -> Result<String> {
        if let Some(a) = self.launchers.get(app) {
            return Ok(a.clone());
        }
        let out = self.exec_ok(&["cmd", "package", "resolve-activity", "--brief", "-c", "android.intent.category.LAUNCHER", app])?;
        let comp = out.lines().map(str::trim).rfind(|l| l.contains('/')).map(str::to_string);
        let comp = comp.ok_or_else(|| anyhow!("{app} is not installed, or has no launcher activity"))?;
        self.launchers.insert(app.to_string(), comp.clone());
        Ok(comp)
    }

    /// Waits (a little) until a window of `app` is the active one.
    fn wait_for_app(&mut self, app: &str) -> Result<()> {
        let deadline = Instant::now() + APP_WINDOW_WAIT;
        let mut seq = None;
        loop {
            let (w, now_seq) = self.windows(seq, deadline)?;
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
        let mut req = req("windows", &[]);
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
        Ok(self.exec_ok(&["settings", "get", ns, key])?.trim().to_string())
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
            let body = self.call_raw_now(&req("screen", &[]))?;
            let reply = ScreenReply::parse(&body, self.width, self.height)?;
            let tree = reply.tree;
            self.last_seq = reply.seq;
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

    fn fresh_screen(&mut self) -> Result<ScreenTree> {
        let body = self.call_raw_now(&req("screen", &[("fresh", json!(true))]))?;
        let reply = ScreenReply::parse(&body, self.width, self.height)?;
        self.last_seq = reply.seq;
        Ok(reply.tree)
    }

    fn screen_after_change(&mut self, current: ScreenTree, wait: Duration) -> Result<ScreenTree> {
        let Some(seq) = self.last_seq else {
            // Something else happened since `current` was read: read afresh.
            return self.screen();
        };
        let body = self.call_raw_now(&req("screen", &[("after", json!(seq)), ("waitMs", json!(wait.as_millis() as u64))]))?;
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
        self.act(req("tap", &[("x", json!(x)), ("y", json!(y))]))
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.call(req("press", &[("x", json!(x)), ("y", json!(y)), ("ms", json!(ms))])).map(|_| ())
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, fling: bool) -> Result<()> {
        self.act(req("swipe", &[("x1", json!(from.0)), ("y1", json!(from.1)), ("x2", json!(to.0)), ("y2", json!(to.1)), ("ms", json!(ms)), ("fling", json!(fling))]))
    }

    fn press_key(&mut self, key: Key) -> Result<()> {
        self.act(req("key", &[("code", json!(keycode(key)))]))
    }

    fn input_text(&mut self, text: &str) -> Result<()> {
        let r = self.call(req("type", &[("text", json!(text))]))?;
        let skipped = r["skipped"].as_i64().unwrap_or(0);
        if skipped > 0 {
            bail!("{skipped} characters of '{text}' could not be typed: is a text field focused?");
        }
        Ok(())
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        self.call(req("erase", &[("count", json!(chars))])).map(|_| ())
    }

    fn mark(&mut self) -> Result<()> {
        self.call(req("mark", &[])).map(|_| ())
    }

    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let settle = json!({
            "op": "settle",
            "quiet": SETTLE_QUIET_MS,
            "nochange": no_change.as_millis() as u64,
            "timeout": timeout.as_millis() as u64,
        });
        let r = match self.pending.take() {
            Some(mut action) => {
                action["then"] = settle;
                self.call_now(action)?
            }
            None => self.call(settle)?,
        };
        Ok(Settled { changed: r["changed"] == true, timed_out: r["status"] == 2 })
    }

    fn launch_app(&mut self, app: &str, o: &LaunchOptions) -> Result<()> {
        if !plain_name(app) {
            bail!("unexpected app id {app}");
        }
        let activity = self.launcher_activity(app)?;
        // One device shell for stopping (or clearing), granting and starting: each separate
        // command would cost a round trip and a process start.
        let mut script = String::new();
        if o.clear_state {
            script.push_str(&format!("pm clear {app} >/dev/null || {{ echo 'pm clear {app} failed'; exit 1; }}; "));
        } else if o.stop_app {
            script.push_str(&format!("pidof {app} >/dev/null && am force-stop {app}; "));
        }
        if o.permissions.is_empty() {
            // As in Maestro, an app is launched with every permission unless the flow says otherwise.
            script.push_str(&format!(
                "for p in $(dumpsys package {app} | grep -E '{PERMISSION_LINES}' | sed -n '/runtime permissions:/,/User [0-9]/p' | grep 'granted=false' | sed 's/^ *\\([^:]*\\):.*/\\1/'); do pm grant {app} $p 2>/dev/null; done; "
            ));
        }
        let mut start: Vec<String> =
            ["am", "start", "-W", "-n", &activity, "-a", "android.intent.action.MAIN", "-c", "android.intent.category.LAUNCHER", "-f", "0x10200000"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        for (k, v) in &o.arguments {
            let flag = match v.as_str() {
                "true" | "false" => "--ez",
                v if v.parse::<i32>().is_ok() && (v == "0" || !v.starts_with('0')) => "--ei",
                _ => "--es",
            };
            start.extend([flag.to_string(), k.clone(), v.clone()]);
        }
        script.push_str(&start.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" "));
        let r = self.call(json!({"op": "exec", "argv": ["sh", "-c", script], "mark": true}))?;
        if r["code"].as_i64() != Some(0) {
            bail!("launching {app} failed ({}): {}", r["code"], r["out"].as_str().unwrap_or("").trim());
        }
        self.wait_for_app(app)?;
        self.settle(Duration::ZERO, Duration::from_secs(10))?;
        Ok(())
    }

    fn stop_app(&mut self, app: &str) -> Result<()> {
        self.mark()?;
        self.exec_ok(&["am", "force-stop", app])?;
        self.settle(Duration::from_millis(300), Duration::from_secs(5))?;
        Ok(())
    }

    fn kill_app(&mut self, app: &str) -> Result<()> {
        // `am kill` leaves a foreground app running; killing it as the system would is a stop.
        self.stop_app(app)
    }

    fn clear_state(&mut self, app: &str) -> Result<()> {
        self.exec_ok(&["pm", "clear", app]).map(|_| ())
    }

    fn open_link(&mut self, link: &str, _app: Option<&str>, browser: bool) -> Result<()> {
        let mut argv = vec!["am", "start", "-a", "android.intent.action.VIEW", "-d", link];
        if browser {
            argv.extend(["-c", "android.intent.category.BROWSABLE"]);
        }
        self.mark()?;
        self.exec_ok(&argv)?;
        Ok(())
    }

    fn hide_keyboard(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let (w, mut seq) = self.windows(None, deadline)?;
        if !has_keyboard(&w) {
            return Ok(());
        }
        self.press_key(Key::Back)?;
        loop {
            let (w, now_seq) = self.windows(seq, deadline)?;
            if !has_keyboard(&w) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("the keyboard is still shown");
            }
            seq = now_seq;
        }
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        self.flush()?;
        self.requests += 1;
        self.last_seq = None;
        let (_, png) = self.agent.client().call_with_blob(req("screenshot", &[]))?;
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
        self.exec_ok(&["settings", "put", "system", "accelerometer_rotation", "0"])?;
        self.exec_ok(&["settings", "put", "system", "user_rotation", &rot.to_string()])?;
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

    fn set_location(&mut self, lat: f64, lon: f64) -> Result<()> {
        let _ = self.exec(&["appops", "set", "com.android.shell", "android:mock_location", "allow"]);
        for provider in ["gps", "network"] {
            let _ = self.exec(&["cmd", "location", "providers", "add-test-provider", provider]);
            let _ = self.exec(&["cmd", "location", "providers", "set-test-provider-enabled", provider, "true"]);
            self.exec_ok(&["cmd", "location", "providers", "set-test-provider-location", provider, "--location", &format!("{lat},{lon}")])?;
        }
        Ok(())
    }

    fn set_airplane_mode(&mut self, on: Option<bool>) -> Result<()> {
        let on = match on {
            Some(v) => v,
            None => self.setting("global", "airplane_mode_on")? != "1",
        };
        self.exec_ok(&["cmd", "connectivity", "airplane-mode", if on { "enable" } else { "disable" }]).map(|_| ())
    }

    fn set_permissions(&mut self, app: &str, perms: &[(String, String)]) -> Result<()> {
        for (name, value) in perms {
            let list = if name.eq_ignore_ascii_case("all") {
                let dump = self.exec_ok(&["dumpsys", "package", app])?;
                runtime_permissions(&dump)
            } else {
                android_permissions(name)
            };
            if list.is_empty() && !name.eq_ignore_ascii_case("all") {
                bail!("`{name}` is not a permission virtuoso knows");
            }
            let verb = match value.to_ascii_lowercase().as_str() {
                "allow" => "grant",
                "deny" | "unset" => "revoke",
                other => bail!("permission value `{other}` is not allow, deny or unset"),
            };
            for p in list {
                // A permission the app does not request cannot be changed; that is not an error.
                let _ = self.exec(&["pm", verb, app, &p]);
            }
        }
        Ok(())
    }

    fn add_media(&mut self, files: &[&Path]) -> Result<()> {
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let ext = f.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            let video = matches!(ext.as_deref(), Some("mp4" | "mov" | "3gp" | "webm" | "mkv"));
            let remote = format!("/sdcard/{}/{name}", if video { "Movies" } else { "Pictures" });
            self.adb.run(&["push", &f.to_string_lossy(), &remote])?;
            let _ = self.exec(&["am", "broadcast", "-a", "android.intent.action.MEDIA_SCANNER_SCAN_FILE", "-d", &format!("file://{remote}")]);
        }
        Ok(())
    }

    fn drag(&mut self, from: (i32, i32), to: (i32, i32), hold_ms: u64, move_ms: u64) -> Result<()> {
        self.call(req("drag", &[("x1", json!(from.0)), ("y1", json!(from.1)), ("x2", json!(to.0)), ("y2", json!(to.1)), ("hold", json!(hold_ms)), ("ms", json!(move_ms))])).map(|_| ())
    }

    fn set_clipboard(&mut self, text: &str) -> Result<()> {
        self.call(req("clipboard", &[("text", json!(text))])).map(|_| ())
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
        std::thread::sleep(Duration::from_millis(300));
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
