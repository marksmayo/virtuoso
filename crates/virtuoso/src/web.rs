//! The web as a device: a page in Chrome, driven over the DevTools Protocol. A flow's `url:`
//! is its app: `launchApp` opens it in a fresh tab (`clearState` first forgets what the site
//! stored), `stopApp` closes the tab.

use crate::chrome::Browser;
use crate::device::{Device, Key, LaunchOptions, Lift, Settled};
use crate::screen::{Element, Rect, ScreenTree};
use std::sync::Arc;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

pub struct WebDevice {
    browser: Browser,
    /// The tab the flow drives, once it has launched: the newest of the tabs it opened.
    page: Option<Page>,
    /// The tabs under it, oldest first: the flow comes back to the last when the page closes.
    under: Vec<Page>,
    /// A dialog the page has open (it answers nothing else until it closes).
    dialog: Option<Dialog>,
    /// The last screen read from a page: what shows under a dialog.
    last: Option<ScreenTree>,
    /// The tabs there before, and those virtuoso opened: any other a page opened.
    known: std::collections::HashSet<String>,
    /// Each other-origin frame's context for reading it, by frame id.
    worlds: std::collections::HashMap<String, i64>,
    /// The viewport a setOrientation asked for, for the tabs opened after it too.
    viewport: Option<(i32, i32)>,
    /// The phone every tab emulates (`--emulate`), if any.
    phone: Option<Phone>,
    /// Chrome on an Android device: the adb link it is reached through.
    android: Option<AndroidLink>,
    /// As the last screen read had it: where the visible part of the page is in the page, and
    /// whether a field being typed into has the focus.
    view: (f64, f64, bool),
    /// A scroll's drag brings the tracker's state and the screen with its wait for the frames:
    /// the settle and the next read take them, while nothing else has been sent to Chrome
    /// since (the round trip it was had in).
    ahead: Option<Ahead>,
    /// A recording running: frames from Chrome's screencast, made a video when it stops.
    recording: Option<Recording>,
}

/// What a scroll's drag brought back with it: as of round trip `at`.
struct Ahead {
    at: u64,
    state: Option<Value>,
    screen: Option<Value>,
}

struct Recording {
    path: std::path::PathBuf,
    frames: std::path::PathBuf,
    count: u32,
    started: Instant,
}

/// A flow's permission name as the browser's (`None`: the web has no such permission).
pub fn permission_name(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "location" | "geolocation" => "geolocation",
        "camera" => "camera",
        "microphone" | "audio" => "microphone",
        "notifications" => "notifications",
        "clipboard" => "clipboard-read",
        "midi" => "midi",
        _ => return None,
    })
}

struct Page {
    target: String,
    session: String,
}

/// A browser dialog: `alert`, `confirm`, `prompt` (or a leave-page check).
struct Dialog {
    kind: String,
    message: String,
    /// A prompt's answer as typed so far.
    typed: String,
}

impl Dialog {
    /// Its buttons, left to right, with whether each accepts it.
    fn buttons(&self) -> Vec<(&'static str, bool)> {
        if self.kind == "alert" { vec![("OK", true)] } else { vec![("Cancel", false), ("OK", true)] }
    }
}

/// Where a dialog's parts are on a `w` x `h` screen: its box, its message, its prompt field,
/// and its buttons in `buttons` order. Drawn as Chrome centres it, near enough to tap.
fn dialog_layout(w: i32, h: i32, buttons: usize) -> (Rect, Rect, Rect, Vec<Rect>) {
    let (bw, bh) = (360.min(w - 16).max(120), 180);
    let (left, top) = ((w - bw) / 2, (h - bh) / 3);
    let boxr = Rect { left, top, right: left + bw, bottom: top + bh };
    let message = Rect { left: left + 16, top: top + 16, right: left + bw - 16, bottom: top + 64 };
    let field = Rect { left: left + 16, top: top + 72, right: left + bw - 16, bottom: top + 108 };
    let buttons = (0..buttons)
        .map(|i| {
            let right = left + bw - 16 - (buttons - 1 - i) as i32 * 96;
            Rect { left: right - 88, top: top + bh - 52, right, bottom: top + bh - 16 }
        })
        .collect();
    (boxr, message, field, buttons)
}

/// How long a page has to load before `launchApp` gives up on it.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// `scheme://host[:port]` of a URL, what a site's storage is kept under.
pub fn origin(url: &str) -> Result<String> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| anyhow!("not a URL: {url}"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        bail!("not a URL: {url}");
    }
    Ok(format!("{scheme}://{host}"))
}

impl WebDevice {
    /// A Chrome of its own, headless or not.
    pub fn launch(path: &Path, headless: bool) -> Result<WebDevice> {
        WebDevice::on(Browser::launch(path, headless)?)
    }

    /// Tabs opened from now on are told of (Target.targetCreated): those already there are not
    /// a page's.
    fn on(mut browser: Browser) -> Result<WebDevice> {
        let targets = browser.cdp.call(None, "Target.getTargets", json!({}))?;
        let known = targets["targetInfos"].as_array().into_iter().flatten().filter_map(|t| t["targetId"].as_str().map(String::from)).collect();
        browser.cdp.call(None, "Target.setDiscoverTargets", json!({"discover": true}))?;
        Ok(WebDevice { browser, page: None, under: Vec::new(), dialog: None, last: None, known, worlds: Default::default(), viewport: None, phone: None, android: None, view: (0.0, 0.0, false), ahead: None, recording: None })
    }

    /// The Chrome already running at `ws_url`.
    pub fn attach(ws_url: &str) -> Result<WebDevice> {
        WebDevice::on(Browser::attach(ws_url)?)
    }

    /// A DevTools call on the page: its result.
    pub fn devtools(&mut self, method: &str, params: Value) -> Result<Value> {
        self.page_call(method, params)
    }

    /// `expression`'s value in the page (awaited when it is a promise).
    pub fn evaluate(&mut self, expression: &str) -> Result<Value> {
        if self.dialog.is_some() {
            bail!("the page has a dialog open: it runs no script until the dialog is answered");
        }
        let r = self.page_call("Runtime.evaluate", json!({"expression": expression, "returnByValue": true, "awaitPromise": true}))?;
        if let Some(e) = r.get("exceptionDetails") {
            let text = e.pointer("/exception/description").or_else(|| e.get("text")).and_then(Value::as_str).unwrap_or("an exception");
            bail!("the page threw: {text}");
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// `method` on the page's session. A dialog the page opens meanwhile ends the wait (the
    /// page answers nothing until it closes): kept for the screen, and the call's answer is null.
    fn page_call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.page_calls(&[(method, params)])
    }

    /// Calls on the page sent at once (see `Cdp::calls`): the last one's result.
    /// How many times this device has waited on Chrome: each call, or batch of calls, is one.
    pub fn round_trips(&self) -> u64 {
        self.browser.cdp.waits
    }

    fn page_calls(&mut self, calls: &[(&str, Value)]) -> Result<Value> {
        self.pump()?;
        let session = self.page.as_ref().map(|p| p.session.clone()).ok_or_else(|| anyhow!("no page is open: launchApp first"))?;
        let opening = |e: &Value| e["method"] == "Page.javascriptDialogOpening" && e["sessionId"] == session.as_str();
        match self.browser.cdp.calls(Some(&session), calls, &opening)? {
            crate::chrome::Answer::Reply(v) => Ok(v),
            crate::chrome::Answer::Stopped(e) => {
                self.on_event(&e)?;
                Ok(Value::Null)
            }
        }
    }

    /// The events read so far: dialogs opening and closing, tabs opening and closing.
    fn pump(&mut self) -> Result<()> {
        while let Some(e) = self.browser.cdp.queued_event() {
            self.on_event(&e)?;
        }
        Ok(())
    }

    fn on_event(&mut self, e: &Value) -> Result<()> {
        let session = self.page.as_ref().map(|p| p.session.as_str()).unwrap_or_default();
        let p = &e["params"];
        match e["method"].as_str().unwrap_or_default() {
            "Page.javascriptDialogOpening" if e["sessionId"] == session => {
                self.dialog = Some(Dialog {
                    kind: p["type"].as_str().unwrap_or("alert").to_string(),
                    message: p["message"].as_str().unwrap_or_default().to_string(),
                    typed: String::new(),
                });
            }
            "Page.javascriptDialogClosed" if e["sessionId"] == session => self.dialog = None,
            "Page.screencastFrame" => {
                if let Some(r) = &mut self.recording {
                    r.count += 1;
                    let _ = std::fs::write(r.frames.join(format!("{:06}.jpg", r.count)), crate::websocket::base64_decode(p["data"].as_str().unwrap_or_default()));
                }
                let s = e["sessionId"].as_str().map(String::from);
                self.browser.cdp.call(s.as_deref(), "Page.screencastFrameAck", json!({"sessionId": p["sessionId"]}))?;
            }
            // A tab a page opened (a link to a new tab, window.open): the page from now on, with
            // the tracker for its next documents and, should it have begun, its first.
            "Target.targetCreated" if p["targetInfo"]["type"] == "page" && self.page.is_some() => {
                let target = p["targetInfo"]["targetId"].as_str().unwrap_or_default().to_string();
                if !self.known.insert(target.clone()) {
                    return Ok(());
                }
                let session = self.browser.cdp.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}))?["sessionId"].as_str().unwrap_or_default().to_string();
                let s = Some(session.as_str());
                self.browser.cdp.call(s, "Page.enable", json!({}))?;
                self.browser.cdp.call(s, "Page.addScriptToEvaluateOnNewDocument", json!({"source": TRACKER_JS}))?;
                self.browser.cdp.call(s, "Runtime.evaluate", json!({"expression": TRACKER_JS}))?;
                let screen = self.screen_calls();
                if !screen.is_empty() {
                    self.browser.cdp.calls(s, &screen, &|_| false)?;
                }
                if let Some(old) = self.page.replace(Page { target, session }) {
                    self.under.push(old);
                }
                self.dialog = None;
            }
            "Target.detachedFromTarget" | "Target.targetDestroyed" => {
                let gone = p["targetId"].as_str().or_else(|| p["sessionId"].as_str()).unwrap_or_default();
                if self.page.as_ref().is_some_and(|pg| pg.target == gone || pg.session == gone) {
                    self.page = self.under.pop();
                    self.dialog = None;
                } else {
                    self.under.retain(|pg| pg.target != gone && pg.session != gone);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Into a screen read (`v`), the frames from another origin it could not read: each read in a
    /// context of its own in that frame, placed after its frame's node, one level deeper, where
    /// the frame is on screen.
    fn add_other_origin_frames(&mut self, v: &mut Value) -> Result<()> {
        let mut xo: Vec<(usize, String, i64, i64)> = v["xo"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| Some((f[0].as_u64()? as usize, f[1].as_str()?.to_string(), f[2].as_i64()?, f[3].as_i64()?)))
            .collect();
        if xo.is_empty() {
            return Ok(());
        }
        let tree = self.page_call("Page.getFrameTree", json!({}))?;
        let mut frames = Vec::new();
        fn collect(t: &Value, out: &mut Vec<(String, String)>) {
            if let (Some(id), Some(url)) = (t["frame"]["id"].as_str(), t["frame"]["url"].as_str()) {
                out.push((id.to_string(), url.to_string()));
            }
            for c in t["childFrames"].as_array().into_iter().flatten() {
                collect(c, out);
            }
        }
        collect(&tree["frameTree"], &mut frames);
        // From the last, so each insertion leaves the earlier indexes where they are.
        xo.sort_by_key(|f| std::cmp::Reverse(f.0));
        for (index, url, left, top) in xo {
            let Some(i) = frames.iter().position(|(_, u)| *u == url) else { continue };
            let (frame, _) = frames.remove(i);
            let Some(read) = self.read_frame(&frame)? else { continue };
            let nodes = v["nodes"].as_array_mut().expect("nodes");
            let depth = nodes.get(index).and_then(|n| n[0].as_i64()).unwrap_or(0) + 1;
            let moved: Vec<Value> = read["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|n| {
                    let mut n = n.clone();
                    n[0] = json!(n[0].as_i64().unwrap_or(0) + depth);
                    for (k, off) in [(1, left), (2, top), (3, left), (4, top)] {
                        n[k] = json!(n[k].as_i64().unwrap_or(0) + off);
                    }
                    n
                })
                .collect();
            nodes.splice(index + 1..index + 1, moved);
        }
        Ok(())
    }

    /// The screen script's answer from inside `frame`, in a context of virtuoso's own there
    /// (made again once should the frame have navigated since); `None` when it cannot be read.
    fn read_frame(&mut self, frame: &str) -> Result<Option<Value>> {
        for fresh in [false, true] {
            let ctx = match self.worlds.get(frame) {
                Some(c) if !fresh => *c,
                _ => {
                    let w = self.page_call("Page.createIsolatedWorld", json!({"frameId": frame, "worldName": "virtuoso"}))?;
                    let Some(c) = w["executionContextId"].as_i64() else { return Ok(None) };
                    self.worlds.insert(frame.to_string(), c);
                    c
                }
            };
            let r = self.page_call("Runtime.evaluate", json!({"expression": SCREEN_JS, "contextId": ctx, "returnByValue": true}));
            if let Ok(r) = r {
                if r.get("exceptionDetails").is_none() && r["result"]["value"].is_object() {
                    return Ok(Some(r["result"]["value"].clone()));
                }
            }
        }
        Ok(None)
    }

    /// Back after `n` frames, with the tracker's state and the screen as they then are, for
    /// the settle and the next read to take (`ahead`): one wait where those were three.
    fn land(&mut self, n: u32) -> Result<()> {
        let looks = format!("r({{state: window.__virtuoso ? window.__virtuoso.state() : null, screen: {SCREEN_JS}}})");
        let wait = (0..n).fold(looks, |inner, _| format!("requestAnimationFrame(() => {inner})"));
        let v = self.evaluate(&format!("new Promise(r => {wait})"))?;
        let at = self.round_trips();
        self.ahead = Some(Ahead { at, state: Some(v["state"].clone()), screen: v.get("screen").filter(|s| s.is_object()).cloned() });
        Ok(())
    }

    /// What `ahead` holds of `pick`, while nothing has been sent to Chrome since it was had.
    fn take_ahead(&mut self, pick: fn(&mut Ahead) -> Option<Value>) -> Option<Value> {
        let at = self.round_trips();
        let a = self.ahead.as_mut().filter(|a| a.at == at)?;
        pick(a)
    }

    /// Back after `n` frames: what an input set going (a scroll) has landed.
    fn frames(&mut self, n: u32) -> Result<()> {
        let wait = (0..n).fold("r()".to_string(), |inner, _| format!("requestAnimationFrame(() => {inner})"));
        self.evaluate(&format!("new Promise(r => {wait})"))?;
        Ok(())
    }

    /// The dialog's answer: accepted or not, with a prompt's text.
    fn answer_dialog(&mut self, accept: bool) -> Result<()> {
        let Some(d) = self.dialog.take() else { return Ok(()) };
        let session = self.page.as_ref().map(|p| p.session.clone()).unwrap_or_default();
        self.browser.cdp.call(Some(&session), "Page.handleJavaScriptDialog", json!({"accept": accept, "promptText": d.typed}))?;
        Ok(())
    }

    /// The screen while a dialog is open: the page as last read, and the dialog over it.
    fn screen_with_dialog(&self) -> ScreenTree {
        let d = self.dialog.as_ref().expect("a dialog");
        let (w, h) = self.last.as_ref().map(|t| (t.width, t.height)).unwrap_or((1280, 800));
        let mut elements: Vec<Element> = self.last.as_ref().map(|t| t.elements.iter().cloned().collect()).unwrap_or_default();
        let buttons = d.buttons();
        let (boxr, message, field, rects) = dialog_layout(w, h, buttons.len());
        let package: Arc<str> = elements.first().map(|e| e.package.clone()).unwrap_or_else(|| Arc::from(""));
        let at = |depth, bounds, text: &str, class: &str, flags| Element { depth, window: 1, bounds, text: text.to_string(), class: Arc::from(class), flags, package: package.clone(), ..Default::default() };
        use crate::screen::{CLICKABLE, EDITABLE, ENABLED, FOCUSED, VISIBLE};
        elements.push(at(0, boxr, "", "dialog", ENABLED | VISIBLE));
        elements.push(at(1, message, &d.message, "p", ENABLED | VISIBLE));
        if d.kind == "prompt" {
            elements.push(at(1, field, &d.typed, "input", CLICKABLE | ENABLED | VISIBLE | EDITABLE | FOCUSED));
        }
        for ((label, _), r) in buttons.iter().zip(rects) {
            elements.push(at(1, r, label, "button", CLICKABLE | ENABLED | VISIBLE));
        }
        ScreenTree::new(elements, w, h)
    }

    /// A fresh tab, attached to with a session of its own, its page events on.
    fn open_page(&mut self) -> Result<()> {
        let target = self.browser.cdp.call(None, "Target.createTarget", json!({"url": "about:blank"}))?["targetId"].as_str().unwrap_or_default().to_string();
        self.known.insert(target.clone());
        let session = self.browser.cdp.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}))?["sessionId"].as_str().unwrap_or_default().to_string();
        self.page = Some(Page { target, session });
        self.page_call("Page.enable", json!({}))?;
        self.page_call("Page.addScriptToEvaluateOnNewDocument", json!({"source": TRACKER_JS}))?;
        let screen = self.screen_calls();
        if !screen.is_empty() {
            self.page_calls(&screen)?;
        }
        Ok(())
    }

    /// The screen every tab is given: the phone emulated (its size, scale, touch and user
    /// agent), else the viewport an orientation asked for, else Chrome's own window.
    fn screen_calls(&self) -> Vec<(&'static str, Value)> {
        let mut calls = Vec::new();
        let (w, h) = self.viewport.or(self.phone.as_ref().map(|p| (p.width, p.height))).unwrap_or_default();
        match &self.phone {
            Some(p) => {
                calls.push(("Emulation.setDeviceMetricsOverride", json!({"width": w, "height": h, "deviceScaleFactor": p.scale, "mobile": true})));
                calls.push(("Emulation.setTouchEmulationEnabled", json!({"enabled": true, "maxTouchPoints": 5})));
                calls.push(("Emulation.setUserAgentOverride", json!({"userAgent": p.user_agent, "platform": "Linux armv81"})));
            }
            None if self.viewport.is_some() => calls.push(("Emulation.setDeviceMetricsOverride", json!({"width": w, "height": h, "deviceScaleFactor": 0, "mobile": false}))),
            None => {}
        }
        calls
    }

    /// Every tab from now on (and the one open) emulates `phone`.
    pub fn emulate(&mut self, phone: Phone) -> Result<()> {
        self.phone = Some(phone);
        if self.page.is_some() {
            let screen = self.screen_calls();
            self.page_calls(&screen)?;
        }
        Ok(())
    }

    /// The page goes to `url`; back once it has loaded.
    fn navigate(&mut self, url: &str) -> Result<()> {
        let r = self.page_call("Page.navigate", json!({"url": url}))?;
        if let Some(e) = r.get("errorText").and_then(Value::as_str) {
            bail!("could not open {url}: {e}");
        }
        let session = self.page.as_ref().map(|p| p.session.clone()).unwrap_or_default();
        let deadline = Instant::now() + LOAD_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.browser.cdp.next_event(left)? {
                Some(e) if e["method"] == "Page.loadEventFired" && e["sessionId"] == session.as_str() => return Ok(()),
                Some(_) => {}
                None => bail!("{url} did not finish loading within {LOAD_TIMEOUT:?}"),
            }
        }
    }
}

/// The script that reads the screen in the page (see its own comment).
/// Compacted by build.rs (no comment lines, no indentation): bytes the binary's budget counts.
const SCREEN_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/web_screen.js"));

/// What a settle reads, installed in every page before its own scripts (see its own comment).
const TRACKER_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/web_tracker.js"));

/// How often a settle looks at the page.
const SETTLE_POLL: Duration = Duration::from_millis(30);

/// The most quiet a settle waits for on the web: the tracker sees requests, short timers and
/// animations itself, where a phone app needs longer to be sure its accessibility events
/// stopped. A desktop Chrome shows an action's work at once: 70 ms (at 50, a page changing
/// every 50 ms looked still between its changes). Chrome on a device (an emulator's, slower by
/// far) keeps 100.
pub fn web_quiet(on_device: bool) -> Duration {
    Duration::from_millis(if on_device { 100 } else { 70 })
}

/// How many frames a scroll's drag waits before the screen is read: one in a desktop Chrome,
/// where the scroll has landed by then (with none, a table was read still moving), two in
/// Chrome on a device (an emulator's, slower by far).
pub fn scroll_frames(on_device: bool) -> u32 {
    if on_device { 2 } else { 1 }
}

/// How long a long press holds on the web: as asked, but no more than 800 ms. A page takes a
/// press of 500 ms as a long one (as Android does), and Puppeteer's port of the benchmark holds
/// 800; the 1000 of a phone's long press only adds time.
pub fn web_long_press(asked: Duration) -> Duration {
    asked.min(Duration::from_millis(800))
}

/// The tracker's state: whether the page is still (quiet `no_change`, no request, no finite
/// animation, loaded), and whether it changed since the mark. `None` (no tracker yet: a
/// document still coming) is neither.
pub fn still(state: &Value, no_change: Duration) -> (bool, bool) {
    let Some(s) = state.as_array() else { return (false, false) };
    let num = |i: usize| s.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let flag = |i: usize| s.get(i).and_then(Value::as_bool).unwrap_or(false);
    let quiet = num(0) >= no_change.as_secs_f64() * 1000.0 && num(1) == 0.0 && num(2) == 0.0 && flag(4);
    (quiet, flag(3))
}

/// How long a settle waits before its next look at the tracker's `state`. A page with nothing
/// pending, loaded, is still once its quiet reaches `no_change`: looked at again just then (a
/// fixed poll looked up to a poll later). A busy page, or none yet, is polled. Never longer
/// than a poll, so a dialog the page opens is seen as soon as before.
pub fn settle_wait(state: &Value, no_change: Duration) -> Duration {
    let Some(s) = state.as_array() else { return SETTLE_POLL };
    let num = |i: usize| s.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let idle = num(1) == 0.0 && num(2) == 0.0 && s.get(4).and_then(Value::as_bool) == Some(true);
    if !idle {
        return SETTLE_POLL;
    }
    let left = (no_change.as_secs_f64() * 1000.0 - num(0)).ceil().max(0.0);
    Duration::from_millis(left as u64).min(SETTLE_POLL)
}

/// The screen script's answer as a tree: one element per node, the tag as its class (one shared
/// string per tag) and the page's origin as its app.
pub fn tree_from(v: &Value, origin: &str) -> Result<ScreenTree> {
    let nodes = v["nodes"].as_array().ok_or_else(|| anyhow!("the page's screen came back without its nodes"))?;
    let package: Arc<str> = Arc::from(origin);
    let mut classes: Vec<Arc<str>> = Vec::new();
    let mut elements = Vec::with_capacity(nodes.len());
    for n in nodes {
        let n = n.as_array().ok_or_else(|| anyhow!("a screen node that is not a list"))?;
        let num = |i: usize| n.get(i).and_then(Value::as_i64).unwrap_or(0);
        let text = |i: usize| n.get(i).and_then(Value::as_str).unwrap_or_default().to_string();
        let tag = n.get(8).and_then(Value::as_str).unwrap_or_default();
        let class = match classes.iter().find(|c| &***c == tag) {
            Some(c) => c.clone(),
            None => {
                let c: Arc<str> = Arc::from(tag);
                classes.push(c.clone());
                c
            }
        };
        elements.push(Element {
            depth: num(0) as u16,
            window: 0,
            bounds: Rect { left: num(1) as i32, top: num(2) as i32, right: num(3) as i32, bottom: num(4) as i32 },
            text: text(5),
            id: text(6),
            hint: text(7),
            desc: text(10),
            class,
            flags: num(9) as u32,
            package: package.clone(),
            ..Default::default()
        });
    }
    Ok(ScreenTree::new(elements, v["w"].as_i64().unwrap_or(0) as i32, v["h"].as_i64().unwrap_or(0) as i32))
}

/// A key as the page hears it: its `key`, `code`, Windows key code, and the text it types.
pub fn key_event(key: Key) -> Option<(&'static str, &'static str, u32, &'static str)> {
    Some(match key {
        Key::Enter | Key::DpadCenter => ("Enter", "Enter", 13, "\r"),
        Key::Backspace => ("Backspace", "Backspace", 8, ""),
        Key::Tab => ("Tab", "Tab", 9, ""),
        Key::Escape => ("Escape", "Escape", 27, ""),
        Key::Space => (" ", "Space", 32, " "),
        Key::DpadUp => ("ArrowUp", "ArrowUp", 38, ""),
        Key::DpadDown => ("ArrowDown", "ArrowDown", 40, ""),
        Key::DpadLeft => ("ArrowLeft", "ArrowLeft", 37, ""),
        Key::DpadRight => ("ArrowRight", "ArrowRight", 39, ""),
        _ => return None,
    })
}

impl WebDevice {
    fn mouse(&mut self, kind: &str, x: i32, y: i32, clicks: u32) -> Result<()> {
        self.page_call("Input.dispatchMouseEvent", mouse_params(kind, x, y, clicks))?;
        Ok(())
    }

    fn key(&mut self, key: &str, code: &str, vk: u32, text: &str) -> Result<()> {
        let mut down = json!({"type": "keyDown", "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk});
        if !text.is_empty() {
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
        }
        // Down and up together: one wait a key (Chrome takes a session's calls in order).
        let k = "Input.dispatchKeyEvent";
        self.page_calls(&[(k, down), (k, json!({"type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk}))])?;
        Ok(())
    }
}

/// A mouse event's parameters: a move with no button, a press holding the left one.
fn mouse_params(kind: &str, x: i32, y: i32, clicks: u32) -> Value {
    let mut p = json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": clicks});
    if kind == "mouseMoved" {
        p["button"] = json!("none");
    }
    if kind == "mousePressed" {
        p["buttons"] = json!(1);
    }
    p
}

/// What `--device` names on the web.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The Chrome installed where virtuoso looks (or VIRTUOSO_CHROME).
    Chrome,
    /// A browser already running, by its WebSocket.
    Attach(String),
    /// A Chrome at this path.
    Path(String),
    /// Chrome on an Android device: the first adb lists, or this serial (`android:<serial>`).
    Android(Option<String>),
}

pub fn target(device: &str) -> Target {
    if device.eq_ignore_ascii_case("chrome") {
        Target::Chrome
    } else if device.eq_ignore_ascii_case("android") {
        Target::Android(None)
    } else if let Some(serial) = device.strip_prefix("android:") {
        Target::Android(Some(serial.to_string()))
    } else if device.starts_with("ws://") {
        Target::Attach(device.to_string())
    } else {
        Target::Path(device.to_string())
    }
}

/// The port a page on this machine is served from (`http://127.0.0.1:4280/...`, `localhost`):
/// what a device reaches it through. Not this machine's page: none.
pub fn local_port(url: &str) -> Option<u16> {
    let (scheme, rest) = url.split_once("://")?;
    let host_port = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (host_port, if scheme == "https" { 443 } else { 80 }),
    };
    matches!(host, "127.0.0.1" | "localhost").then_some(port)
}

/// Chrome's DevTools on an Android device, through adb: the forward to its socket, and the
/// reverses that let the device reach pages this machine serves. Undone when it goes.
struct AndroidLink {
    adb: crate::android::Adb,
    forward: u16,
    reversed: Vec<u16>,
}

impl AndroidLink {
    fn reverse(&mut self, port: u16) -> Result<()> {
        if !self.reversed.contains(&port) {
            self.adb.run(&["reverse", &format!("tcp:{port}"), &format!("tcp:{port}")])?;
            self.reversed.push(port);
        }
        Ok(())
    }
}

impl Drop for AndroidLink {
    fn drop(&mut self) {
        for p in &self.reversed {
            let _ = self.adb.run(&["reverse", "--remove", &format!("tcp:{p}")]);
        }
        let _ = self.adb.run(&["forward", "--remove", &format!("tcp:{}", self.forward)]);
    }
}

/// Chrome on the device `serial` (else the first adb lists): started fresh on a blank page, its
/// first-run screens off (Chrome reads its flags from /data/local/tmp when it is the debug app),
/// its DevTools socket forwarded to a port of this machine, and attached to.
fn android_chrome(serial: Option<&str>) -> Result<WebDevice> {
    let serial = match serial {
        Some(s) => s.to_string(),
        None => crate::android::devices()?.into_iter().next().ok_or_else(|| anyhow!("no Android device: adb lists none"))?,
    };
    let adb = crate::android::Adb::new(&serial);
    adb.shell("echo '_ --disable-fre --no-default-browser-check --no-first-run' > /data/local/tmp/chrome-command-line")?;
    adb.shell("am set-debug-app --persistent com.android.chrome")?;
    adb.shell("am force-stop com.android.chrome")?;
    adb.shell("am start -n com.android.chrome/com.google.android.apps.chrome.Main -d about:blank")?;
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while !adb.shell("cat /proc/net/unix")?.contains("@chrome_devtools_remote") {
        if Instant::now() >= deadline {
            bail!("Chrome on {serial} did not open its DevTools within {LOAD_TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let port: u16 = adb.run(&["forward", "tcp:0", "localabstract:chrome_devtools_remote"])?.trim().parse().map_err(|_| anyhow!("adb forward named no port"))?;
    let link = AndroidLink { adb, forward: port, reversed: Vec::new() };
    let mut d = WebDevice::attach(&format!("ws://127.0.0.1:{port}/devtools/browser"))?;
    d.android = Some(link);
    Ok(d)
}

/// A phone a browser emulates: its viewport (CSS pixels), its device pixel ratio, its user agent.
#[derive(Debug, Clone, PartialEq)]
pub struct Phone {
    pub name: &'static str,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub user_agent: &'static str,
}

/// The phones `--emulate` knows, as Playwright's device list has them (the benchmark gives
/// every tool these same values).
const PHONES: &[Phone] = &[Phone {
    name: "pixel-7",
    width: 412,
    height: 839,
    scale: 2.625,
    user_agent: "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Mobile Safari/537.36",
}];

/// The phone named (`pixel-7`, or `Pixel 7` as it is written).
pub fn emulation(name: &str) -> Result<Phone> {
    let key = name.trim().to_ascii_lowercase().replace(' ', "-");
    PHONES.iter().find(|p| p.name == key).cloned().ok_or_else(|| {
        anyhow!("no phone called `{name}` to emulate: {}", PHONES.iter().map(|p| p.name).collect::<Vec<_>>().join(", "))
    })
}

/// The browser `--device` names, headless unless VIRTUOSO_HEADED is set (`--headed`), emulating
/// the phone VIRTUOSO_EMULATE names (`--emulate`).
pub fn connect(device: &str) -> Result<WebDevice> {
    let headless = std::env::var_os("VIRTUOSO_HEADED").is_none();
    let phone = std::env::var("VIRTUOSO_EMULATE").ok().filter(|s| !s.is_empty()).map(|n| emulation(&n)).transpose()?;
    let mut d = match target(device) {
        Target::Chrome => WebDevice::launch(&crate::chrome::find_chrome()?, headless)?,
        Target::Attach(url) => WebDevice::attach(&url)?,
        Target::Path(p) => WebDevice::launch(Path::new(&p), headless)?,
        Target::Android(serial) => android_chrome(serial.as_deref())?,
    };
    if let Some(p) = phone {
        d.emulate(p)?;
    }
    Ok(d)
}

impl Device for WebDevice {
    fn platform(&self) -> &'static str {
        "web"
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        self.pump()?;
        if self.dialog.is_none() {
            let v = match self.take_ahead(|a| a.screen.take()) {
                Some(screen) => json!({"result": {"value": screen}}),
                None => self.page_call("Runtime.evaluate", json!({"expression": SCREEN_JS, "returnByValue": true}))?,
            };
            if self.dialog.is_none() {
                let mut v = v["result"]["value"].clone();
                let view = &v["view"];
                self.view = (view[0].as_f64().unwrap_or(0.0), view[1].as_f64().unwrap_or(0.0), view[2].as_bool().unwrap_or(false));
                self.add_other_origin_frames(&mut v)?;
                let origin = v["origin"].as_str().unwrap_or_default().to_string();
                let tree = tree_from(&v, &origin)?;
                self.last = Some(tree.clone());
                return Ok(tree);
            }
        }
        Ok(self.screen_with_dialog())
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<()> {
        self.pump()?;
        if let Some(d) = &self.dialog {
            // A dialog takes every tap: its buttons answer it, the rest of the screen is not live.
            let (w, h) = self.last.as_ref().map(|t| (t.width, t.height)).unwrap_or((1280, 800));
            let buttons = d.buttons();
            let (_, _, _, rects) = dialog_layout(w, h, buttons.len());
            if let Some(((_, accept), _)) = buttons.iter().zip(rects).find(|(_, r)| r.contains(x, y)) {
                return self.answer_dialog(*accept);
            }
            return Ok(());
        }
        self.taps(x, y, 1, Duration::ZERO)
    }

    /// Clicks with a rising click count: two make a double click.
    fn taps(&mut self, x: i32, y: i32, count: u32, interval: Duration) -> Result<()> {
        if self.android.is_some() && count <= 1 {
            // Chrome on Android, with a field being typed into: a mouse press takes its focus and
            // the page shifts as the focus goes (the toolbar the field brought back goes again),
            // so the release and its click fall elsewhere. A finger's tap is clicked where it went
            // down: the tap is a touch (placed in the visible part of the page, as the screen is
            // read), done once the page has had its click (Chrome makes it a little after; what
            // comes first can lose it). Not for every tap: after a first one Chrome holds a
            // touch's click some 280 ms (a double tap may follow), and a page that rebuilds
            // itself meanwhile takes the click somewhere else.
            // (Both as the screen the tap was read off had them: no call between that read and
            // the tap, for a page that moves its elements about.)
            let (dx, dy, typing) = self.view;
            if typing {
                const CLICKS: &str = "window.__virtuoso ? __virtuoso.clicks : -1";
                let before = self.evaluate(CLICKS).unwrap_or(Value::Null);
                let t = "Input.dispatchTouchEvent";
                self.page_calls(&[(t, json!({"type": "touchStart", "touchPoints": [{"x": x, "y": y}]})), (t, json!({"type": "touchEnd", "touchPoints": []}))])?;
                let deadline = Instant::now() + Duration::from_millis(600);
                while Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(30));
                    match self.evaluate(CLICKS) {
                        Ok(now) if now != before => break,
                        // A tap that opened another page: its click was had.
                        Err(_) => break,
                        _ => {}
                    }
                }
                return Ok(());
            }
            // Otherwise a mouse tap, its press and release together. A mouse event is placed in
            // the page's own terms, the screen read in its visible part: moved by where that is.
            let (px, py) = (x as f64 + dx, y as f64 + dy);
            let m = "Input.dispatchMouseEvent";
            self.page_calls(&[
                (m, json!({"type": "mouseMoved", "x": px, "y": py, "button": "none"})),
                (m, json!({"type": "mousePressed", "x": px, "y": py, "button": "left", "buttons": 1, "clickCount": 1})),
                (m, json!({"type": "mouseReleased", "x": px, "y": py, "button": "left", "clickCount": 1})),
            ])?;
            return Ok(());
        }
        // The move and each click's press and release go together (Chrome takes them in order):
        // one wait a click, where each alone was a round trip.
        let m = "Input.dispatchMouseEvent";
        let mut batch = vec![(m, mouse_params("mouseMoved", x, y, 0))];
        for n in 1..=count.max(1) {
            if n > 1 {
                self.page_calls(&batch)?;
                batch.clear();
                std::thread::sleep(interval);
            }
            batch.push((m, mouse_params("mousePressed", x, y, n)));
            batch.push((m, mouse_params("mouseReleased", x, y, n)));
        }
        self.page_calls(&batch)?;
        Ok(())
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.mouse("mouseMoved", x, y, 0)?;
        self.mouse("mousePressed", x, y, 1)?;
        std::thread::sleep(web_long_press(Duration::from_millis(ms)));
        self.mouse("mouseReleased", x, y, 1)
    }

    /// A finger's swipe, as Maestro's web swipe is: it drags what takes pointer drags
    /// (touch-action: none) and scrolls the rest. Lifted still it holds first (no fling);
    /// cancelled, the touch is cancelled where it is. Back once the scroll has landed.
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, end: Lift) -> Result<()> {
        let touch = |kind: &str, at: Option<(i32, i32)>| json!({"type": kind, "touchPoints": at.map(|(x, y)| vec![json!({"x": x, "y": y})]).unwrap_or_default()});
        let t = "Input.dispatchTouchEvent";
        if end == Lift::Cancel {
            // A scroll's drag (scrollUntilVisible, scroll) only has to scroll: one move does as
            // far, and its start, move and cancel go together (Chrome takes them in order), one
            // wait for the three where each alone is a ~45 ms round trip.
            if self.phone.is_none() && self.android.is_none() {
                // A desktop Chrome: the mouse wheel over where the drag starts, as far as the
                // drag (a finger moving up scrolls the content down), as a desktop user scrolls
                // and Playwright and Puppeteer do. One event, and no drag library takes it.
                let wheel = json!({"type": "mouseWheel", "x": from.0, "y": from.1, "deltaX": from.0 - to.0, "deltaY": from.1 - to.1});
                self.page_call("Input.dispatchMouseEvent", wheel)?;
            } else {
                self.page_calls(&[(t, touch("touchStart", Some(from))), (t, touch("touchMove", Some(to))), (t, touch("touchCancel", None))])?;
            }
            return self.land(scroll_frames(self.android.is_some()));
        }
        // A swipe: three moves, in its time, are a swipe to scrollers and drag libraries alike.
        const STEPS: i32 = 3;
        self.page_call(t, touch("touchStart", Some(from)))?;
        for i in 1..=STEPS {
            let at = (from.0 + (to.0 - from.0) * i / STEPS, from.1 + (to.1 - from.1) * i / STEPS);
            self.page_call(t, touch("touchMove", Some(at)))?;
            std::thread::sleep(Duration::from_millis(ms / STEPS as u64));
        }
        if end == Lift::Still {
            std::thread::sleep(Duration::from_millis(120));
            self.page_call(t, touch("touchMove", Some(to)))?;
        }
        self.page_call(t, touch("touchEnd", None))?;
        self.frames(2)
    }

    /// Press, hold, move in steps, release: what follows the pointer follows it.
    fn drag(&mut self, from: (i32, i32), to: (i32, i32), hold_ms: u64, move_ms: u64) -> Result<()> {
        const STEPS: i32 = 10;
        self.mouse("mouseMoved", from.0, from.1, 0)?;
        self.mouse("mousePressed", from.0, from.1, 1)?;
        std::thread::sleep(Duration::from_millis(hold_ms));
        for i in 1..=STEPS {
            let (x, y) = (from.0 + (to.0 - from.0) * i / STEPS, from.1 + (to.1 - from.1) * i / STEPS);
            self.page_call("Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": x, "y": y, "button": "left", "buttons": 1}))?;
            std::thread::sleep(Duration::from_millis(move_ms / STEPS as u64));
        }
        self.mouse("mouseReleased", to.0, to.1, 1)
    }

    fn press_key(&mut self, key: Key) -> Result<()> {
        self.pump()?;
        if self.dialog.is_some() {
            return match key {
                Key::Enter | Key::DpadCenter => self.answer_dialog(true),
                Key::Escape | Key::Back => self.answer_dialog(false),
                Key::Backspace => self.erase_text(1),
                _ => Ok(()),
            };
        }
        if key == Key::Back {
            // A tab a page opened, with no history of its own: Back closes it and comes back to
            // the tab under it, as in mobile Chrome. Otherwise the page goes back.
            if !self.under.is_empty() && self.evaluate("history.length")?.as_i64().unwrap_or(1) <= 1 {
                if let Some(p) = self.page.take() {
                    self.browser.cdp.call(None, "Target.closeTarget", json!({"targetId": p.target}))?;
                }
                self.page = self.under.pop();
                return Ok(());
            }
            self.evaluate("history.back()")?;
            return Ok(());
        }
        let Some((k, code, vk, text)) = key_event(key) else { bail!("the {key:?} key is not on the web") };
        self.key(k, code, vk, text)
    }

    fn input_text(&mut self, text: &str) -> Result<()> {
        self.pump()?;
        if let Some(d) = &mut self.dialog {
            d.typed.push_str(text);
            return Ok(());
        }
        // A key at a time, as a keyboard types (and Maestro's web inputText): a page that moves
        // the focus as each character lands (a code split into boxes) sees each one.
        let mut buf = [0u8; 4];
        for ch in text.chars() {
            let c = ch.encode_utf8(&mut buf);
            let c = if c == "\n" { "\r" } else { c };
            // Its down and up together: one wait a character. Still a character at a time, so
            // a page that moves the focus as each lands sees each one.
            let k = "Input.dispatchKeyEvent";
            self.page_calls(&[(k, json!({"type": "keyDown", "key": c, "text": c, "unmodifiedText": c})), (k, json!({"type": "keyUp", "key": c}))])?;
        }
        Ok(())
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        self.pump()?;
        if let Some(d) = &mut self.dialog {
            for _ in 0..chars {
                d.typed.pop();
            }
            return Ok(());
        }
        for _ in 0..chars {
            self.key("Backspace", "Backspace", 8, "")?;
        }
        Ok(())
    }

    fn mark(&mut self) -> Result<()> {
        self.pump()?;
        if self.dialog.is_none() {
            self.page_call("Runtime.evaluate", json!({"expression": "window.__virtuoso && window.__virtuoso.mark()"}))?;
        }
        Ok(())
    }

    /// The tracker waits for requests, short timers and animations whatever the quiet: a
    /// scroll's drags need no quiet of their own (a virtualized list re-renders at each).
    fn scroll_quiet(&self) -> Duration {
        Duration::ZERO
    }

    /// Back once the page has been still for `no_change` (see `still`), or at `timeout`.
    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let no_change = no_change.min(web_quiet(self.android.is_some()));
        let deadline = Instant::now() + timeout;
        let mut changed = false;
        loop {
            self.pump()?;
            if self.dialog.is_some() {
                // The page is held by its dialog: the screen will not change until it is answered.
                return Ok(Settled { changed: true, timed_out: false });
            }
            // A read during a navigation can find no page context: the page is not still yet.
            let state = match self.take_ahead(|a| a.state.take()) {
                Some(state) => state,
                None => self.evaluate("window.__virtuoso ? window.__virtuoso.state() : null").unwrap_or(Value::Null),
            };
            let (quiet, c) = still(&state, no_change);
            changed |= c || state.is_null();
            if quiet {
                return Ok(Settled { changed, timed_out: false });
            }
            if Instant::now() >= deadline {
                return Ok(Settled { changed, timed_out: true });
            }
            std::thread::sleep(settle_wait(&state, no_change));
        }
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        let r = self.page_call("Page.captureScreenshot", json!({"format": "png"}))?;
        Ok(crate::websocket::base64_decode(r["data"].as_str().unwrap_or_default()))
    }

    /// Chrome's screencast, its frames kept until stop_recording makes them a video (ffmpeg).
    fn start_recording(&mut self, path: &Path) -> Result<()> {
        if self.recording.is_some() {
            bail!("a recording is already running");
        }
        let frames = std::env::temp_dir().join(format!("virtuoso-frames-{}-{}", std::process::id(), Instant::now().elapsed().as_nanos()));
        std::fs::create_dir_all(&frames)?;
        self.recording = Some(Recording { path: path.to_path_buf(), frames, count: 0, started: Instant::now() });
        self.page_call("Page.startScreencast", json!({"format": "jpeg", "quality": 70}))?;
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<()> {
        let _ = self.page_call("Page.stopScreencast", json!({}));
        let r = self.recording.take().ok_or_else(|| anyhow!("no recording is running"))?;
        let fps = (r.count.max(1) as f64 / r.started.elapsed().as_secs_f64().max(0.1)).clamp(1.0, 30.0);
        let made = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-framerate", &format!("{fps:.2}"), "-i"])
            .arg(r.frames.join("%06d.jpg"))
            .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&r.path)
            .status();
        let _ = std::fs::remove_dir_all(&r.frames);
        match made {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => bail!("ffmpeg could not make the recording ({s})"),
            Err(_) => bail!("a recording on the web needs ffmpeg on the PATH"),
        }
    }

    fn set_location(&mut self, lat: f64, lon: f64) -> Result<()> {
        let origin = self.evaluate("location.origin")?;
        self.browser.cdp.call(None, "Browser.setPermission", json!({"permission": {"name": "geolocation"}, "setting": "granted", "origin": origin}))?;
        self.page_call("Emulation.setGeolocationOverride", json!({"latitude": lat, "longitude": lon, "accuracy": 1}))?;
        Ok(())
    }

    /// For the site `app` is on: allow, deny or unset (ask again) each permission named.
    fn set_permissions(&mut self, app: &str, perms: &[(String, String)]) -> Result<()> {
        let origin = origin(app)?;
        for (name, value) in perms {
            let setting = match value.as_str() {
                "allow" => "granted",
                "deny" => "denied",
                _ => "prompt",
            };
            let names: Vec<&str> = if name == "all" { vec!["geolocation", "camera", "microphone", "notifications", "clipboard-read", "midi"] } else { permission_name(name).into_iter().collect() };
            for n in names {
                self.browser.cdp.call(None, "Browser.setPermission", json!({"permission": {"name": n}, "setting": setting, "origin": origin}))?;
            }
        }
        Ok(())
    }

    fn set_dark_mode(&mut self, on: Option<bool>) -> Result<()> {
        let value = match on {
            Some(true) => "dark",
            Some(false) => "light",
            None => "",
        };
        self.page_call("Emulation.setEmulatedMedia", json!({"features": [{"name": "prefers-color-scheme", "value": value}]}))?;
        Ok(())
    }

    fn dark_mode(&mut self) -> Result<bool> {
        Ok(self.evaluate("matchMedia('(prefers-color-scheme: dark)').matches")? == json!(true))
    }

    /// Offline (Some(true)) or back online: the page's network, as airplane mode takes a phone's.
    fn set_airplane_mode(&mut self, on: Option<bool>) -> Result<()> {
        self.page_call("Network.enable", json!({}))?;
        let offline = on.unwrap_or(false);
        self.page_call("Network.emulateNetworkConditions", json!({"offline": offline, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1}))?;
        Ok(())
    }

    /// The viewport turned: portrait taller than wide, landscape wider than tall.
    fn set_orientation(&mut self, o: crate::device::Orientation) -> Result<()> {
        use crate::device::Orientation::*;
        let (w, h) = (self.evaluate("innerWidth")?.as_i64().unwrap_or(1280) as i32, self.evaluate("innerHeight")?.as_i64().unwrap_or(800) as i32);
        let (long, short) = (w.max(h), w.min(h));
        let (w, h) = if matches!(o, Portrait | UpsideDown) { (short, long) } else { (long, short) };
        self.viewport = Some((w, h));
        let screen = self.screen_calls();
        self.page_calls(&screen)?;
        Ok(())
    }

    fn open_link(&mut self, link: &str, _app: Option<&str>, _browser: bool) -> Result<()> {
        if self.page.is_none() {
            self.open_page()?;
        }
        self.navigate(link)
    }

    /// A desktop page has no keyboard to wait for, or to put away.
    fn await_keyboard(&mut self, _wait: Duration, _poll: Duration) -> Result<()> {
        Ok(())
    }

    fn hide_keyboard(&mut self) -> Result<()> {
        Ok(())
    }

    fn launch_app(&mut self, app: &str, opts: &LaunchOptions) -> Result<()> {
        // On a device, a page this machine serves is reached through an adb reverse of its port.
        if let (Some(link), Some(port)) = (&mut self.android, local_port(app)) {
            link.reverse(port)?;
        }
        if opts.stop_app || self.page.is_none() {
            self.stop_app(app)?;
            self.open_page()?;
            if opts.clear_state {
                self.clear_state(app)?;
            }
        }
        self.navigate(app)
    }

    fn stop_app(&mut self, _app: &str) -> Result<()> {
        self.dialog = None;
        self.last = None;
        for p in self.under.drain(..).chain(self.page.take()).collect::<Vec<_>>() {
            // A tab the page closed itself is gone already.
            let _ = self.browser.cdp.call(None, "Target.closeTarget", json!({"targetId": p.target}));
        }
        Ok(())
    }

    fn clear_state(&mut self, app: &str) -> Result<()> {
        let origin = origin(app)?;
        // On a page's session: the browser's own answers it with an internal error.
        if self.page.is_none() {
            self.open_page()?;
        }
        self.page_call("Storage.clearDataForOrigin", json!({"origin": origin, "storageTypes": "all"}))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_to_emulate_is_known_by_name_and_an_unknown_one_says_which_are() {
        let p = emulation("pixel-7").unwrap();
        assert_eq!((p.width, p.height), (412, 839));
        assert!(p.user_agent.contains("Pixel 7") && p.user_agent.contains("Mobile"), "{}", p.user_agent);
        assert!(p.scale > 2.0);
        assert_eq!(emulation("Pixel 7").unwrap().width, 412, "as its name is written");
        let e = emulation("nokia-3310").unwrap_err().to_string();
        assert!(e.contains("pixel-7"), "{e}");
    }

    #[test]
    fn the_scripts_go_into_the_binary_without_comments_or_indentation() {
        for js in [SCREEN_JS, TRACKER_JS] {
            assert!(js.lines().all(|l| !l.trim_start().starts_with("//")), "a comment line in {js:.80}");
            assert!(js.lines().all(|l| !l.starts_with(' ')), "an indented line in {js:.80}");
            assert!(js.contains("window") || js.contains("document"), "the script itself is there");
        }
    }

    #[test]
    fn still_means_quiet_long_enough_nothing_pending_and_loaded() {
        let ms = Duration::from_millis;
        assert_eq!(still(&json!([250.0, 0, 0, false, true]), ms(200)), (true, false));
        assert_eq!(still(&json!([250.0, 0, 0, true, true]), ms(200)), (true, true), "changed since the mark");
        assert!(!still(&json!([150.0, 0, 0, false, true]), ms(200)).0, "not quiet long enough");
        assert!(!still(&json!([900.0, 1, 0, false, true]), ms(200)).0, "a request in flight");
        assert!(!still(&json!([900.0, 0, 2, false, true]), ms(200)).0, "an animation running");
        assert!(!still(&json!([900.0, 0, 0, false, false]), ms(200)).0, "still loading");
        assert_eq!(still(&Value::Null, ms(200)), (false, false), "no tracker yet");
    }

    #[test]
    fn a_desktop_chrome_reads_a_scroll_one_frame_after_its_drag_and_chrome_on_a_device_two() {
        // Read with no frame at all, the table was still moving and the tap missed what it saw.
        assert_eq!(scroll_frames(false), 1);
        assert_eq!(scroll_frames(true), 2);
    }

    #[test]
    fn a_long_press_holds_800_ms_at_most_well_past_the_500_a_page_takes_for_one() {
        assert_eq!(web_long_press(Duration::from_millis(1000)), Duration::from_millis(800));
        assert_eq!(web_long_press(Duration::from_millis(600)), Duration::from_millis(600));
    }

    #[test]
    fn a_desktop_chrome_settles_after_70_ms_of_quiet_and_chrome_on_a_device_after_100() {
        // On a desktop the tracker sees requests, short timers and animations as they happen;
        // Chrome on a device (an emulator's) is slower to show what an action set going. Not
        // 50: a page changing every 50 ms (a ticker) would look still between its changes.
        assert_eq!(web_quiet(false), Duration::from_millis(70));
        assert_eq!(web_quiet(true), Duration::from_millis(100));
    }

    #[test]
    fn a_settle_looks_again_when_its_quiet_will_be_up_and_polls_while_the_page_is_busy() {
        let ms = Duration::from_millis;
        // Nothing pending, loaded: 90 ms of the 100 asked for are quiet, so the next look is
        // in 10 ms (the next poll would come 20 ms after the page is still).
        assert_eq!(settle_wait(&json!([90.0, 0, 0, false, true]), ms(100)), ms(10));
        assert_eq!(settle_wait(&json!([89.4, 0, 0, false, true]), ms(100)), ms(11), "whole ms, not early");
        // Never longer than a poll: a dialog opening meanwhile is seen as soon as before.
        assert_eq!(settle_wait(&json!([5.0, 0, 0, true, true]), ms(100)), SETTLE_POLL);
        // A page still busy changes the quiet before it is up: polled.
        assert_eq!(settle_wait(&json!([90.0, 1, 0, false, true]), ms(100)), SETTLE_POLL, "a request in flight");
        assert_eq!(settle_wait(&json!([90.0, 0, 1, false, true]), ms(100)), SETTLE_POLL, "an animation running");
        assert_eq!(settle_wait(&json!([90.0, 0, 0, false, false]), ms(100)), SETTLE_POLL, "still loading");
        assert_eq!(settle_wait(&Value::Null, ms(100)), SETTLE_POLL, "no tracker yet");
    }

    #[test]
    fn a_device_name_is_chrome_a_browser_to_attach_to_or_a_browsers_path() {
        assert_eq!(target("chrome"), Target::Chrome);
        assert_eq!(target("Chrome"), Target::Chrome);
        assert_eq!(target("ws://127.0.0.1:9222/devtools/browser/x"), Target::Attach("ws://127.0.0.1:9222/devtools/browser/x".into()));
        assert_eq!(target("/opt/chrome/chrome"), Target::Path("/opt/chrome/chrome".into()));
    }

    #[test]
    fn android_names_chrome_on_a_device_the_first_one_or_by_serial() {
        assert_eq!(target("android"), Target::Android(None));
        assert_eq!(target("Android"), Target::Android(None));
        assert_eq!(target("android:emulator-5554"), Target::Android(Some("emulator-5554".into())));
    }

    #[test]
    fn a_flows_own_host_on_this_machine_is_the_port_a_device_reaches_it_through() {
        assert_eq!(local_port("http://127.0.0.1:4280/e/login-form"), Some(4280));
        assert_eq!(local_port("http://localhost:8080"), Some(8080));
        assert_eq!(local_port("http://localhost/x"), Some(80));
        assert_eq!(local_port("https://example.com/"), None, "not this machine: no reverse");
        assert_eq!(local_port("about:blank"), None);
    }

    #[test]
    fn a_screen_read_becomes_a_tree_of_elements() {
        let v = json!({"w": 1280, "h": 800, "nodes": [
            [0, 0, 0, 1280, 800, "", "", "", "html", 34],
            [1, 8, 8, 60, 30, "Alpha", "by-id", "", "button", 35],
            [1, 8, 40, 200, 60, "", "field", "Email", "input", 163],
            [2, 8, 70, 40, 90, "Bravo", "", "", "button", 35]
        ]});
        let t = tree_from(&v, "http://127.0.0.1:4280").unwrap();
        assert_eq!((t.width, t.height, t.elements.len()), (1280, 800, 4));
        let b = &t.elements[1];
        assert_eq!((b.text.as_str(), b.id.as_str(), &*b.class, b.flags), ("Alpha", "by-id", "button", 35));
        assert_eq!(b.bounds, Rect { left: 8, top: 8, right: 60, bottom: 30 });
        assert_eq!(&*b.package, "http://127.0.0.1:4280");
        assert_eq!(t.elements[2].hint, "Email");
        assert_eq!(t.elements[1].parent, Some(0));
        assert_eq!(t.elements[3].parent, Some(2), "a deeper node's parent is the nearest shallower one before it");
        assert!(Arc::ptr_eq(&t.elements[1].class, &t.elements[3].class), "one string per tag");
    }

    #[test]
    fn a_sites_origin_is_its_scheme_host_and_port() {
        assert_eq!(origin("http://127.0.0.1:4280/e/login?x=1#top").unwrap(), "http://127.0.0.1:4280");
        assert_eq!(origin("https://example.com").unwrap(), "https://example.com");
        assert_eq!(origin("https://example.com?q").unwrap(), "https://example.com");
        assert!(origin("example.com").is_err());
        assert!(origin("http://").is_err());
    }
}
