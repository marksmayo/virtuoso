//! WebDriver classic under the DevTools Protocol client, for Safari (safaridriver speaks
//! classic WebDriver over HTTP, and only part of BiDi): each CDP call the web device makes is
//! run here as safaridriver's commands, and answered in CDP's shape. Classic has no events: the
//! ones the device follows are made here from what the commands find (a load at a navigation's
//! end, a dialog at a command it held up, a tab come or gone after input). A tab's window handle
//! stands for both CDP's target and its session.

use crate::bidi::{pointer_actions, webdriver_key, Pointers};
use crate::chrome::Transport;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A WebDriver command's error: its code (`no such window`) and message.
#[derive(Debug, Clone, PartialEq)]
pub struct WdError {
    pub error: String,
    pub message: String,
}

impl WdError {
    pub fn new(error: &str, message: &str) -> WdError {
        WdError { error: error.to_string(), message: message.to_string() }
    }
}

impl std::fmt::Display for WdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() {
            write!(f, "{}", self.error)
        } else {
            write!(f, "{}: {}", self.error, self.message)
        }
    }
}

/// A WebDriver session's commands: `path` is after the session's own (`/url`).
pub trait Driver {
    fn command(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, WdError>;
}

/// A page script, run by safaridriver as an asynchronous one: the scripts every document is to
/// run first (once a document: classic WebDriver has none of its own), then the expression,
/// by indirect eval (as a script's completion value, the way CDP evaluates it), awaited if
/// asked. What it answers: `{value}`, or `{thrown}` with what was thrown.
pub const EVALUATE_JS: &str = "const [expression, preload, awaited, byValue, done] = arguments;
if (preload && !window.__virtuosoPreloaded) { window.__virtuosoPreloaded = true; for (const s of preload) { try { (0, eval)(s); } catch (e) {} } }
const fail = (e) => done({thrown: String(e)});
let v;
try { v = (0, eval)(expression); } catch (e) { fail(e); return; }
const give = (r) => done({value: byValue && r !== undefined ? r : null});
if (awaited && v && typeof v.then === 'function') v.then(give, fail); else give(v);";

/// The CDP client's transport over a WebDriver classic session (see the module).
pub struct Classic<D: Driver> {
    driver: D,
    /// The replies and events made here, for the client to read.
    ready: VecDeque<String>,
    /// The window the session's commands go to (classic WebDriver's current one), once known.
    pub current: Option<String>,
    /// The scripts every document runs first (`Page.addScriptToEvaluateOnNewDocument`).
    preload: Vec<String>,
    /// The events the calls being run have made: handed on before their answers.
    events: Vec<Value>,
    /// Navigations made: each one's loader id.
    navigations: u64,
    /// `Browser.getVersion`'s product ("Safari/26.1").
    product: String,
    /// The windows (tabs) known to be open.
    pub handles: Vec<String>,
    /// Where the mouse is, as the browser has it.
    pointers: Pointers,
    /// Whether a dialog is open (told of, not yet answered).
    dialog: bool,
    /// Until when calls look for tabs input opened (see `TAB_WATCH`).
    pub watch_until: Option<Instant>,
}

/// How long after input calls look for the tabs it opened or closed (as the web tracker counts a
/// tab a page opens as work pending for 5 s).
const TAB_WATCH: Duration = Duration::from_secs(5);

/// What a CDP call is answered with: its result, or an error's message.
type Answered = std::result::Result<Value, String>;

impl<D: Driver> Classic<D> {
    pub fn new(driver: D, product: &str) -> Classic<D> {
        Classic { driver, ready: VecDeque::new(), current: None, preload: Vec::new(), events: Vec::new(), navigations: 0, product: product.to_string(), handles: Vec::new(), pointers: Pointers::default(), dialog: false, watch_until: None }
    }

    /// A command on the tab `ctx`: its window made the current one first, if it is not.
    fn on(&mut self, ctx: Option<&str>, method: &str, path: &str, body: Option<&Value>) -> Result<Value, WdError> {
        if let Some(ctx) = ctx.filter(|c| !c.is_empty() && self.current.as_deref() != Some(*c)) {
            self.driver.command("POST", "/window", Some(&json!({"handle": ctx})))?;
            self.current = Some(ctx.to_string());
        }
        let r = self.driver.command(method, path, body);
        if matches!(&r, Err(e) if e.error == HELD) {
            self.dialog_opened(ctx);
        }
        r
    }

    /// A dialog held a command up: told of, once, as Chrome's opening is. WebDriver tells a
    /// prompt (it takes text) from the rest, and no more: an alert is told of as a confirm.
    fn dialog_opened(&mut self, ctx: Option<&str>) {
        if self.dialog {
            return;
        }
        self.dialog = true;
        let message = self.driver.command("GET", "/alert/text", None).ok().and_then(|m| m.as_str().map(String::from)).unwrap_or_default();
        let kind = if self.driver.command("POST", "/alert/text", Some(&json!({"text": ""}))).is_ok() { "prompt" } else { "confirm" };
        let ctx = ctx.map(String::from).or_else(|| self.current.clone());
        self.events.push(json!({"method": "Page.javascriptDialogOpening", "sessionId": ctx, "params": {"type": kind, "message": message, "defaultPrompt": ""}}));
    }

    /// One CDP call on the tab `ctx` (a window handle), run as safaridriver's commands.
    fn run(&mut self, method: &str, params: &Value, ctx: Option<&str>) -> Answered {
        let wd = |e: WdError| e.to_string();
        match method {
            "Target.setDiscoverTargets" | "Page.enable" | "Network.enable" | "Page.screencastFrameAck" => Ok(json!({})),
            "Browser.getVersion" => Ok(json!({"product": self.product, "userAgent": ""})),
            "Target.attachToTarget" => Ok(json!({"sessionId": params["targetId"]})),
            "Target.getTargets" => {
                let handles = self.driver.command("GET", "/window/handles", None).map_err(wd)?;
                let current = self.driver.command("GET", "/window", None).map_err(wd)?;
                self.current = current.as_str().map(String::from);
                let url = match self.driver.command("GET", "/url", None).map_err(wd)? {
                    Value::String(u) if !u.is_empty() => u,
                    _ => "about:blank".to_string(),
                };
                self.handles = handles.as_array().into_iter().flatten().filter_map(|h| h.as_str().map(String::from)).collect();
                let infos: Vec<Value> = self.handles.iter().map(|h| json!({"targetId": h, "type": "page", "url": if Some(h) == self.current.as_ref() { url.as_str() } else { "" }})).collect();
                Ok(json!({"targetInfos": infos}))
            }
            "Target.createTarget" => {
                let made = self.driver.command("POST", "/window/new", Some(&json!({"type": "tab"}))).map_err(wd)?;
                let handle = made["handle"].as_str().unwrap_or_default().to_string();
                self.handles.push(handle.clone());
                Ok(json!({"targetId": handle}))
            }
            "Target.closeTarget" => {
                let target = params["targetId"].as_str().unwrap_or_default().to_string();
                self.on(Some(&target), "DELETE", "/window", None).map_err(wd)?;
                self.current = None;
                self.handles.retain(|h| *h != target);
                self.events.push(json!({"method": "Target.detachedFromTarget", "params": {"sessionId": target, "targetId": target}}));
                Ok(json!({"success": true}))
            }
            // WebDriver's own actions (a drag's whole sequence), sent as they are to their tab.
            "input.performActions" => {
                let ctx = params["context"].as_str();
                self.on(ctx, "POST", "/actions", Some(&json!({"actions": params["actions"]}))).map(|_| json!({})).map_err(wd)
            }
            // The top frame alone: frames from other origins are not read on Safari.
            "Page.getFrameTree" => Ok(json!({"frameTree": {"frame": {"id": ctx, "url": ""}, "childFrames": []}})),
            "Page.navigate" => match self.on(ctx, "POST", "/url", Some(&json!({"url": params["url"]}))) {
                Ok(_) => {
                    // Classic WebDriver answers a navigation once its page has loaded.
                    self.events.push(json!({"method": "Page.loadEventFired", "sessionId": ctx, "params": {}}));
                    self.navigations += 1;
                    Ok(json!({"frameId": "", "loaderId": self.navigations.to_string()}))
                }
                // Held up by a dialog the page opened as it loaded: back with it open.
                Err(e) if e.error == HELD => Ok(json!({"frameId": ""})),
                Err(e) => Ok(json!({"frameId": "", "errorText": e.to_string()})),
            },
            "Page.handleJavaScriptDialog" => {
                if let Some(t) = params["promptText"].as_str().filter(|t| !t.is_empty()) {
                    self.driver.command("POST", "/alert/text", Some(&json!({"text": t}))).map_err(wd)?;
                }
                let accept = params["accept"].as_bool().unwrap_or(true);
                self.driver.command("POST", if accept { "/alert/accept" } else { "/alert/dismiss" }, None).map_err(wd)?;
                self.dialog = false;
                self.events.push(json!({"method": "Page.javascriptDialogClosed", "sessionId": ctx, "params": {"result": accept}}));
                Ok(json!({}))
            }
            "Page.addScriptToEvaluateOnNewDocument" => {
                let source = params["source"].as_str().unwrap_or_default().to_string();
                let at = match self.preload.iter().position(|s| *s == source) {
                    Some(i) => i,
                    None => {
                        self.preload.push(source);
                        self.preload.len() - 1
                    }
                };
                Ok(json!({"identifier": (at + 1).to_string()}))
            }
            "Runtime.evaluate" => {
                if !params["contextId"].is_null() {
                    return Err("isolated worlds are not on Safari".to_string());
                }
                let preload = if self.preload.is_empty() { Value::Null } else { json!(self.preload) };
                let args = json!([params["expression"], preload, params["awaitPromise"].as_bool().unwrap_or(false), params["returnByValue"].as_bool().unwrap_or(false)]);
                match self.on(ctx, "POST", "/execute/async", Some(&json!({"script": EVALUATE_JS, "args": args}))) {
                    Ok(v) => match v.get("thrown") {
                        Some(t) => Ok(json!({"exceptionDetails": {"text": t}})),
                        None => Ok(json!({"result": {"value": v.get("value").cloned().unwrap_or(Value::Null)}})),
                    },
                    Err(e) if e.error == "javascript error" => Ok(json!({"exceptionDetails": {"text": e.to_string()}})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "Page.createIsolatedWorld" => Err("isolated worlds are not on Safari".to_string()),
            "Page.captureScreenshot" => self.on(ctx, "GET", "/screenshot", None).map(|data| json!({"data": data})).map_err(wd),
            // From a page of the site's own (see the web device's clear_state): the cookies it sees.
            "Storage.clearDataForOrigin" => self.on(ctx, "DELETE", "/cookie", None).map(|_| json!({})).map_err(wd),
            "Emulation.setDeviceMetricsOverride" if params["mobile"] != true => {
                // The window is sized: as much bigger than the viewport asked for as it is now.
                let chrome = self.on(ctx, "POST", "/execute/sync", Some(&json!({"script": "return [outerWidth - innerWidth, outerHeight - innerHeight]", "args": []}))).map_err(wd)?;
                let (dw, dh) = (chrome[0].as_i64().unwrap_or(0), chrome[1].as_i64().unwrap_or(0));
                let (w, h) = (params["width"].as_i64().unwrap_or(0), params["height"].as_i64().unwrap_or(0));
                self.on(ctx, "POST", "/window/rect", Some(&json!({"width": w + dw, "height": h + dh}))).map(|_| json!({})).map_err(wd)
            }
            "Emulation.setDeviceMetricsOverride" | "Emulation.setTouchEmulationEnabled" => Err("a phone is not on Safari: WebDriver sets no device scale, touch or mobile viewport".to_string()),
            "Emulation.setUserAgentOverride" => Err("a user agent of its own is not on Safari".to_string()),
            "Emulation.setEmulatedMedia" => Err("dark mode is not on Safari: WebDriver sets no colour scheme".to_string()),
            "Emulation.setGeolocationOverride" => Err("a location is not on Safari: WebDriver sets none".to_string()),
            "Browser.setPermission" => Err("permissions are not on Safari: WebDriver sets none".to_string()),
            "Network.emulateNetworkConditions" => Err("airplane mode is not on Safari: WebDriver takes no network offline".to_string()),
            "Page.startScreencast" | "Page.stopScreencast" => Err("recording is not on Safari: WebDriver has no screencast".to_string()),
            other => Err(format!("{other} is not on Safari: it has no WebDriver classic counterpart here")),
        }
    }


    /// One source's input, gathered: its sequence of actions sent, and each call answered.
    fn flush(&mut self, g: Group, answers: &mut Vec<String>) {
        let mut source = g.source;
        source["actions"] = json!(g.actions);
        let answered = self.on(g.ctx.as_deref(), "POST", "/actions", Some(&json!({"actions": [source]}))).map(|_| json!({})).map_err(|e| e.to_string());
        for id in g.ids {
            answers.push(answer(id, answered.clone()));
        }
    }

    /// Whether input opened tabs, or closed them (window.open, a link to a new tab,
    /// window.close()): each told of, as Chrome tells of its targets.
    fn look_for_tabs(&mut self) {
        let Ok(now) = self.driver.command("GET", "/window/handles", None) else { return };
        let now: Vec<String> = now.as_array().into_iter().flatten().filter_map(|h| h.as_str().map(String::from)).collect();
        if !self.handles.is_empty() {
            for h in now.iter().filter(|h| !self.handles.contains(h)) {
                self.events.push(json!({"method": "Target.targetCreated", "params": {"targetInfo": {"targetId": h, "type": "page", "url": ""}}}));
            }
            for h in self.handles.iter().filter(|h| !now.contains(h)) {
                if self.current.as_ref() == Some(h) {
                    self.current = None;
                }
                self.events.push(json!({"method": "Target.detachedFromTarget", "params": {"sessionId": h, "targetId": h}}));
            }
        }
        self.handles = now;
    }
}

/// One input source's actions gathered from calls sent together, on one tab.
struct Group {
    ctx: Option<String>,
    /// The source, without its actions.
    source: Value,
    actions: Vec<Value>,
    /// The calls they came from.
    ids: Vec<u64>,
}

/// A CDP input call as a WebDriver input source (without its actions) and its actions. A touch
/// is the mouse: Safari on the Mac takes no touch input from WebDriver.
fn input_actions(method: &str, params: &Value, pointers: &mut Pointers) -> Option<(Value, Vec<Value>)> {
    let round = |v: &Value| v.as_f64().map(|f| f.round() as i64);
    let mouse = json!({"type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"}});
    match method {
        "Input.dispatchMouseEvent" => Some((mouse, pointer_actions(params["type"].as_str().unwrap_or_default(), round(&params["x"]), round(&params["y"]), &mut pointers.mouse))),
        "Input.dispatchTouchEvent" => {
            let at = &params["touchPoints"][0];
            Some((mouse, pointer_actions(params["type"].as_str().unwrap_or_default(), round(&at["x"]), round(&at["y"]), &mut pointers.mouse)))
        }
        "Input.dispatchKeyEvent" => {
            let key = params["key"].as_str().or_else(|| params["text"].as_str()).unwrap_or_default();
            let kind = if params["type"] == "keyUp" { "keyUp" } else { "keyDown" };
            Some((json!({"type": "key", "id": "keyboard"}), vec![json!({"type": kind, "value": webdriver_key(key)})]))
        }
        _ => None,
    }
}

/// The error of a command a dialog held up.
const HELD: &str = "unexpected alert open";

/// A CDP call's answer as the client reads it. One a dialog held up is no answer (Chrome's
/// comes once the dialog is closed): the dialog's news, before it, ends the wait.
fn answer(id: u64, answered: Answered) -> String {
    let answered = match answered {
        Err(e) if e.starts_with(HELD) => Ok(json!({})),
        a => a,
    };
    match answered {
        Ok(result) => json!({"id": id, "result": result}),
        Err(e) => json!({"id": id, "error": {"message": e}}),
    }
    .to_string()
}

impl<D: Driver> Transport for Classic<D> {
    fn send_text(&mut self, text: &str) -> Result<()> {
        self.send_texts(&[text])
    }

    fn send_texts(&mut self, texts: &[&str]) -> Result<()> {
        let mut answers = Vec::with_capacity(texts.len());
        // A run of one source's input (a click's move, press and release; a word's keys): one
        // sequence of actions, one command.
        let mut group: Option<Group> = None;
        let mut input = false;
        for t in texts {
            let m: Value = serde_json::from_str(t)?;
            let id = m["id"].as_u64().unwrap_or(0);
            let method = m["method"].as_str().unwrap_or_default();
            let ctx = m["sessionId"].as_str().map(String::from);
            if let Some((source, actions)) = input_actions(method, &m["params"], &mut self.pointers) {
                input = true;
                match &mut group {
                    Some(g) if g.ctx == ctx && g.source["id"] == source["id"] => {
                        g.actions.extend(actions);
                        g.ids.push(id);
                    }
                    _ => {
                        if let Some(g) = group.take() {
                            self.flush(g, &mut answers);
                        }
                        group = Some(Group { ctx, source, actions, ids: vec![id] });
                    }
                }
                continue;
            }
            if let Some(g) = group.take() {
                self.flush(g, &mut answers);
            }
            input |= method == "input.performActions";
            let answered = self.run(method, &m["params"], ctx.as_deref());
            answers.push(answer(id, answered));
        }
        if let Some(g) = group.take() {
            self.flush(g, &mut answers);
        }
        // After input, and for a while after (Safari tells of a tab a link opened a moment after
        // the click's answer): whether tabs came or went.
        if input {
            self.watch_until = Some(Instant::now() + TAB_WATCH);
        }
        if input || self.watch_until.is_some_and(|t| Instant::now() < t) {
            self.look_for_tabs();
        }
        // The events first: a browser tells of what a call did (a tab it opened, a dialog that
        // held it up) before it answers.
        for e in self.events.drain(..) {
            self.ready.push_back(e.to_string());
        }
        self.ready.extend(answers);
        Ok(())
    }

    fn recv_text(&mut self, _timeout: Duration) -> Result<Option<String>> {
        Ok(self.ready.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::Cdp;

    /// safaridriver's side: the commands sent, and how each is answered.
    struct Fake {
        sent: Vec<(String, String, Value)>,
        answer: Box<dyn FnMut(&str, &str, &Value) -> Result<Value, WdError>>,
    }

    impl Driver for Fake {
        fn command(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, WdError> {
            let body = body.cloned().unwrap_or(Value::Null);
            self.sent.push((method.to_string(), path.to_string(), body.clone()));
            (self.answer)(method, path, &body)
        }
    }

    fn fake(answer: impl FnMut(&str, &str, &Value) -> Result<Value, WdError> + 'static) -> Fake {
        Fake { sent: Vec::new(), answer: Box::new(answer) }
    }

    /// A CDP client on a fake safaridriver whose current window is `tab-1`.
    fn cdp(answer: impl FnMut(&str, &str, &Value) -> Result<Value, WdError> + 'static) -> Cdp<Classic<Fake>> {
        let mut c = Classic::new(fake(answer), "Safari/26.1");
        c.current = Some("tab-1".to_string());
        let mut c = Cdp::new(c);
        // Everything here is answered at the send: a call left unanswered is a failure.
        c.call_timeout = Duration::from_millis(50);
        c
    }

    #[test]
    fn tabs_are_windows() {
        let mut c = cdp(|m, p, _| match (m, p) {
            ("GET", "/window/handles") => Ok(json!(["tab-1", "tab-2"])),
            ("GET", "/window") => Ok(json!("tab-2")),
            ("GET", "/url") => Ok(json!("")),
            ("POST", "/window/new") => Ok(json!({"handle": "tab-3", "type": "tab"})),
            ("DELETE", "/window") => Ok(json!(["tab-1", "tab-2"])),
            _ => Ok(Value::Null),
        });
        // The tabs there: the current one's address asked (Safari's first is blank).
        let t = c.call(None, "Target.getTargets", json!({})).unwrap();
        assert_eq!(
            t,
            json!({"targetInfos": [{"targetId": "tab-1", "type": "page", "url": ""}, {"targetId": "tab-2", "type": "page", "url": "about:blank"}]})
        );
        assert_eq!(c.transport().current.as_deref(), Some("tab-2"));
        // A tab of its own: its window is its session.
        assert_eq!(c.call(None, "Target.createTarget", json!({"url": "about:blank"})).unwrap(), json!({"targetId": "tab-3"}));
        assert_eq!(sent(&c).last().unwrap().2, json!({"type": "tab"}));
        assert_eq!(c.call(None, "Target.attachToTarget", json!({"targetId": "tab-3", "flatten": true})).unwrap(), json!({"sessionId": "tab-3"}));
        // Closed from its own window, and gone: an event, as Chrome's is.
        assert_eq!(c.call(None, "Target.closeTarget", json!({"targetId": "tab-3"})).unwrap(), json!({"success": true}));
        let s = sent(&c);
        let n = s.len();
        assert_eq!((s[n - 2].1.as_str(), &s[n - 2].2), ("/window", &json!({"handle": "tab-3"})));
        assert_eq!((s[n - 1].0.as_str(), s[n - 1].1.as_str()), ("DELETE", "/window"));
        assert_eq!(c.transport().current, None, "no window is current once its window is closed");
        assert_eq!(c.next_event(Duration::ZERO).unwrap().unwrap(), json!({"method": "Target.detachedFromTarget", "params": {"sessionId": "tab-3", "targetId": "tab-3"}}));
    }

    #[test]
    fn what_needs_no_command_is_answered_here() {
        let mut c = cdp(|_, _, _| Ok(Value::Null));
        for m in ["Target.setDiscoverTargets", "Page.enable", "Network.enable", "Page.screencastFrameAck"] {
            assert_eq!(c.call(Some("tab-1"), m, json!({})).unwrap(), json!({}), "{m}");
        }
        assert_eq!(c.call(None, "Browser.getVersion", json!({})).unwrap(), json!({"product": "Safari/26.1", "userAgent": ""}));
        // The top frame alone: frames are not read across origins on Safari.
        assert_eq!(c.call(Some("tab-1"), "Page.getFrameTree", json!({})).unwrap(), json!({"frameTree": {"frame": {"id": "tab-1", "url": ""}, "childFrames": []}}));
        assert!(sent(&c).is_empty());
    }

    /// The calls `texts` sent together, all answered: their results.
    fn batch(c: &mut Cdp<Classic<Fake>>, calls: &[(&str, Value)]) -> Value {
        match c.calls(Some("tab-1"), calls, &|_| false).unwrap() {
            crate::chrome::Answer::Reply(v) | crate::chrome::Answer::Stopped(v) => v,
        }
    }

    fn handles(list: &'static [&'static str]) -> impl FnMut(&str, &str, &Value) -> Result<Value, WdError> {
        move |m, p, _| match (m, p) {
            ("GET", "/window/handles") => Ok(json!(list)),
            _ => Ok(Value::Null),
        }
    }

    #[test]
    fn a_click_is_one_sequence_of_a_mouse_pointers_actions() {
        let mut c = cdp(handles(&["tab-1"]));
        let m = "Input.dispatchMouseEvent";
        let r = batch(
            &mut c,
            &[(m, json!({"type": "mouseMoved", "x": 10.4, "y": 20.6})), (m, json!({"type": "mousePressed", "x": 10, "y": 21, "clickCount": 1})), (m, json!({"type": "mouseReleased", "x": 10, "y": 21}))],
        );
        assert_eq!(r, json!({}));
        let s = sent(&c);
        assert_eq!((s[0].0.as_str(), s[0].1.as_str()), ("POST", "/actions"));
        assert_eq!(
            s[0].2,
            json!({"actions": [{"type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"}, "actions": [
                {"type": "pointerMove", "x": 10, "y": 21, "duration": 0},
                {"type": "pointerDown", "button": 0},
                {"type": "pointerUp", "button": 0},
            ]}]})
        );
        // Then whether input opened (or closed) a tab.
        assert_eq!(s[1].1, "/window/handles");
        assert_eq!(s.len(), 2, "{s:?}");
    }

    #[test]
    fn a_touch_is_the_mouse_on_a_mac() {
        // Safari on the Mac takes no touch input from WebDriver: a finger is the mouse.
        let mut c = cdp(handles(&["tab-1"]));
        let t = "Input.dispatchTouchEvent";
        batch(&mut c, &[(t, json!({"type": "touchStart", "touchPoints": [{"x": 5, "y": 6}]})), (t, json!({"type": "touchMove", "touchPoints": [{"x": 5, "y": 60}]})), (t, json!({"type": "touchEnd", "touchPoints": []}))]);
        let s = sent(&c);
        let source = &s[0].2["actions"][0];
        assert_eq!(source["parameters"]["pointerType"], json!("mouse"));
        assert_eq!(source["actions"].as_array().unwrap().len(), 4, "move, down, move, up: {source}");
    }

    #[test]
    fn typing_is_one_sequence_of_key_actions() {
        let mut c = cdp(handles(&["tab-1"]));
        let k = "Input.dispatchKeyEvent";
        batch(&mut c, &[(k, json!({"type": "keyDown", "key": "a", "text": "a"})), (k, json!({"type": "keyUp", "key": "a"})), (k, json!({"type": "keyDown", "key": "Enter", "text": "\r"})), (k, json!({"type": "keyUp", "key": "Enter"}))]);
        let s = sent(&c);
        assert_eq!(
            s[0].2,
            json!({"actions": [{"type": "key", "id": "keyboard", "actions": [
                {"type": "keyDown", "value": "a"}, {"type": "keyUp", "value": "a"},
                {"type": "keyDown", "value": "\u{E007}"}, {"type": "keyUp", "value": "\u{E007}"},
            ]}]})
        );
    }

    #[test]
    fn the_mouse_and_the_keyboard_go_in_turn() {
        let mut c = cdp(handles(&["tab-1"]));
        batch(&mut c, &[("Input.dispatchMouseEvent", json!({"type": "mousePressed", "x": 1, "y": 1})), ("Input.dispatchKeyEvent", json!({"type": "keyDown", "key": "b"})), ("Input.dispatchMouseEvent", json!({"type": "mouseReleased", "x": 1, "y": 1}))]);
        let ids: Vec<Value> = sent(&c).iter().filter(|s| s.1 == "/actions").map(|s| s.2["actions"][0]["id"].clone()).collect();
        assert_eq!(ids, [json!("mouse"), json!("keyboard"), json!("mouse")]);
    }

    #[test]
    fn webdriver_actions_sent_as_they_are_go_to_their_tab() {
        let mut c = cdp(handles(&["tab-1", "tab-2"]));
        let actions = json!([{"type": "pointer", "id": "mouse", "actions": [{"type": "pointerDown", "button": 0}]}]);
        c.call(None, "input.performActions", json!({"context": "tab-2", "actions": actions})).unwrap();
        let s = sent(&c);
        assert_eq!(s[0].2, json!({"handle": "tab-2"}));
        assert_eq!((s[1].1.as_str(), &s[1].2), ("/actions", &json!({"actions": actions})));
    }

    #[test]
    fn a_tab_input_opened_or_closed_is_told_of_before_the_inputs_answer() {
        let mut c = cdp(handles(&["tab-1", "tab-9"]));
        c.transport_mut().handles = vec!["tab-1".into(), "tab-2".into()];
        let m = "Input.dispatchMouseEvent";
        batch(&mut c, &[(m, json!({"type": "mousePressed", "x": 1, "y": 1})), (m, json!({"type": "mouseReleased", "x": 1, "y": 1}))]);
        let made = c.queued_event().unwrap();
        assert_eq!(made, json!({"method": "Target.targetCreated", "params": {"targetInfo": {"targetId": "tab-9", "type": "page", "url": ""}}}));
        let gone = c.queued_event().unwrap();
        assert_eq!(gone, json!({"method": "Target.detachedFromTarget", "params": {"sessionId": "tab-2", "targetId": "tab-2"}}));
        assert_eq!(c.transport().handles, ["tab-1", "tab-9"]);
    }

    /// safaridriver with a dialog open: every command but the dialog's own held up by it.
    fn with_dialog(kind: &'static str) -> impl FnMut(&str, &str, &Value) -> Result<Value, WdError> {
        let mut open = true;
        move |m, p, _| match (m, p) {
            ("GET", "/alert/text") if open => Ok(json!("Sure?")),
            // Text goes to a prompt alone.
            ("POST", "/alert/text") if open && kind == "prompt" => Ok(Value::Null),
            ("POST", "/alert/text") if open => Err(WdError::new("element not interactable", "")),
            ("POST", "/alert/accept" | "/alert/dismiss") if open => {
                open = false;
                Ok(Value::Null)
            }
            (_, "/alert/text" | "/alert/accept" | "/alert/dismiss") => Err(WdError::new("no such alert", "")),
            _ if open => Err(WdError::new("unexpected alert open", "")),
            _ => Ok(json!({"value": 2})),
        }
    }

    #[test]
    fn a_dialog_that_holds_a_call_up_is_told_of_and_ends_its_wait() {
        let mut c = cdp(with_dialog("confirm"));
        let opening = |e: &Value| e["method"] == "Page.javascriptDialogOpening";
        let a = c.call_until(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1"}), &opening).unwrap();
        let crate::chrome::Answer::Stopped(e) = a else { panic!("the call's wait went on") };
        // An alert or a confirm (WebDriver tells them apart no more than that): two buttons.
        assert_eq!(e, json!({"method": "Page.javascriptDialogOpening", "sessionId": "tab-1", "params": {"type": "confirm", "message": "Sure?", "defaultPrompt": ""}}));
        // Told of once.
        let a = c.call_until(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1"}), &opening).unwrap();
        assert!(matches!(a, crate::chrome::Answer::Reply(_)));
        // Answered: the page runs again.
        c.call(Some("tab-1"), "Page.handleJavaScriptDialog", json!({"accept": false, "promptText": ""})).unwrap();
        assert!(sent(&c).iter().any(|s| s.1 == "/alert/dismiss"));
        assert_eq!(c.next_event(Duration::ZERO).unwrap().unwrap()["method"], json!("Page.javascriptDialogClosed"));
        assert_eq!(c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1", "returnByValue": true})).unwrap(), json!({"result": {"value": 2}}));
    }

    #[test]
    fn a_prompt_is_one_that_takes_text_and_its_answer_is_typed_in_it() {
        let mut c = cdp(with_dialog("prompt"));
        let m = "Input.dispatchMouseEvent";
        batch(&mut c, &[(m, json!({"type": "mousePressed", "x": 1, "y": 1})), (m, json!({"type": "mouseReleased", "x": 1, "y": 1}))]);
        let e = c.queued_event().unwrap();
        assert_eq!(e["params"]["type"], json!("prompt"));
        c.call(Some("tab-1"), "Page.handleJavaScriptDialog", json!({"accept": true, "promptText": "Ann"})).unwrap();
        // (Input being recent, each call is followed by a look for tabs.)
        let s: Vec<_> = sent(&c).into_iter().filter(|s| s.1 != "/window/handles").collect();
        let n = s.len();
        assert_eq!((s[n - 2].1.as_str(), &s[n - 2].2), ("/alert/text", &json!({"text": "Ann"})));
        assert_eq!(s[n - 1].1, "/alert/accept");
    }

    #[test]
    fn a_dialog_that_held_a_navigation_up_ends_it_unloaded() {
        let mut c = cdp(with_dialog("alert"));
        let r = c.call_until(Some("tab-1"), "Page.navigate", json!({"url": "http://x/"}), &|_| false).unwrap();
        let crate::chrome::Answer::Reply(r) = r else { panic!() };
        assert_eq!(r, json!({"frameId": ""}));
        assert_eq!(c.queued_event().unwrap()["method"], json!("Page.javascriptDialogOpening"));
    }

    #[test]
    fn a_screenshot_is_the_windows() {
        let mut c = cdp(|_, _, _| Ok(json!("iVBOR")));
        assert_eq!(c.call(Some("tab-1"), "Page.captureScreenshot", json!({"format": "png"})).unwrap(), json!({"data": "iVBOR"}));
        assert_eq!(sent(&c)[0], ("GET".to_string(), "/screenshot".to_string(), Value::Null));
    }

    #[test]
    fn a_sites_cookies_are_deleted_from_its_own_page() {
        let mut c = cdp(|_, _, _| Ok(Value::Null));
        c.call(Some("tab-1"), "Storage.clearDataForOrigin", json!({"origin": "http://x", "storageTypes": "all"})).unwrap();
        assert_eq!(sent(&c)[0], ("DELETE".to_string(), "/cookie".to_string(), Value::Null));
    }

    #[test]
    fn a_viewport_is_the_window_sized_around_it() {
        // The window's size is set: as much bigger than the viewport asked for as it is now.
        let mut c = cdp(|_, p, _| Ok(if p == "/execute/sync" { json!([0, 92]) } else { Value::Null }));
        c.call(Some("tab-1"), "Emulation.setDeviceMetricsOverride", json!({"width": 800, "height": 1280, "deviceScaleFactor": 0, "mobile": false})).unwrap();
        let s = sent(&c);
        assert_eq!((s[1].0.as_str(), s[1].1.as_str(), &s[1].2), ("POST", "/window/rect", &json!({"width": 800, "height": 1372})));
    }

    #[test]
    fn what_safari_cannot_do_says_so() {
        let mut c = cdp(|_, _, _| Ok(Value::Null));
        let cases = [
            ("Emulation.setDeviceMetricsOverride", json!({"width": 412, "height": 839, "deviceScaleFactor": 2.625, "mobile": true}), "phone"),
            ("Emulation.setTouchEmulationEnabled", json!({"enabled": true}), "phone"),
            ("Emulation.setUserAgentOverride", json!({"userAgent": "UA"}), "user agent"),
            ("Emulation.setEmulatedMedia", json!({}), "dark mode"),
            ("Emulation.setGeolocationOverride", json!({"latitude": 1.0}), "location"),
            ("Browser.setPermission", json!({"permission": {"name": "geolocation"}}), "permissions"),
            ("Network.emulateNetworkConditions", json!({"offline": true}), "airplane mode"),
            ("Page.startScreencast", json!({}), "recording"),
            ("Page.stopScreencast", json!({}), "recording"),
            ("Fetch.enable", json!({}), "Fetch.enable"),
        ];
        for (m, p, says) in cases {
            let e = c.call(Some("tab-1"), m, p).unwrap_err().to_string();
            assert!(e.contains(says) && e.contains("Safari"), "{m}: {e}");
        }
        assert!(sent(&c).is_empty());
    }

    #[test]
    fn a_tab_input_opens_a_moment_later_is_told_of_at_a_call_soon_after() {
        // Safari tells of a tab a link or window.open made a moment after the click's answer:
        // for a while after input, calls look for tabs too.
        let mut c = cdp(|m, p, _| match (m, p) {
            ("GET", "/window/handles") => Ok(json!(["tab-1", "tab-5"])),
            _ => Ok(json!({"value": 1})),
        });
        c.transport_mut().handles = vec!["tab-1".into()];
        c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
        assert!(!sent(&c).iter().any(|s| s.1 == "/window/handles"), "no input yet: no look");
        c.transport_mut().watch_until = Some(std::time::Instant::now() + Duration::from_secs(5));
        c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
        assert_eq!(c.queued_event().unwrap()["method"], json!("Target.targetCreated"));
        // Input starts the while.
        c.transport_mut().watch_until = None;
        c.call(Some("tab-1"), "Input.dispatchKeyEvent", json!({"type": "keyDown", "key": "a"})).unwrap();
        assert!(c.transport().watch_until.is_some_and(|t| t > std::time::Instant::now() + Duration::from_secs(4)));
        // Once over, calls look no more.
        c.transport_mut().watch_until = Some(std::time::Instant::now() - Duration::from_millis(1));
        let n = sent(&c).len();
        c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
        assert_eq!(sent(&c).len(), n + 1);
    }

    #[test]
    fn a_tab_that_closed_itself_is_no_longer_the_current_window() {
        let mut c = cdp(handles(&["tab-1"]));
        c.transport_mut().handles = vec!["tab-1".into(), "tab-2".into()];
        c.transport_mut().current = Some("tab-2".into());
        c.call(None, "input.performActions", json!({"context": "tab-2", "actions": []})).unwrap();
        assert_eq!(c.transport().current, None);
        // The next call on a tab switches to its window.
        c.call(Some("tab-1"), "Page.captureScreenshot", json!({})).unwrap();
        assert_eq!(sent(&c)[2], ("POST".to_string(), "/window".to_string(), json!({"handle": "tab-1"})));
    }

    #[test]
    fn input_and_a_call_after_it_go_in_turn_and_a_call_posted_is_answered_too() {
        let mut c = cdp(handles(&["tab-1"]));
        batch(&mut c, &[("Input.dispatchKeyEvent", json!({"type": "keyDown", "key": "a"})), ("Runtime.evaluate", json!({"expression": "1"}))]);
        let paths: Vec<String> = sent(&c).iter().map(|s| s.1.clone()).collect();
        assert_eq!(paths, ["/actions", "/execute/async", "/window/handles"]);
        c.post(Some("tab-1"), "Page.captureScreenshot", json!({})).unwrap();
        // (Then, input being recent, a look for tabs.)
        let s = sent(&c);
        assert_eq!(s[s.len() - 2].1, "/screenshot");
    }

    #[test]
    fn a_call_on_another_tab_switches_to_its_window_first() {
        let mut c = cdp(|_, _, _| Ok(json!({"value": 1})));
        c.call(Some("tab-2"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
        c.call(Some("tab-2"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
        let s = sent(&c);
        assert_eq!((s[0].0.as_str(), s[0].1.as_str(), &s[0].2), ("POST", "/window", &json!({"handle": "tab-2"})));
        assert_eq!(s.len(), 3, "switched once: {s:?}");
        assert_eq!(c.transport().current.as_deref(), Some("tab-2"));
    }

    #[test]
    fn a_navigation_answers_once_loaded_and_its_load_is_an_event() {
        let mut c = cdp(|_, _, _| Ok(Value::Null));
        let r = c.call(Some("tab-1"), "Page.navigate", json!({"url": "http://x/"})).unwrap();
        assert_eq!(r["loaderId"].as_str().map(|s| !s.is_empty()), Some(true), "{r}");
        assert_eq!(sent(&c), [("POST".to_string(), "/url".to_string(), json!({"url": "http://x/"}))]);
        let load = c.next_event(Duration::ZERO).unwrap().unwrap();
        assert_eq!(load, json!({"method": "Page.loadEventFired", "sessionId": "tab-1", "params": {}}));
    }

    #[test]
    fn a_navigation_that_fails_says_why_in_its_error_text() {
        let mut c = cdp(|_, _, _| Err(WdError::new("unknown error", "Could not connect to the server.")));
        let r = c.call(Some("tab-1"), "Page.navigate", json!({"url": "http://127.0.0.1:1/"})).unwrap();
        assert_eq!(r, json!({"frameId": "", "errorText": "unknown error: Could not connect to the server."}));
        assert_eq!(c.next_event(Duration::ZERO).unwrap(), None);
    }

    fn sent(c: &Cdp<Classic<Fake>>) -> Vec<(String, String, Value)> {
        c.transport().driver.sent.clone()
    }

    #[test]
    fn a_page_script_is_run_in_the_page_and_its_value_comes_back() {
        let mut c = cdp(|_, _, _| Ok(json!({"value": {"state": [3, true]}})));
        let r = c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "f()", "returnByValue": true, "awaitPromise": true})).unwrap();
        assert_eq!(r, json!({"result": {"value": {"state": [3, true]}}}));
        let s = sent(&c);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!((s[0].0.as_str(), s[0].1.as_str()), ("POST", "/execute/async"));
        // The expression, no script to install first, awaited, by value.
        assert_eq!(s[0].2["args"], json!(["f()", null, true, true]));
        assert!(s[0].2["script"].as_str().unwrap().contains("eval"));
    }

    #[test]
    fn a_script_that_throws_comes_back_as_cdp_exception_details() {
        let mut c = cdp(|_, _, _| Ok(json!({"thrown": "Error: boom"})));
        let r = c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "boom()"})).unwrap();
        assert_eq!(r, json!({"exceptionDetails": {"text": "Error: boom"}}));
        // As a script safaridriver could not run is.
        let mut c = cdp(|_, _, _| Err(WdError::new("javascript error", "SyntaxError: Unexpected token")));
        let r = c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "{"})).unwrap();
        assert_eq!(r, json!({"exceptionDetails": {"text": "javascript error: SyntaxError: Unexpected token"}}));
    }

    #[test]
    fn scripts_to_run_in_every_document_are_run_before_a_script_in_one_that_has_not_had_them() {
        // Classic WebDriver has no preload scripts: each script sent carries them, and the page
        // runs them first, once a document.
        let mut c = cdp(|_, _, _| Ok(json!({"value": null})));
        assert_eq!(c.call(Some("tab-1"), "Page.addScriptToEvaluateOnNewDocument", json!({"source": "track()"})).unwrap(), json!({"identifier": "1"}));
        // The same script again is the one already kept.
        c.call(Some("tab-1"), "Page.addScriptToEvaluateOnNewDocument", json!({"source": "track()"})).unwrap();
        c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
        let s = sent(&c);
        assert_eq!(s.len(), 1, "kept here, nothing sent for them: {s:?}");
        assert_eq!(s[0].2["args"], json!(["1", ["track()"], false, false]));
    }

    #[test]
    fn there_are_no_isolated_worlds() {
        let mut c = cdp(|_, _, _| Ok(Value::Null));
        let e = c.call(Some("tab-1"), "Page.createIsolatedWorld", json!({"frameId": "f", "worldName": "virtuoso"})).unwrap_err().to_string();
        assert!(e.contains("not on Safari"), "{e}");
        let e = c.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "1", "contextId": 3})).unwrap_err().to_string();
        assert!(e.contains("not on Safari"), "{e}");
        assert!(sent(&c).is_empty());
    }
}
