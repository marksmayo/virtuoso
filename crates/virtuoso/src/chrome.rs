//! Chrome over its DevTools Protocol: finding and launching the browser (headless, a throwaway
//! profile), or attaching to one already running, and the protocol's request/reply and events
//! on the WebSocket.

use crate::websocket::WebSocket;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// What carries the protocol's messages: the WebSocket, or a test's stand-in.
pub trait Transport {
    fn send_text(&mut self, text: &str) -> Result<()>;
    /// Several messages, in order: one write where the transport can.
    fn send_texts(&mut self, texts: &[&str]) -> Result<()> {
        texts.iter().try_for_each(|t| self.send_text(t))
    }
    /// The next message, or `None` when none came within `timeout`.
    fn recv_text(&mut self, timeout: Duration) -> Result<Option<String>>;
}

impl Transport for WebSocket {
    fn send_text(&mut self, text: &str) -> Result<()> {
        WebSocket::send_text(self, text)
    }

    fn send_texts(&mut self, texts: &[&str]) -> Result<()> {
        WebSocket::send_texts(self, texts)
    }

    fn recv_text(&mut self, timeout: Duration) -> Result<Option<String>> {
        self.recv_text_within(timeout)
    }
}

/// What a call came back with: its reply, or the event that ended the wait.
pub enum Answer {
    Reply(Value),
    Stopped(Value),
}

/// The protocol on a transport: each call waits for its own reply; the events that come
/// meanwhile are kept, in order, for `next_event`.
pub struct Cdp<T: Transport = WebSocket> {
    transport: T,
    next_id: u64,
    events: VecDeque<Value>,
    /// How long a call waits for its reply.
    pub call_timeout: Duration,
    /// How many times a call (or a batch of them) has waited on Chrome: what costs a round trip.
    pub waits: u64,
}

/// A call as it goes to Chrome.
#[derive(serde::Serialize)]
struct Request<'a> {
    id: u64,
    method: &'a str,
    params: &'a Value,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a str>,
}

impl<T: Transport> Cdp<T> {
    pub fn new(transport: T) -> Cdp<T> {
        Cdp { transport, next_id: 1, events: VecDeque::new(), call_timeout: Duration::from_secs(30), waits: 0 }
    }

    /// `method` with `params`, on the browser or on a page's session; its result.
    pub fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        match self.call_until(session, method, params, &|_| false)? {
            Answer::Reply(v) | Answer::Stopped(v) => Ok(v),
        }
    }

    /// The next event already read, without waiting for another.
    pub fn queued_event(&mut self) -> Option<Value> {
        self.events.pop_front()
    }

    /// As `call`, but an event `stop` takes ends the wait for the reply (a page that opened a
    /// dialog answers nothing until it closes): the event, kept from the queue.
    pub fn call_until(&mut self, session: Option<&str>, method: &str, params: Value, stop: &dyn Fn(&Value) -> bool) -> Result<Answer> {
        self.calls(session, &[(method, params)], stop)
    }

    /// A call sent with no wait for its reply, dropped when it comes (a later call's wait takes
    /// it in passing): a gesture's steps at their own times. An error it answers goes unseen.
    pub fn post(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<()> {
        let id = self.next_id;
        self.next_id += 1;
        let m = Request { id, method, params: &params, session_id: session };
        self.transport.send_text(&serde_json::to_string(&m)?)
    }

    /// Several calls sent at once, then their replies awaited: Chrome takes a session's calls
    /// in order, so a sequence (a touch's start, move and end) costs one wait, not one each.
    /// The last call's result; an error from any is the answer. `stop` as in `call_until`.
    pub fn calls(&mut self, session: Option<&str>, calls: &[(&str, Value)], stop: &dyn Fn(&Value) -> bool) -> Result<Answer> {
        self.waits += 1;
        let mut waiting: Vec<(u64, &str)> = Vec::with_capacity(calls.len());
        let mut texts = Vec::with_capacity(calls.len());
        for (method, params) in calls {
            let id = self.next_id;
            self.next_id += 1;
            // Written from borrows: the params (a screen read's is the whole script) not copied.
            let m = Request { id, method, params, session_id: session };
            texts.push(serde_json::to_string(&m)?);
            waiting.push((id, method));
        }
        // Sent together: one write for the batch.
        self.transport.send_texts(&texts.iter().map(String::as_str).collect::<Vec<_>>())?;
        let last = waiting.last().map(|(id, _)| *id);
        let mut result = Value::Null;
        let deadline = Instant::now() + self.call_timeout;
        while !waiting.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(text) = self.transport.recv_text(left)? else {
                if Instant::now() >= deadline {
                    bail!("Chrome did not answer {} within {:?}", waiting[0].1, self.call_timeout);
                }
                continue;
            };
            let mut v: Value = serde_json::from_str(&text).with_context(|| format!("Chrome sent something that is not JSON: {text:.200}"))?;
            match v.get("id").and_then(Value::as_u64) {
                Some(n) => {
                    // A reply to a call that gave up waiting: dropped.
                    let Some(at) = waiting.iter().position(|(id, _)| *id == n) else { continue };
                    let (_, method) = waiting.remove(at);
                    if let Some(e) = v.get("error") {
                        bail!("{method}: {}", e.get("message").and_then(Value::as_str).unwrap_or("error"));
                    }
                    if Some(n) == last {
                        // Moved out, not copied: a screen read's result is the whole page.
                        result = v.as_object_mut().and_then(|o| o.remove("result")).unwrap_or(Value::Null);
                    }
                }
                None if stop(&v) => return Ok(Answer::Stopped(v)),
                None => {
                    // The session the calls went to gone (a tap that closed its own tab): what
                    // is still unanswered never will be. Kept, for the page to follow.
                    let gone = session.is_some() && v["method"] == "Target.detachedFromTarget" && v["params"]["sessionId"].as_str() == session;
                    self.events.push_back(v);
                    if gone {
                        return Ok(Answer::Reply(Value::Null));
                    }
                }
            }
        }
        Ok(Answer::Reply(result))
    }

    /// The next event (a message with a method and no id), waiting up to `timeout`.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<Value>> {
        if let Some(e) = self.events.pop_front() {
            return Ok(Some(e));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(text) = self.transport.recv_text(left)? else { return Ok(None) };
            let v: Value = serde_json::from_str(&text)?;
            if v.get("id").is_none() {
                return Ok(Some(v));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }
}

/// Where Chrome is usually installed, in the order to try: `env` looks a variable up.
pub fn chrome_candidates(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    match os {
        "windows" => ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(|k| env(k))
            // Windows separators whichever OS builds them: a Linux path joins with '/'.
            .map(|base| PathBuf::from(format!(r"{}\Google\Chrome\Application\chrome.exe", base.trim_end_matches(['\\', '/']))))
            .collect(),
        "macos" => [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .iter()
        .map(PathBuf::from)
        .collect(),
        _ => ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]
            .iter()
            .flat_map(|n| [Path::new("/usr/bin").join(n), Path::new("/usr/local/bin").join(n), Path::new("/snap/bin").join(n)])
            .collect(),
    }
}

/// The first candidate that exists, or `VIRTUOSO_CHROME`.
pub fn find_chrome() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("VIRTUOSO_CHROME") {
        return Ok(PathBuf::from(p));
    }
    let env = |k: &str| std::env::var(k).ok();
    chrome_candidates(std::env::consts::OS, &env)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("Chrome is not installed where virtuoso looks for it: set VIRTUOSO_CHROME to its path"))
}

/// The browser a launch of `chrome` runs. Headless, Chrome's headless shell when there is one of
/// its own version (Chrome's old headless mode, a binary of its own: it starts sooner, in less
/// memory, with no desktop browser's parts): the one `setting` (VIRTUOSO_CHROME_SHELL) names, or
/// else the one installed beside it. `setting` "off" (or "0", "false") keeps Chrome; a window
/// to watch (`--headed`) is always Chrome's.
///
/// Beside it means where the version is in the path: puppeteer's installs (`npx
/// @puppeteer/browsers install chrome@V chrome-headless-shell@V`, and its own cache), each in a
/// folder of its platform and version, and Playwright's, of its revision. Nothing is run to ask
/// a binary its version (that would cost a launch on every launch), so a shell found anywhere
/// else, whose version is not known to be Chrome's, is not taken; a named one is the namer's.
pub fn browser_for(chrome: &Path, headless: bool, setting: Option<&str>) -> PathBuf {
    if !headless {
        return chrome.to_path_buf();
    }
    match setting.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) if ["off", "0", "false"].iter().any(|o| s.eq_ignore_ascii_case(o)) => chrome.to_path_buf(),
        Some(s) => PathBuf::from(s),
        None => shell_beside(chrome).unwrap_or_else(|| chrome.to_path_buf()),
    }
}

/// The headless shell of `chrome`'s version installed beside it (see `browser_for`).
fn shell_beside(chrome: &Path) -> Option<PathBuf> {
    // Chrome's own folder, `chrome-<platform>` (above the app's bundle on macOS), in its
    // version's: `<cache>/chrome/<platform>-<version>/` (puppeteer) or `chromium-<revision>/`.
    let own = chrome.ancestors().skip(1).find(|a| a.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("chrome-")))?;
    let platform = own.file_name()?.to_str()?.strip_prefix("chrome-")?;
    let version = own.parent()?;
    let version_name = version.file_name()?.to_str()?;
    let above = version.parent()?;
    let shells = if above.file_name().is_some_and(|n| n == "chrome") {
        above.parent()?.join("chrome-headless-shell").join(version_name)
    } else {
        above.join(format!("chromium_headless_shell-{}", version_name.strip_prefix("chromium-")?))
    };
    let shell = shells.join(format!("chrome-headless-shell-{platform}")).join(format!("chrome-headless-shell{}", std::env::consts::EXE_SUFFIX));
    shell.is_file().then_some(shell)
}

/// Whether `path` is Chrome's headless shell (Chrome for Testing's `chrome-headless-shell`, or
/// `headless_shell` as Chromium builds it), by its name.
pub fn is_headless_shell(path: &Path) -> bool {
    path.file_stem().and_then(|n| n.to_str()).is_some_and(|n| n == "chrome-headless-shell" || n == "headless_shell")
}

/// `launch_args` for the browser at `binary`: the headless shell is headless, the old way, with
/// no mode to choose (`--headless=new` names one of Chrome's); the rest it takes as Chrome does.
pub fn launch_args_for(binary: &Path, profile: &Path, headless: bool, window: (i32, i32)) -> Vec<String> {
    let mut a = launch_args(profile, headless, window);
    if is_headless_shell(binary) {
        a.retain(|x| !x.starts_with("--headless"));
    }
    a
}

/// Chrome's `DevToolsActivePort` file: the port, then the browser's WebSocket path.
pub fn parse_active_port(text: &str) -> Option<(u16, String)> {
    let mut lines = text.lines().map(str::trim);
    let port = lines.next()?.parse().ok()?;
    let path = lines.next().filter(|p| p.starts_with('/'))?;
    Some((port, path.to_string()))
}

/// A desktop's window, the same for every run (headless Chrome's own is 800 x 600).
pub const DESKTOP_WINDOW: (i32, i32) = (1280, 800);

/// The flags for a browser the tests and flows drive: headless or not, a `window` of the size
/// given, a profile of its own, a free debugging port, and none of a first run's prompts and
/// background work.
pub fn launch_args(profile: &Path, headless: bool, window: (i32, i32)) -> Vec<String> {
    let mut a = Vec::new();
    if headless {
        a.push("--headless=new".to_string());
        // No GPU process: headless Chrome starts one (software rendering, nothing to show) for a
        // page's first frame, and input waits for that frame, ~360 ms on a page's first tap.
        // Drawn in software from the start, the frame and the tap come at once. (No WebGL then;
        // a window to watch, --headed, keeps the GPU.)
        a.push("--disable-gpu".to_string());
        // Nothing is shown, so no frame waits for a screen's next refresh (60 a second, 17 ms
        // each): one is drawn as soon as it is asked for. A scroll's step waited on three; a
        // window to watch keeps its screen's rate. Not on Windows: there Chrome, drawing as
        // fast as it could while it started, was about a quarter of a second later to begin
        // its first navigation (GitHub's runners: 8 of 8 paired runs), on every flow.
        #[cfg(not(windows))]
        a.push("--disable-frame-rate-limit".to_string());
    }
    a.push(format!("--window-size={},{}", window.0, window.1));
    a.extend(
        [
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-default-apps",
            "--disable-sync",
            // Chrome's own extensions start with it, each in a renderer, competing with the
            // first page for the CPU.
            "--disable-component-extensions-with-background-pages",
            // A desktop browser's background work, none of it a flow's: field trials, crash
            // reports, phishing checks, extensions, the hang monitor, services, the search engine
            // prompt, sound. (The features list below turns off translation, media routing,
            // optimization hints, Lens and certificate transparency updates likewise.)
            "--disable-field-trial-config",
            "--disable-breakpad",
            "--disable-client-side-phishing-detection",
            "--disable-extensions",
            "--disable-hang-monitor",
            "--no-service-autorun",
            "--disable-search-engine-choice-screen",
            "--mute-audio",
            // Drawn in software, yet Chrome kept GPU and shader caches on disk: files written as
            // the first page loads, and removed again at the end.
            "--disable-gpu-shader-disk-cache",
            // A scroll lands at once: a smooth one animates on, unseen by a settle.
            "--disable-smooth-scrolling",
            "--metrics-recording-only",
            "--password-store=basic",
            "--use-mock-keychain",
        ]
        .map(String::from),
    );
    // One switch: Chrome takes the last --disable-features it is given, not them all.
    // WebUIOmniboxPopup: headless, the address bar's popup was still preloaded in a renderer of
    // its own, never shown. GpuPersistentCache, SkiaGraphiteUsePersistentCache: the last of the
    // GPU caches on disk (see --disable-gpu-shader-disk-cache). PaintHolding: a page's first
    // paint held back after a navigation. CalculateNativeWinOcclusion (Windows): a window judged
    // covered by others (a headless one, at random) has its timers throttled. Prerender2,
    // ShoppingList, SegmentationPlatform, HistoryEmbeddings, AutofillServerCommunication: services
    // that live in the browser process (a tool's peak memory on macOS), none of them a flow's.
    // PartitionAllocBackupRefPtr: a use-after-free guard, a count beside each allocation, a
    // hardening a throwaway test browser can do without.
    let occlusion = if cfg!(windows) { ",CalculateNativeWinOcclusion" } else { "" };
    a.push(format!("--disable-features=WebUIOmniboxPopup,GpuPersistentCache,SkiaGraphiteUsePersistentCache,Translate,MediaRouter,DialMediaRouteProvider,GlobalMediaControls,OptimizationHints,LensOverlay,CertificateTransparencyComponentUpdater,DestroyProfileOnBrowserClose,PaintHolding,Prerender2,ShoppingList,SegmentationPlatform,HistoryEmbeddings,AutofillServerCommunication,PartitionAllocBackupRefPtr{occlusion}"));
    // The network in the browser process: one process less to start and to hop through. Not on
    // macOS: there the browser process is the largest, and the network in it made it ~5% larger
    // (a tool's peak memory is its largest process's there). Not on Windows: there a busy
    // browser process held a page's load up for seconds (a runner's Launch app of 5-40 s).
    #[cfg(target_os = "linux")]
    a.push("--enable-features=NetworkServiceInProcess2".to_string());
    a.push(format!("--user-data-dir={}", profile.display()));
    a.push("about:blank".to_string());
    a
}

/// How often a launch looks for Chrome's port file, and a stop for its exit: each wait ends up
/// to a poll late, on every run (a file read or a process check costs next to nothing).
const CHROME_POLL: Duration = Duration::from_millis(1);

/// How soon a profile Chrome's last processes still hold is tried again (see `remove_profile`).
const PROFILE_RETRY: Duration = Duration::from_millis(10);

/// A browser being driven: launched by virtuoso (stopped, and its profile removed, when this
/// is dropped) or attached to.
pub struct Browser {
    child: Option<Child>,
    /// Chrome and every process it starts (Windows): ended together at the stop.
    #[cfg(windows)]
    tree: Option<tree::Tree>,
    profile: Option<PathBuf>,
    /// The binary launched: Chrome, or its headless shell (none for one attached to).
    binary: Option<PathBuf>,
    /// How the launch went (none for a browser attached to, or once closed).
    launch: Option<crate::phases::Launch>,
    pub cdp: Cdp,
    /// The browser's WebSocket: another client can attach to it.
    pub ws_url: String,
}

impl Browser {
    /// A Chrome of its own, in a desktop's window.
    pub fn launch(path: &Path, headless: bool) -> Result<Browser> {
        Browser::launch_sized(path, headless, DESKTOP_WINDOW)
    }

    /// A Chrome of its own (or, headless, its headless shell: see `browser_for`), in a window
    /// of the size given.
    pub fn launch_sized(path: &Path, headless: bool, window: (i32, i32)) -> Result<Browser> {
        let binary = browser_for(path, headless, std::env::var("VIRTUOSO_CHROME_SHELL").ok().as_deref());
        Browser::launch_binary(&binary, headless, window)
    }

    fn launch_binary(path: &Path, headless: bool, window: (i32, i32)) -> Result<Browser> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let began = Instant::now();
        let root = profiles_root(&std::env::temp_dir());
        // Already there but for a machine's first launch: a look, not a write.
        let _ = std::fs::create_dir_all(&root);
        let (profile, profile_tries) = fresh_profile(&root, std::process::id(), &N)?;
        let mut child = match Command::new(path).args(launch_args_for(path, &profile, headless, window)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn() {
            Ok(c) => c,
            Err(e) => {
                closed(&profile);
                let _ = std::fs::remove_dir_all(&profile);
                return Err(anyhow!("could not start Chrome at {}: {e}", path.display()));
            }
        };
        let spawned = Instant::now();
        let stderr_line = watch_stderr(&mut child);
        // In a job of its own before it starts its other processes (they join it as it does).
        #[cfg(windows)]
        let tree = tree::Tree::of(&child);
        let connected = connect_when_listening(&mut child, &profile);
        let (cdp, ws_url, listening) = match connected {
            Ok(c) => c,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                closed(&profile);
                let _ = std::fs::remove_dir_all(&profile);
                return Err(e);
            }
        };
        let launch = crate::phases::Launch { began, spawned, listening, connected: Instant::now(), profile_tries };
        *STARTUP_DETAIL.lock().unwrap() = Some(startup_detail(spawned, listening, stderr_line.get().copied()));
        // From here a failure drops the browser, which stops it and removes the profile.
        Ok(Browser {
            #[cfg(windows)]
            tree,
            child: Some(child),
            profile: Some(profile),
            binary: Some(path.to_path_buf()),
            launch: Some(launch),
            cdp,
            ws_url,
        })
    }

    /// The binary virtuoso launched (Chrome, or its headless shell; none for one attached to).
    pub fn binary(&self) -> Option<&Path> {
        self.binary.as_deref()
    }

    /// Stops a browser virtuoso launched (as a drop does): how its launch and its stop went.
    /// `None` for one attached to, or already closed.
    pub fn close(&mut self) -> Option<crate::phases::Phases> {
        let launch = self.launch.take()?;
        let close = self.stop();
        Some(crate::phases::Phases { launch, close })
    }

    /// `Browser.getVersion`'s product, e.g. "Chrome/158.0.8127.88": asked only when wanted (a
    /// flow has no use for it, and it is a wait on Chrome).
    pub fn product(&mut self) -> Result<String> {
        Ok(self.cdp.call(None, "Browser.getVersion", json!({}))?["product"].as_str().unwrap_or_default().to_string())
    }

    /// The throwaway profile of a browser virtuoso launched (none for one attached to).
    pub fn profile(&self) -> Option<&Path> {
        self.profile.as_deref()
    }

    pub fn attach(ws_url: &str) -> Result<Browser> {
        let cdp = Cdp::new(WebSocket::connect(ws_url)?);
        Ok(Browser {
            #[cfg(windows)]
            tree: None,
            child: None,
            profile: None,
            binary: None,
            launch: None,
            cdp,
            ws_url: ws_url.to_string(),
        })
    }
}

/// Measuring only: the launch's detail, from the spawn: Chrome's "DevTools listening on" line
/// on stderr, the port file seen, and the WebSocket's TCP connect, upgrade sent and 101 read.
pub static STARTUP_DETAIL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Measuring only: when Chrome printed "DevTools listening on" (its stderr drained for ever).
pub fn watch_stderr(child: &mut Child) -> std::sync::Arc<std::sync::OnceLock<Instant>> {
    let seen = std::sync::Arc::new(std::sync::OnceLock::new());
    if let Some(err) = child.stderr.take() {
        let s = seen.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut r = std::io::BufReader::new(err);
            let mut line = Vec::new();
            while r.read_until(b'\n', &mut line).map(|n| n > 0).unwrap_or(false) {
                if line.starts_with(b"DevTools listening on") {
                    let _ = s.set(Instant::now());
                }
                line.clear();
            }
        });
    }
    seen
}

/// Measuring only: the `startup:` line, each from the spawn.
pub fn startup_detail(spawned: Instant, listening: Instant, stderr_line: Option<Instant>) -> String {
    let ms = |t: Instant| format!("{:.1} ms", t.saturating_duration_since(spawned).as_secs_f64() * 1000.0);
    let mut s = String::from("startup: ");
    s += &format!("stderr line {}, ", stderr_line.map_or("none".to_string(), ms));
    s += &format!("port file {}", ms(listening));
    if let Some([tcp, sent, upgraded]) = *crate::websocket::LAST_CONNECT.lock().unwrap() {
        s += &format!(", tcp {}, upgrade sent {}, 101 {}", ms(tcp), ms(sent), ms(upgraded));
    }
    s
}

/// Chrome writes its port and the browser's WebSocket path to the profile once it listens:
/// connected to then, with when the port file was seen.
fn connect_when_listening(child: &mut Child, profile: &Path) -> Result<(Cdp, String, Instant)> {
    let file = profile.join("DevToolsActivePort");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some((port, path)) = std::fs::read_to_string(&file).ok().as_deref().and_then(parse_active_port) {
            let listening = Instant::now();
            let url = format!("ws://127.0.0.1:{port}{path}");
            return Ok((Cdp::new(WebSocket::connect(&url)?), url, listening));
        }
        if let Ok(Some(status)) = child.try_wait() {
            bail!("Chrome exited before it listened ({status})");
        }
        if Instant::now() > deadline {
            bail!("Chrome did not open its debugging port within 30 s");
        }
        std::thread::sleep(CHROME_POLL);
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Browser {
    /// Stops a launched browser and removes its profile (once: a second stop does nothing):
    /// how long each took.
    fn stop(&mut self) -> crate::phases::Close {
        let began = Instant::now();
        if let Some(mut c) = self.child.take() {
            // Windows: the profile is thrown away, nothing in it to save, so every process is
            // ended at once (no graceful close to wait for), and not waited for: ended, they
            // cannot come back, and Windows tears them down (tens of ms) without the stop
            // watching. Its files are let go a while after (removing it took longer than the
            // rest of the stop): left for the sweep after the run (see `sweep_after_exit`).
            #[cfg(windows)]
            if self.tree.take().is_some_and(|t| t.end()) {
                if let Some(p) = self.profile.take() {
                    closed(&p);
                }
                return crate::phases::Close { chrome_exit: began.elapsed(), profile_removed: None };
            }
            // Connected (a Browser is made once its WebSocket is): asked to close, then waited for.
            self.cdp.call_timeout = Duration::from_secs(2);
            let _ = self.cdp.call(None, "Browser.close", json!({}));
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end && matches!(c.try_wait(), Ok(None)) {
                std::thread::sleep(CHROME_POLL);
            }
            let _ = c.kill();
            let _ = c.wait();
        }
        let chrome_exit = began.elapsed();
        let profile_removed = self.profile.take().map(|p| {
            let at = Instant::now();
            closed(&p);
            remove_profile(&p);
            at.elapsed()
        });
        crate::phases::Close { chrome_exit, profile_removed }
    }
}

/// A new, empty folder in `root` for a launch's profile, `virtuoso-chrome-<pid>-<n>` (and how
/// many names it took), open
/// (see `open_profiles`) from before it exists: a sweep never takes it for one left behind. A
/// folder of that name a run before left (its process id handed out again) is never removed
/// here (a Chrome still ending can hold its files, and the removal block on them): it is left
/// for the sweep, and the next `n` taken, however many there are. A folder that cannot be made
/// for another reason (one being removed is not yet gone) is passed over too, a few times.
fn fresh_profile(root: &Path, pid: u32, n: &std::sync::atomic::AtomicU32) -> Result<(PathBuf, u32)> {
    let (mut tries, mut failed) = (0, 0);
    loop {
        tries += 1;
        let profile = root.join(format!("virtuoso-chrome-{pid}-{}", n.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        open_profiles().push(profile.clone());
        match std::fs::create_dir(&profile) {
            Ok(()) => return Ok((profile, tries)),
            Err(e) => {
                closed(&profile);
                failed += u32::from(e.kind() != std::io::ErrorKind::AlreadyExists);
                if failed >= 20 {
                    return Err(anyhow!("could not make a profile folder {}: {e}", profile.display()));
                }
            }
        }
    }
}

/// The profiles of this process's browsers still open.
fn open_profiles() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    static OPEN: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

fn closed(p: &Path) {
    open_profiles().retain(|o| o != p);
}

/// The last sweep's summary (`Sweep::summary`), in the profiles' folder, for whoever wants it (a
/// run does not read it: the file the last sweeper just wrote is scanned as it is opened).
#[cfg_attr(not(windows), allow(dead_code))]
const LAST_SWEEP: &str = "last-sweep.txt";

/// The argument that makes a virtuoso the sweeper after another: `virtuoso __sweep-profiles <pid>`
/// (see `sweep_after_exit`).
pub const SWEEPER_ARG: &str = "__sweep-profiles";

/// The virtuoso a sweeper is to sweep after, from its arguments (the program first).
pub fn sweeper_for(args: &[String]) -> Option<u32> {
    match args {
        [_, a, pid] if a == SWEEPER_ARG => pid.parse().ok(),
        _ => None,
    }
}

/// What a sweeper does: waits for virtuoso `parent` to exit (`wait_for_exit`), then removes the
/// profiles stopped browsers left in `temp`'s profiles' folder (and old-style ones in `temp`)
/// once, and leaves its summary there for the next run to tell. Profiles still held (an ended
/// Chrome's files can be held for minutes) are not waited for: the next run's sweeper tries them.
#[cfg_attr(not(windows), allow(dead_code))]
fn sweep_after(parent: u32, temp: &Path, me: u32, running: &dyn Fn(u32) -> bool, wait_for_exit: &dyn Fn(u32)) {
    wait_for_exit(parent);
    let root = profiles_root(temp);
    let stats = std::sync::Mutex::new(crate::phases::Sweep::default());
    sweep_all(&root, temp, me, running, 0, Duration::ZERO, &stats);
    let s = *stats.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::write(root.join(LAST_SWEEP), s.summary(parent));
}

/// Whether a sweep has anything to do, by one look at the profiles' folder `root`: a profile in
/// it that virtuoso `me` (this process) has not open, or old-style ones not yet swept (see
/// `LEGACY_SWEPT`). No folder yet: no launch made one, nothing to sweep.
#[cfg_attr(not(windows), allow(dead_code))]
fn needs_sweep(root: &Path, me: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else { return false };
    if !root.join(LEGACY_SWEPT).exists() {
        return true;
    }
    let open = open_profiles().clone();
    entries.flatten().any(|e| profile_pid(&e.file_name().to_string_lossy()).is_some_and(|pid| pid != me || !open.contains(&e.path())))
}

/// The virtuoso process a profile folder's name (`virtuoso-chrome-<pid>-<n>`) is of.
fn profile_pid(name: &str) -> Option<u32> {
    name.strip_prefix("virtuoso-chrome-")?.split('-').next()?.parse().ok()
}

/// At virtuoso's end (Windows): the profiles its browsers' stops left, and any others left,
/// handed to a sweeper: a virtuoso of its own, detached, that waits for this one to exit and
/// then removes them (`run_sweeper`). Removing them in a thread beside
/// Chrome as it started raced Chrome's start for the disk (Chrome listened seconds late), and
/// held the run's own exit up to half a minute on profiles still held; after the run, nothing of
/// the sweep competes with a flow, and nothing waits for it.
#[cfg(windows)]
pub fn sweep_after_exit() {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
    if !needs_sweep(&profiles_root(&std::env::temp_dir()), std::process::id()) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    // The run's output is not the sweeper's to hold open: a pipe a caller reads to its end
    // (CI's step) would wait for the sweeper too.
    no_inherited_std_handles();
    let spawn = |flags: u32| {
        Command::new(&exe).args([SWEEPER_ARG, &std::process::id().to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(flags).spawn()
    };
    let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    // Out of a job the run is in (one that ends its processes with it), where the job lets it.
    if spawn(flags | CREATE_BREAKAWAY_FROM_JOB).is_err() {
        let _ = spawn(flags);
    }
}

/// This process's standard handles kept from the processes it starts from now on.
#[cfg(windows)]
fn no_inherited_std_handles() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: a handle of this process's, its inherit flag cleared (an invalid one fails).
        unsafe { SetHandleInformation(GetStdHandle(which), HANDLE_FLAG_INHERIT, 0) };
    }
}

/// The sweeper (`virtuoso __sweep-profiles <parent>`): see `sweep_after_exit`. At normal
/// priority: at background priority (THREAD_MODE_BACKGROUND_BEGIN, or the idle priority class)
/// it held up the next run's own file work in the profiles' folder, 0.7 to 38 s in a run of 25
/// back to back (it held the folder while starved, the next run waiting on it). Its one pass
/// takes tens of ms.
#[cfg(windows)]
pub fn run_sweeper(parent: u32) {
    let temp = std::env::temp_dir();
    sweep_after(parent, &temp, std::process::id(), &running, &wait_for_exit);
}

/// Waits (up to a minute) for process `pid` to exit; at once if it has.
#[cfg(windows)]
fn wait_for_exit(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: a handle opened here and closed here.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 60_000);
            CloseHandle(h);
        }
    }
}

/// Where launches make their profiles, in the temp folder `temp`: a folder of virtuoso's own on
/// Windows, so the sweep reads it alone, not the whole temp folder (thousands of entries on a
/// runner). Elsewhere `temp` itself: a stop removes its own profile, and there is no sweep.
fn profiles_root(temp: &Path) -> PathBuf {
    #[cfg(windows)]
    return temp.join("virtuoso");
    #[cfg(not(windows))]
    temp.to_path_buf()
}

/// Left in the profiles' folder once no old-style profile (made by an older virtuoso in the temp
/// folder itself) is left there: the temp folder is not walked again.
const LEGACY_SWEPT: &str = "legacy-swept";

/// The sweep of `root`, the profiles' folder, and (until none is left) of the old-style profiles
/// in `legacy`, the temp folder: as `sweep_in`, the counts of both together.
#[cfg_attr(not(windows), allow(dead_code))]
fn sweep_all(root: &Path, legacy: &Path, me: u32, running: &dyn Fn(u32) -> bool, rounds: u32, wait: Duration, stats: &std::sync::Mutex<crate::phases::Sweep>) {
    let began = Instant::now();
    sweep_in(root, me, running, rounds, wait, stats);
    let marker = root.join(LEGACY_SWEPT);
    if !marker.exists() {
        let old = std::sync::Mutex::new(crate::phases::Sweep::default());
        sweep_in(legacy, me, running, rounds, wait, &old);
        let o = *old.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
        s.found += o.found;
        s.removed += o.removed;
        s.held += o.held;
        s.retries = s.retries.max(o.retries);
        if o.found == o.removed {
            let _ = std::fs::write(&marker, "");
        }
    }
    stats.lock().unwrap_or_else(|e| e.into_inner()).took = Some(began.elapsed());
}

/// The sweep in `root`, for virtuoso process `me`: `running` tells whether another virtuoso
/// is; a profile still held is tried again up to `rounds` times, `wait` apart.
#[cfg_attr(not(windows), allow(dead_code))]
fn sweep_in(root: &Path, me: u32, running: &dyn Fn(u32) -> bool, rounds: u32, wait: Duration, stats: &std::sync::Mutex<crate::phases::Sweep>) {
    let began = Instant::now();
    let update = |f: &dyn Fn(&mut crate::phases::Sweep)| f(&mut stats.lock().unwrap_or_else(|e| e.into_inner()));
    let mut held = Vec::new();
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(pid) = profile_pid(&name) else { continue };
        update(&|s| s.found += 1);
        let path = e.path();
        let left = if pid == me { !open_profiles().contains(&path) } else { !running(pid) };
        if !left {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_err() {
            held.push(path);
            update(&|s| s.held += 1);
        } else {
            update(&|s| s.removed += 1);
        }
    }
    // One whose Chrome was stopped a moment ago (a stop does not wait for its processes to be
    // gone) can still have files held: tried again as they end (seconds, on a loaded machine).
    let mut round = 0;
    while !held.is_empty() && round < rounds {
        if round > 0 {
            std::thread::sleep(wait);
        }
        round += 1;
        let mut gone = 0;
        held.retain(|p| {
            if open_profiles().contains(p) {
                return false;
            }
            let kept = std::fs::remove_dir_all(p).is_err() && p.exists();
            gone += u32::from(!kept);
            kept
        });
        update(&|s| {
            s.retries = round;
            s.removed += gone;
        });
    }
    update(&|s| s.took = Some(began.elapsed()));
}

/// Whether process `pid` is running. One this process may not look at counts as running: a
/// profile is only removed when its virtuoso is known to be gone.
#[cfg(windows)]
fn running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: a handle opened here and closed here; code is a valid out-pointer.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        ok == 0 || code == STILL_ACTIVE as u32
    }
}

/// Chrome and every process it starts, in a Windows job: ended together, and ended with
/// virtuoso if it is itself ended before it can stop them (the job closes with its last handle).
#[cfg(windows)]
mod tree {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Tree(HANDLE);

    impl Tree {
        /// `child` in a new job, or None when it cannot be (the stop then closes it gracefully).
        pub fn of(child: &std::process::Child) -> Option<Tree> {
            // SAFETY: a new job handle, owned by the Tree; the limit struct is zeroed and sized.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let tree = Tree(job);
                let mut limit: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limit.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
                if SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limit as *const _ as *const _, size) == 0 {
                    return None;
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                    return None;
                }
                Some(tree)
            }
        }

        /// Ends every process in it (not waited for: Windows has them gone a little after):
        /// whether it could.
        pub fn end(&self) -> bool {
            // SAFETY: the job handle is valid while the Tree lives.
            unsafe { TerminateJobObject(self.0, 1) != 0 }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            // SAFETY: the Tree owns the handle; closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Removes a launched browser's profile. Chrome's own processes (renderers, utilities) end a
/// little after the browser's, and until they do they hold files in it (Windows refuses to
/// delete those): a busy machine took longer than one try, and the profile was left behind.
/// Tried again a few times, as they end.
fn remove_profile(p: &Path) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::remove_dir_all(p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound && Instant::now() < end => std::thread::sleep(PROFILE_RETRY),
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_folder_that_cannot_be_made_says_so_after_a_few_names() {
        // Nowhere to make it (the temp folder gone): not looked for for ever, and none of the
        // names tried is left counted as a browser's open profile.
        let root = std::env::temp_dir().join(format!("virtuoso-no-such-root-{}", std::process::id())).join("deeper");
        let n = std::sync::atomic::AtomicU32::new(0);
        let e = fresh_profile(&root, 78, &n).unwrap_err().to_string();
        assert!(e.contains("could not make a profile folder") && e.contains("virtuoso-chrome-78-19"), "{e}");
        assert_eq!(n.load(std::sync::atomic::Ordering::Relaxed), 20);
        assert!(!open_profiles().iter().any(|p| p.starts_with(&root)));
    }

    #[test]
    #[cfg(windows)]
    fn a_launchs_profile_is_never_a_folder_a_run_before_left_that_cannot_be_emptied() {
        // A stop does not wait for Chrome's processes to be gone, and Windows hands a process
        // id out again at once: the next virtuoso can be given the name of a profile whose
        // files a dying Chrome still holds, its old port file among them (read as this
        // launch's, it was a port nobody listens on).
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!("virtuoso-fresh-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let held = root.join("virtuoso-chrome-77-0");
        std::fs::create_dir_all(&held).unwrap();
        let port = held.join("DevToolsActivePort");
        std::fs::write(&port, "1
/old
").unwrap();
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&port).unwrap();
        let n = std::sync::atomic::AtomicU32::new(0);
        let (fresh, tries) = fresh_profile(&root, 77, &n).unwrap();
        assert_ne!(fresh, held);
        assert_eq!(tries, 2, "the held name passed over");
        assert!(fresh.is_dir() && std::fs::read_dir(&fresh).unwrap().next().is_none(), "new and empty");
        assert!(open_profiles().contains(&fresh) && !open_profiles().contains(&held), "the one in use is the one kept from a sweep");
        closed(&fresh);
        drop(lock);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_launch_never_deletes_a_folder_a_run_before_left_and_takes_the_next_name() {
        // Removing one at launch can block on files a dying Chrome still holds (seconds, on
        // Windows): it is left for the sweep, as it is, and the launch takes the next name.
        let root = std::env::temp_dir().join(format!("virtuoso-fresh-left-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let left = root.join("virtuoso-chrome-76-0");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::write(left.join("DevToolsActivePort"), "1\n/old\n").unwrap();
        let (fresh, tries) = fresh_profile(&root, 76, &std::sync::atomic::AtomicU32::new(0)).unwrap();
        let kept = left.join("DevToolsActivePort").exists();
        closed(&fresh);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!((fresh, tries), (root.join("virtuoso-chrome-76-1"), 2));
        assert!(kept, "the folder a run before left is not emptied");
    }

    #[test]
    fn a_launch_passes_over_any_number_of_folders_runs_before_left() {
        // A process id handed out again can come with many: a long run's profiles, all left
        // for the sweep. Each one there is a name passed over, not a failure.
        let root = std::env::temp_dir().join(format!("virtuoso-fresh-many-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for i in 0..30 {
            std::fs::create_dir_all(root.join(format!("virtuoso-chrome-75-{i}"))).unwrap();
        }
        let got = fresh_profile(&root, 75, &std::sync::atomic::AtomicU32::new(0));
        let _ = std::fs::remove_dir_all(&root);
        let (fresh, tries) = got.unwrap();
        closed(&fresh);
        assert_eq!((fresh, tries), (root.join("virtuoso-chrome-75-30"), 31));
    }

    #[test]
    fn a_fresh_profile_says_how_many_names_it_tried() {
        let root = std::env::temp_dir().join(format!("virtuoso-tries-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (p, tries) = fresh_profile(&root, 79, &std::sync::atomic::AtomicU32::new(0)).unwrap();
        assert_eq!(tries, 1);
        closed(&p);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_sweep_counts_the_profiles_it_found_and_removed_and_leaves_those_in_use() {
        // This process's (pid 4000000001 here) open profiles, and those of a virtuoso still
        // running, are found and left; the others removed. Other folders are not profiles.
        let root = std::env::temp_dir().join(format!("virtuoso-sweep-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let me = 4_000_000_001u32;
        let dirs = ["virtuoso-chrome-4000000001-0", "virtuoso-chrome-4000000001-1", "virtuoso-chrome-4000000002-0", "virtuoso-chrome-4000000003-0", "something-else"];
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        open_profiles().push(root.join(dirs[1]));
        let stats = std::sync::Mutex::new(crate::phases::Sweep::default());
        sweep_in(&root, me, &|pid| pid == 4_000_000_002, 3, Duration::from_millis(1), &stats);
        let s = *stats.lock().unwrap();
        closed(&root.join(dirs[1]));
        let there: Vec<bool> = dirs.iter().map(|d| root.join(d).exists()).collect();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!((s.found, s.removed, s.held, s.retries), (4, 2, 0, 0), "{s:?}");
        assert!(s.took.is_some());
        assert_eq!(there, [false, true, true, false, true]);
    }

    #[test]
    fn a_sweeper_is_a_virtuoso_told_which_virtuoso_it_sweeps_after() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(sweeper_for(&args(&["virtuoso", SWEEPER_ARG, "1234"])), Some(1234));
        assert_eq!(sweeper_for(&args(&["virtuoso", SWEEPER_ARG])), None);
        assert_eq!(sweeper_for(&args(&["virtuoso", SWEEPER_ARG, "x"])), None);
        assert_eq!(sweeper_for(&args(&["virtuoso", "test", "1234"])), None);
        assert_eq!(sweeper_for(&args(&["virtuoso", SWEEPER_ARG, "1234", "more"])), None);
    }

    #[test]
    fn a_sweeper_waits_for_its_virtuoso_to_exit_then_sweeps_and_leaves_a_summary() {
        // Nothing of the sweep runs while the run does: it waits for it to be gone first (its
        // profiles then left too), and leaves what it did for the next run to tell.
        let temp = std::env::temp_dir().join(format!("virtuoso-sweeper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let root = profiles_root(&temp);
        let parent = 4_000_000_010u32;
        let profile = root.join(format!("virtuoso-chrome-{parent}-0"));
        std::fs::create_dir_all(&profile).unwrap();
        let exited = std::cell::Cell::new(false);
        let waited = std::cell::RefCell::new(Vec::new());
        let removed_before_exit = std::cell::Cell::new(false);
        sweep_after(
            parent,
            &temp,
            4_000_000_011,
            &|pid| pid == parent && !exited.get(),
            &|pid| {
                removed_before_exit.set(!profile.exists());
                waited.borrow_mut().push(pid);
                exited.set(true);
            },
        );
        let summary = std::fs::read_to_string(root.join(LAST_SWEEP)).unwrap_or_default();
        let there = profile.exists();
        let _ = std::fs::remove_dir_all(&temp);
        assert_eq!(*waited.borrow(), [parent]);
        assert!(!removed_before_exit.get() && !there, "removed once its virtuoso was gone");
        assert!(summary.starts_with("found 1, removed 1, held 0, took ") && summary.ends_with(&format!("(after virtuoso {parent})")), "{summary}");
    }

    #[test]
    fn a_sweeper_is_started_only_when_there_is_something_to_sweep() {
        // One look at the profiles' folder: a profile this process has not open, or the old-style
        // ones in the temp folder not yet swept. Nothing else there counts.
        let temp = std::env::temp_dir().join(format!("virtuoso-needs-sweep-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let root = temp.join("virtuoso");
        let me = 4_000_000_020u32;
        let mut seen = vec![needs_sweep(&root, me)];
        std::fs::create_dir_all(&root).unwrap();
        seen.push(needs_sweep(&root, me));
        std::fs::write(root.join(LEGACY_SWEPT), "").unwrap();
        std::fs::write(root.join(LAST_SWEEP), "found 0").unwrap();
        seen.push(needs_sweep(&root, me));
        let open = root.join(format!("virtuoso-chrome-{me}-0"));
        std::fs::create_dir_all(&open).unwrap();
        open_profiles().push(open.clone());
        seen.push(needs_sweep(&root, me));
        closed(&open);
        seen.push(needs_sweep(&root, me));
        let _ = std::fs::remove_dir_all(&temp);
        // No folder yet; the old-style ones not swept; nothing; this process's open one; left.
        assert_eq!(seen, [false, true, false, false, true]);
    }

    #[test]
    fn profiles_live_in_a_folder_of_their_own_on_windows() {
        // There the sweep reads only it, not the whole temp folder (thousands of entries on a
        // runner). Elsewhere a stop removes its own profile: there is no sweep.
        let root = profiles_root(Path::new("t"));
        assert_eq!(root, if cfg!(windows) { Path::new("t").join("virtuoso") } else { PathBuf::from("t") });
    }

    #[test]
    fn a_sweep_removes_the_old_style_profiles_in_the_temp_folder_until_none_is_left() {
        // Older virtuosos made their profiles in the temp folder itself. It is walked for them,
        // with the profiles' own folder, until none is left; then never again.
        let temp = std::env::temp_dir().join(format!("virtuoso-sweep-legacy-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let root = temp.join("virtuoso");
        std::fs::create_dir_all(root.join("virtuoso-chrome-4000000005-0")).unwrap();
        for d in ["virtuoso-chrome-4000000005-1", "virtuoso-chrome-4000000006-0", "not-a-profile"] {
            std::fs::create_dir_all(temp.join(d)).unwrap();
        }
        let sweep = |running: &dyn Fn(u32) -> bool| {
            let stats = std::sync::Mutex::new(crate::phases::Sweep::default());
            sweep_all(&root, &temp, 4_000_000_001, running, 0, Duration::ZERO, &stats);
            let s = *stats.lock().unwrap();
            s
        };
        // An old-style one of a virtuoso still running is left, and the temp folder walked again.
        let first = sweep(&|pid| pid == 4_000_000_006);
        let after_first = (root.join("virtuoso-chrome-4000000005-0").exists(), temp.join("virtuoso-chrome-4000000005-1").exists(), temp.join("virtuoso-chrome-4000000006-0").exists());
        let second = sweep(&|_| false);
        let after_second = temp.join("virtuoso-chrome-4000000006-0").exists();
        // None left: an old-style one made now is not looked for.
        std::fs::create_dir_all(temp.join("virtuoso-chrome-4000000007-0")).unwrap();
        let third = sweep(&|_| false);
        let after_third = (temp.join("virtuoso-chrome-4000000007-0").exists(), temp.join("not-a-profile").exists());
        let _ = std::fs::remove_dir_all(&temp);
        assert_eq!((first.found, first.removed), (3, 2), "{first:?}");
        assert_eq!(after_first, (false, false, true));
        assert_eq!((second.found, second.removed), (1, 1), "{second:?}");
        assert!(!after_second);
        assert_eq!((third.found, third.removed), (0, 0), "{third:?}");
        assert!(third.took.is_some());
        assert_eq!(after_third, (true, true));
    }

    #[test]
    #[cfg(windows)]
    fn a_sweep_counts_a_profile_still_held_and_its_retries() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!("virtuoso-sweep-held-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let held = root.join("virtuoso-chrome-4000000004-0");
        std::fs::create_dir_all(&held).unwrap();
        let file = held.join("DevToolsActivePort");
        std::fs::write(&file, "1").unwrap();
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&file).unwrap();
        let stats = std::sync::Mutex::new(crate::phases::Sweep::default());
        sweep_in(&root, 4_000_000_001, &|_| false, 3, Duration::from_millis(1), &stats);
        let s = *stats.lock().unwrap();
        drop(lock);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!((s.found, s.removed, s.held, s.retries), (1, 0, 1, 3), "{s:?}");
        assert!(s.took.is_some());
    }

    #[test]
    #[cfg(unix)]
    fn a_sweep_counts_a_profile_it_cannot_remove_and_its_retries_on_unix() {
        // A folder whose own folder will not let go of its files (no write permission there)
        // cannot be removed: held, and tried again each round.
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("virtuoso-sweep-held-unix-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let held = root.join("virtuoso-chrome-4000000004-0");
        let locked = held.join("Default");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("Preferences"), "{}").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root removes it anyway: nothing to show then.
        let as_root = std::fs::write(locked.join("probe"), "").is_ok();
        let stats = std::sync::Mutex::new(crate::phases::Sweep::default());
        if !as_root {
            sweep_in(&root, 4_000_000_001, &|_| false, 3, Duration::from_millis(1), &stats);
        }
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&root);
        if as_root {
            return;
        }
        let s = *stats.lock().unwrap();
        assert_eq!((s.found, s.removed, s.held, s.retries), (1, 0, 1, 3), "{s:?}");
        assert!(s.took.is_some());
    }

    /// Messages to hand out, and what was sent.
    struct Fake {
        incoming: VecDeque<String>,
        sent: Vec<Value>,
        texts: Vec<String>,
    }

    impl Transport for Fake {
        fn send_text(&mut self, text: &str) -> Result<()> {
            self.sent.push(serde_json::from_str(text)?);
            self.texts.push(text.to_string());
            Ok(())
        }

        fn recv_text(&mut self, _timeout: Duration) -> Result<Option<String>> {
            Ok(self.incoming.pop_front())
        }
    }

    fn cdp(incoming: &[&str]) -> Cdp<Fake> {
        Cdp::new(Fake { incoming: incoming.iter().map(|s| s.to_string()).collect(), sent: vec![], texts: vec![] })
    }

    #[test]
    fn a_launch_and_a_stop_look_for_chrome_every_millisecond_and_a_profile_goes_on_a_10_ms_retry() {
        // Each wait ends up to a poll late, on every run; a look costs next to nothing.
        assert_eq!(CHROME_POLL, Duration::from_millis(1));
        assert_eq!(PROFILE_RETRY, Duration::from_millis(10));
    }

    #[test]
    fn a_call_sends_its_method_with_an_id_and_returns_its_result() {
        let mut c = cdp(&[r#"{"id":1,"result":{"product":"HeadlessChrome/1"}}"#]);
        let r = c.call(None, "Browser.getVersion", json!({})).unwrap();
        assert_eq!(r["product"], "HeadlessChrome/1");
        assert_eq!(c.transport.sent[0], json!({"id": 1, "method": "Browser.getVersion", "params": {}}));
    }

    #[test]
    fn a_call_goes_as_compact_json_its_id_method_params_and_session_in_that_order() {
        let mut c = cdp(&[r#"{"id":1,"result":{}}"#]);
        c.call(Some("S1"), "Runtime.evaluate", json!({"returnByValue": true, "expression": "a\"b"})).unwrap();
        assert_eq!(c.transport.texts[0], r#"{"id":1,"method":"Runtime.evaluate","params":{"expression":"a\"b","returnByValue":true},"sessionId":"S1"}"#);
    }

    #[test]
    fn a_call_on_a_page_names_its_session() {
        let mut c = cdp(&[r#"{"id":1,"sessionId":"S1","result":{}}"#]);
        c.call(Some("S1"), "Page.enable", json!({})).unwrap();
        assert_eq!(c.transport.sent[0]["sessionId"], "S1");
    }

    #[test]
    fn events_before_the_reply_are_kept_in_order_for_later() {
        let mut c = cdp(&[
            r#"{"method":"Page.loadEventFired","params":{"timestamp":1}}"#,
            r#"{"method":"Network.requestWillBeSent","params":{}}"#,
            r#"{"id":1,"result":{"ok":true}}"#,
        ]);
        assert_eq!(c.call(None, "X.y", json!({})).unwrap()["ok"], true);
        assert_eq!(c.next_event(Duration::ZERO).unwrap().unwrap()["method"], "Page.loadEventFired");
        assert_eq!(c.next_event(Duration::ZERO).unwrap().unwrap()["method"], "Network.requestWillBeSent");
        assert!(c.next_event(Duration::ZERO).unwrap().is_none());
    }

    #[test]
    fn a_posted_call_is_sent_without_a_wait_and_its_reply_is_dropped_when_it_comes() {
        // A gesture's steps go at their times, not each after the last one's answer.
        let mut c = cdp(&[r#"{"id":1,"result":{}}"#, r#"{"id":2,"result":{"n":2}}"#]);
        c.post(Some("S1"), "Input.dispatchTouchEvent", json!({"type": "touchMove"})).unwrap();
        assert_eq!(c.waits, 0, "no wait");
        assert_eq!(c.transport.sent.len(), 1);
        assert_eq!(c.transport.sent[0]["sessionId"], "S1");
        // The next call's wait takes the posted call's reply in passing.
        assert_eq!(c.call(Some("S1"), "B.b", json!({})).unwrap()["n"], 2);
    }

    #[test]
    fn calls_are_all_sent_before_any_reply_and_answered_in_their_order() {
        // Replies out of order, an event among them: each call's own result, in call order.
        let mut c = cdp(&[
            r#"{"id":2,"result":{"n":2}}"#,
            r#"{"method":"Page.frameNavigated","params":{}}"#,
            r#"{"id":1,"result":{"n":1}}"#,
            r#"{"id":3,"result":{"n":3}}"#,
        ]);
        let r = c.calls(Some("S1"), &[("A.a", json!({})), ("B.b", json!({})), ("C.c", json!({}))], &|_| false).unwrap();
        let Answer::Reply(v) = r else { panic!("stopped") };
        assert_eq!(v["n"], 3, "the last call's result");
        assert_eq!(c.transport.sent.len(), 3);
        assert_eq!(c.transport.sent.iter().map(|m| m["id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(c.next_event(Duration::ZERO).unwrap().unwrap()["method"], "Page.frameNavigated");
    }

    #[test]
    fn calls_stop_at_an_event_that_ends_the_wait() {
        let mut c = cdp(&[r#"{"id":1,"result":{}}"#, r#"{"method":"Page.javascriptDialogOpening","params":{}}"#]);
        let r = c.calls(None, &[("A.a", json!({})), ("B.b", json!({}))], &|e| e["method"] == "Page.javascriptDialogOpening").unwrap();
        assert!(matches!(r, Answer::Stopped(_)));
    }

    #[test]
    fn ids_go_up_and_a_reply_to_another_call_is_not_taken() {
        let mut c = cdp(&[r#"{"id":1,"result":{"n":1}}"#, r#"{"id":7,"result":{"stale":true}}"#, r#"{"id":2,"result":{"n":2}}"#]);
        assert_eq!(c.call(None, "A.a", json!({})).unwrap()["n"], 1);
        assert_eq!(c.call(None, "B.b", json!({})).unwrap()["n"], 2);
        assert_eq!(c.transport.sent[1]["id"], 2);
    }

    #[test]
    fn an_event_the_caller_stops_on_ends_the_wait_for_the_reply() {
        let mut c = cdp(&[r#"{"method":"Network.x","params":{}}"#, r#"{"method":"Page.javascriptDialogOpening","params":{"message":"Hi"}}"#]);
        let a = c.call_until(None, "Input.dispatchMouseEvent", json!({}), &|e| e["method"] == "Page.javascriptDialogOpening").unwrap();
        let Answer::Stopped(e) = a else { panic!("stopped by the dialog") };
        assert_eq!(e["params"]["message"], "Hi");
        assert_eq!(c.queued_event().unwrap()["method"], "Network.x", "the events before it are kept");
        assert!(c.queued_event().is_none());
    }

    #[test]
    fn an_error_reply_is_an_error_with_its_message() {
        let mut c = cdp(&[r#"{"id":1,"error":{"code":-32000,"message":"No node with given id found"}}"#]);
        let e = c.call(None, "DOM.getBoxModel", json!({"nodeId": 9})).unwrap_err();
        assert!(e.to_string().contains("No node with given id found"), "{e}");
        assert!(e.to_string().contains("DOM.getBoxModel"), "{e}");
    }

    #[test]
    fn calls_on_a_session_that_goes_away_end_without_their_replies_and_its_going_is_kept() {
        // A tap on a button that closes its own tab: the session is gone before Chrome answers
        // the release, and it never does. Not a wait to the timeout: null, the event kept for
        // the page to follow. Another session's going is only an event.
        let mut c = cdp(&[
            r#"{"method":"Target.detachedFromTarget","params":{"sessionId":"S2","targetId":"T2"}}"#,
            r#"{"id":1,"sessionId":"S1","result":{}}"#,
            r#"{"method":"Target.detachedFromTarget","params":{"sessionId":"S1","targetId":"T1"}}"#,
        ]);
        c.call_timeout = Duration::from_millis(50);
        let m = "Input.dispatchMouseEvent";
        let answer = c.calls(Some("S1"), &[(m, json!({"type": "mousePressed"})), (m, json!({"type": "mouseReleased"}))], &|_| false).unwrap();
        assert!(matches!(answer, Answer::Reply(Value::Null)));
        assert_eq!(c.queued_event().unwrap()["params"]["sessionId"], "S2");
        assert_eq!(c.queued_event().unwrap()["params"]["sessionId"], "S1");
        assert!(c.queued_event().is_none());
    }

    #[test]
    fn no_reply_within_the_call_timeout_is_an_error() {
        let mut c = cdp(&[]);
        c.call_timeout = Duration::from_millis(50);
        assert!(c.call(None, "A.a", json!({})).unwrap_err().to_string().contains("A.a"));
    }

    #[test]
    fn chrome_is_looked_for_where_each_os_installs_it() {
        let env = |k: &str| match k {
            "ProgramFiles" => Some(r"C:\Program Files".to_string()),
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_string()),
            "LOCALAPPDATA" => Some(r"C:\Users\u\AppData\Local".to_string()),
            _ => None,
        };
        let w = chrome_candidates("windows", &env);
        assert_eq!(w[0], PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"));
        assert!(w.contains(&PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe")));
        assert!(w.contains(&PathBuf::from(r"C:\Users\u\AppData\Local\Google\Chrome\Application\chrome.exe")));
        let m = chrome_candidates("macos", &env);
        assert_eq!(m[0], PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        assert!(m.contains(&PathBuf::from("/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")));
        let l = chrome_candidates("linux", &env);
        for p in ["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"] {
            assert!(l.contains(&PathBuf::from(p)), "{p}");
        }
    }

    #[test]
    fn the_active_port_file_gives_the_port_and_the_browsers_path() {
        assert_eq!(parse_active_port("53211\n/devtools/browser/0b1c\n"), Some((53211, "/devtools/browser/0b1c".to_string())));
        assert_eq!(parse_active_port("53211\r\n/devtools/browser/x"), Some((53211, "/devtools/browser/x".to_string())));
        assert_eq!(parse_active_port(""), None);
        assert_eq!(parse_active_port("53211\n"), None);
    }

    #[test]
    fn a_launch_is_headless_on_its_own_profile_and_a_free_port() {
        let a = launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW);
        assert!(a.contains(&"--headless=new".to_string()));
        assert!(a.contains(&"--remote-debugging-port=0".to_string()));
        assert!(a.iter().any(|x| x == &format!("--user-data-dir={}", Path::new("/tmp/p").display())));
        for f in ["--no-first-run", "--no-default-browser-check"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
        assert!(a.last().unwrap() == "about:blank");
        assert!(!launch_args(Path::new("/tmp/p"), false, DESKTOP_WINDOW).iter().any(|x| x.starts_with("--headless")));
    }

    #[test]
    fn a_launched_browser_does_none_of_a_desktop_browsers_background_work() {
        // A flow's browser lives for seconds: crash reports, phishing checks, translation,
        // media routing, field trials and the like are work its page competes with.
        let a = launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW);
        for f in ["--disable-field-trial-config", "--disable-breakpad", "--disable-client-side-phishing-detection", "--disable-extensions", "--disable-hang-monitor", "--no-service-autorun", "--disable-search-engine-choice-screen", "--mute-audio"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
        let off = a.iter().find_map(|x| x.strip_prefix("--disable-features=")).unwrap_or_default();
        for f in ["Translate", "MediaRouter", "DialMediaRouteProvider", "GlobalMediaControls", "OptimizationHints", "LensOverlay", "CertificateTransparencyComponentUpdater", "DestroyProfileOnBrowserClose"] {
            assert!(off.split(',').any(|x| x == f), "{f} in {off}");
        }
        assert_eq!(a.iter().filter(|x| x.starts_with("--disable-features=")).count(), 1, "Chrome takes only the last");
    }

    /// The features a launch turns off (its one --disable-features switch).
    fn features_off(a: &[String]) -> Vec<&str> {
        assert_eq!(a.iter().filter(|x| x.starts_with("--disable-features=")).count(), 1, "Chrome takes only the last");
        a.iter().find_map(|x| x.strip_prefix("--disable-features=")).unwrap_or_default().split(',').collect()
    }

    #[test]
    fn a_launched_browser_holds_no_first_paint_after_a_navigation() {
        // PaintHolding keeps a page's first paint back after a navigation (Playwright turns it
        // off too): a Launch app's and a first tap's wait.
        for headless in [true, false] {
            assert!(features_off(&launch_args(Path::new("/tmp/p"), headless, DESKTOP_WINDOW)).contains(&"PaintHolding"));
        }
    }

    #[test]
    fn a_launched_browser_on_windows_never_judges_its_window_covered() {
        // Windows: a window judged covered by others (a headless one, at random) has its timers
        // throttled (Playwright turns this off too). Not a feature elsewhere.
        for headless in [true, false] {
            assert_eq!(features_off(&launch_args(Path::new("/tmp/p"), headless, DESKTOP_WINDOW)).contains(&"CalculateNativeWinOcclusion"), cfg!(windows));
        }
    }

    #[test]
    fn a_launched_browser_runs_none_of_a_desktop_browsers_services_in_its_process() {
        // Prerendering, shopping, segmentation, history search, autofill's server: work and
        // memory in the browser process (a tool's peak on macOS), none of it a flow's.
        let a = launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW);
        let off = a.iter().find_map(|x| x.strip_prefix("--disable-features=")).unwrap_or_default();
        for f in ["Prerender2", "ShoppingList", "SegmentationPlatform", "HistoryEmbeddings", "AutofillServerCommunication"] {
            assert!(off.split(',').any(|x| x == f), "{f} in {off}");
        }
    }

    #[test]
    fn a_launch_has_a_window_of_the_size_asked_a_desktops_by_default() {
        let sizes = |w| launch_args(Path::new("/tmp/p"), true, w).into_iter().filter(|x| x.starts_with("--window-size=")).collect::<Vec<_>>();
        assert_eq!(sizes(DESKTOP_WINDOW), ["--window-size=1280,800"]);
        assert_eq!(sizes((412, 839)), ["--window-size=412,839"]);
    }

    #[test]
    fn a_launched_browser_allocates_without_backup_ref_ptrs() {
        // BackupRefPtr guards against use-after-free with a count beside each allocation: a
        // hardening a throwaway test browser can do without, and memory in every process.
        let a = launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW);
        let off = a.iter().find_map(|x| x.strip_prefix("--disable-features=")).unwrap_or_default();
        assert!(off.split(',').any(|x| x == "PartitionAllocBackupRefPtr"), "{off}");
    }

    #[test]
    fn a_headless_chrome_draws_frames_as_asked_and_a_window_at_its_screens_rate() {
        // Off Windows (where it made every launch slower): see launch_args.
        let switch = "--disable-frame-rate-limit".to_string();
        assert_eq!(launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW).contains(&switch), cfg!(not(windows)));
        assert!(!launch_args(Path::new("/tmp/p"), false, DESKTOP_WINDOW).contains(&switch));
    }

    /// A file at `root`/`parts`, made with its folders: where a browser is installed.
    fn installed(root: &Path, parts: &[&str]) -> PathBuf {
        let p = parts.iter().fold(root.to_path_buf(), |p, s| p.join(s));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"").unwrap();
        p
    }

    fn shell_exe() -> String {
        format!("chrome-headless-shell{}", std::env::consts::EXE_SUFFIX)
    }

    #[test]
    fn a_headless_launch_takes_the_headless_shell_of_chromes_own_version_from_puppeteers_cache() {
        // `npx @puppeteer/browsers install chrome@V chrome-headless-shell@V`: each in a folder
        // named for its platform and version, side by side.
        let dir = tempfile::tempdir().unwrap();
        let chrome = installed(dir.path(), &["chrome", "win64-158.0.8127.88", "chrome-win64", "chrome.exe"]);
        assert_eq!(browser_for(&chrome, true, None), chrome, "no shell installed: Chrome");
        let shell = installed(dir.path(), &["chrome-headless-shell", "win64-158.0.8127.88", "chrome-headless-shell-win64", &shell_exe()]);
        assert_eq!(browser_for(&chrome, true, None), shell);
        assert_eq!(browser_for(&chrome, false, None), chrome, "a window to watch: Chrome");
        // Another version's shell is not Chrome's.
        let other = installed(dir.path(), &["chrome", "win64-157.0.8000.1", "chrome-win64", "chrome.exe"]);
        assert_eq!(browser_for(&other, true, None), other);
    }

    #[test]
    fn a_headless_launch_finds_the_shell_beside_chrome_for_testings_app_on_macos() {
        // The app's binary is deep in its bundle; the shell is a plain binary.
        let dir = tempfile::tempdir().unwrap();
        let chrome = installed(
            dir.path(),
            &["chrome", "mac_arm-158.0.8127.88", "chrome-mac-arm64", "Google Chrome for Testing.app", "Contents", "MacOS", "Google Chrome for Testing"],
        );
        let shell = installed(dir.path(), &["chrome-headless-shell", "mac_arm-158.0.8127.88", "chrome-headless-shell-mac-arm64", &shell_exe()]);
        assert_eq!(browser_for(&chrome, true, None), shell);
    }

    #[test]
    fn a_headless_launch_takes_the_headless_shell_of_playwrights_chromium_of_the_same_revision() {
        let dir = tempfile::tempdir().unwrap();
        let chrome = installed(dir.path(), &["chromium-1255", "chrome-linux64", "chrome"]);
        let shell = installed(dir.path(), &["chromium_headless_shell-1255", "chrome-headless-shell-linux64", &shell_exe()]);
        installed(dir.path(), &["chromium_headless_shell-1254", "chrome-headless-shell-linux64", &shell_exe()]);
        assert_eq!(browser_for(&chrome, true, None), shell);
        let older = installed(dir.path(), &["chromium-1200", "chrome-linux64", "chrome"]);
        assert_eq!(browser_for(&older, true, None), older);
    }

    #[test]
    fn an_installed_chrome_has_no_shell_beside_it() {
        // Google Chrome as a desktop installs it: no folder of Chrome for Testing's to look in.
        let dir = tempfile::tempdir().unwrap();
        let chrome = installed(dir.path(), &["Google", "Chrome", "Application", "chrome.exe"]);
        installed(dir.path(), &["Google", "Chrome", "Application", &shell_exe()]);
        assert_eq!(browser_for(&chrome, true, None), chrome);
        assert_eq!(browser_for(Path::new("chrome"), true, None), PathBuf::from("chrome"));
    }

    #[test]
    fn virtuoso_chrome_shell_names_the_shell_or_turns_it_off() {
        let dir = tempfile::tempdir().unwrap();
        let chrome = installed(dir.path(), &["chrome", "linux-158.0.8127.88", "chrome-linux64", "chrome"]);
        let found = installed(dir.path(), &["chrome-headless-shell", "linux-158.0.8127.88", "chrome-headless-shell-linux64", &shell_exe()]);
        let named = PathBuf::from("/opt/shell/chrome-headless-shell");
        assert_eq!(browser_for(&chrome, true, Some("/opt/shell/chrome-headless-shell")), named);
        assert_eq!(browser_for(&chrome, false, Some("/opt/shell/chrome-headless-shell")), chrome, "--headed keeps Chrome");
        for off in ["off", "0", "OFF", "false"] {
            assert_eq!(browser_for(&chrome, true, Some(off)), chrome, "{off}");
        }
        // Set but empty (a CI matrix's blank): as if unset.
        assert_eq!(browser_for(&chrome, true, Some("")), found);
    }

    #[test]
    fn a_headless_shell_is_known_by_its_name() {
        assert!(is_headless_shell(Path::new("/x/chrome-headless-shell-linux64/chrome-headless-shell")));
        assert!(is_headless_shell(Path::new("/x/headless_shell")));
        assert!(!is_headless_shell(Path::new("/usr/bin/google-chrome")));
        // Windows paths where Windows reads them (elsewhere a backslash is no separator).
        if cfg!(windows) {
            assert!(is_headless_shell(Path::new(r"C:\x\chrome-headless-shell.exe")));
            assert!(!is_headless_shell(Path::new(r"C:\x\chrome.exe")));
        }
    }

    #[test]
    fn the_headless_shell_is_told_nothing_of_a_headless_mode_it_does_not_have() {
        // It is headless, the old way, and has no other: --headless=new names a mode of Chrome's.
        let a = launch_args_for(Path::new("/x/chrome-headless-shell"), Path::new("/tmp/p"), true, DESKTOP_WINDOW);
        assert!(!a.iter().any(|x| x.starts_with("--headless")), "{a:?}");
        for f in ["--disable-gpu", "--remote-debugging-port=0", "--window-size=1280,800", "--disable-extensions"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
        assert_eq!(launch_args_for(Path::new("/x/chrome"), Path::new("/tmp/p"), true, DESKTOP_WINDOW), launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW));
    }

    #[test]
    fn a_headless_browser_draws_without_a_gpu_process_and_a_window_to_watch_keeps_it() {
        // Headless Chrome starts its GPU process for a page's first frame, and input waits for
        // that frame: ~360 ms on every flow's first tap. Without one it draws in software at once.
        assert!(launch_args(Path::new("/tmp/p"), true, DESKTOP_WINDOW).contains(&"--disable-gpu".to_string()));
        assert!(!launch_args(Path::new("/tmp/p"), false, DESKTOP_WINDOW).contains(&"--disable-gpu".to_string()));
    }
}
