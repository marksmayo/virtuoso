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

/// Chrome's `DevToolsActivePort` file: the port, then the browser's WebSocket path.
pub fn parse_active_port(text: &str) -> Option<(u16, String)> {
    let mut lines = text.lines().map(str::trim);
    let port = lines.next()?.parse().ok()?;
    let path = lines.next().filter(|p| p.starts_with('/'))?;
    Some((port, path.to_string()))
}

/// The flags for a browser the tests and flows drive: headless or not, a profile of its own,
/// a free debugging port, and none of a first run's prompts and background work.
pub fn launch_args(profile: &Path, headless: bool) -> Vec<String> {
    let mut a = Vec::new();
    if headless {
        a.push("--headless=new".to_string());
        // No GPU process: headless Chrome starts one (software rendering, nothing to show) for a
        // page's first frame, and input waits for that frame, ~360 ms on a page's first tap.
        // Drawn in software from the start, the frame and the tap come at once. (No WebGL then;
        // a window to watch, --headed, keeps the GPU.)
        a.push("--disable-gpu".to_string());
    }
    a.extend(
        [
            "--remote-debugging-port=0",
            // A desktop's window, the same for every run (headless Chrome's own is 800 x 600).
            "--window-size=1280,800",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-default-apps",
            "--disable-sync",
            // Chrome's own extensions start with it, each in a renderer, competing with the
            // first page for the CPU.
            "--disable-component-extensions-with-background-pages",
            // One switch: Chrome takes the last --disable-features it is given, not them all.
            // WebUIOmniboxPopup: headless, the address bar's popup was still preloaded in a
            // renderer of its own, never shown.
            "--disable-features=WebUIOmniboxPopup",
            // A scroll lands at once: a smooth one animates on, unseen by a settle.
            "--disable-smooth-scrolling",
            "--metrics-recording-only",
            "--password-store=basic",
            "--use-mock-keychain",
        ]
        .map(String::from),
    );
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
    profile: Option<PathBuf>,
    pub cdp: Cdp,
    /// The browser's WebSocket: another client can attach to it.
    pub ws_url: String,
}

impl Browser {
    pub fn launch(path: &Path, headless: bool) -> Result<Browser> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let profile = std::env::temp_dir().join(format!("virtuoso-chrome-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile);
        std::fs::create_dir_all(&profile)?;
        let mut child = match Command::new(path).args(launch_args(&profile, headless)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(anyhow!("could not start Chrome at {}: {e}", path.display()));
            }
        };
        let connected = connect_when_listening(&mut child, &profile);
        let (cdp, ws_url) = match connected {
            Ok(c) => c,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&profile);
                return Err(e);
            }
        };
        // From here a failure drops the browser, which stops it and removes the profile.
        Ok(Browser { child: Some(child), profile: Some(profile), cdp, ws_url })
    }

    /// `Browser.getVersion`'s product, e.g. "Chrome/157.0.8103.79": asked only when wanted (a
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
        Ok(Browser { child: None, profile: None, cdp, ws_url: ws_url.to_string() })
    }
}

/// Chrome writes its port and the browser's WebSocket path to the profile once it listens.
fn connect_when_listening(child: &mut Child, profile: &Path) -> Result<(Cdp, String)> {
    let file = profile.join("DevToolsActivePort");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some((port, path)) = std::fs::read_to_string(&file).ok().as_deref().and_then(parse_active_port) {
            let url = format!("ws://127.0.0.1:{port}{path}");
            return Ok((Cdp::new(WebSocket::connect(&url)?), url));
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
        if let Some(mut c) = self.child.take() {
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
        if let Some(p) = self.profile.take() {
            remove_profile(&p);
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
        let a = launch_args(Path::new("/tmp/p"), true);
        assert!(a.contains(&"--headless=new".to_string()));
        assert!(a.contains(&"--remote-debugging-port=0".to_string()));
        assert!(a.iter().any(|x| x == &format!("--user-data-dir={}", Path::new("/tmp/p").display())));
        for f in ["--no-first-run", "--no-default-browser-check"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
        assert!(a.last().unwrap() == "about:blank");
        assert!(!launch_args(Path::new("/tmp/p"), false).iter().any(|x| x.starts_with("--headless")));
    }

    #[test]
    fn a_headless_browser_draws_without_a_gpu_process_and_a_window_to_watch_keeps_it() {
        // Headless Chrome starts its GPU process for a page's first frame, and input waits for
        // that frame: ~360 ms on every flow's first tap. Without one it draws in software at once.
        assert!(launch_args(Path::new("/tmp/p"), true).contains(&"--disable-gpu".to_string()));
        assert!(!launch_args(Path::new("/tmp/p"), false).contains(&"--disable-gpu".to_string()));
    }
}
