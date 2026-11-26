//! WebDriver classic under the DevTools Protocol client, for Safari (safaridriver speaks
//! classic WebDriver over HTTP, and only part of BiDi): each CDP call the web device makes is
//! run here as safaridriver's commands, and answered in CDP's shape. Classic has no events: the
//! ones the device follows are made here from what the commands find (a load at a navigation's
//! end, a dialog at a command it held up, a tab come or gone after input). A tab's window handle
//! stands for both CDP's target and its session.

use crate::chrome::Transport;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;

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
    handles: Vec<String>,
}

/// What a CDP call is answered with: its result, or an error's message.
type Answered = std::result::Result<Value, String>;

impl<D: Driver> Classic<D> {
    pub fn new(driver: D, product: &str) -> Classic<D> {
        Classic { driver, ready: VecDeque::new(), current: None, preload: Vec::new(), events: Vec::new(), navigations: 0, product: product.to_string(), handles: Vec::new() }
    }

    /// A command on the tab `ctx`: its window made the current one first, if it is not.
    fn on(&mut self, ctx: Option<&str>, method: &str, path: &str, body: Option<&Value>) -> Result<Value, WdError> {
        if let Some(ctx) = ctx.filter(|c| !c.is_empty() && self.current.as_deref() != Some(*c)) {
            self.driver.command("POST", "/window", Some(&json!({"handle": ctx})))?;
            self.current = Some(ctx.to_string());
        }
        self.driver.command(method, path, body)
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
            // The top frame alone: frames from other origins are not read on Safari.
            "Page.getFrameTree" => Ok(json!({"frameTree": {"frame": {"id": ctx, "url": ""}, "childFrames": []}})),
            "Page.navigate" => match self.on(ctx, "POST", "/url", Some(&json!({"url": params["url"]}))) {
                Ok(_) => {
                    // Classic WebDriver answers a navigation once its page has loaded.
                    self.events.push(json!({"method": "Page.loadEventFired", "sessionId": ctx, "params": {}}));
                    self.navigations += 1;
                    Ok(json!({"frameId": "", "loaderId": self.navigations.to_string()}))
                }
                Err(e) => Ok(json!({"frameId": "", "errorText": e.to_string()})),
            },
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
            other => Err(format!("{other} has no WebDriver classic counterpart here")),
        }
    }

}

/// A CDP call's answer as the client reads it.
fn answer(id: u64, answered: Answered) -> String {
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
        for t in texts {
            let m: Value = serde_json::from_str(t)?;
            let id = m["id"].as_u64().unwrap_or(0);
            let answered = self.run(m["method"].as_str().unwrap_or_default(), &m["params"], m["sessionId"].as_str());
            answers.push(answer(id, answered));
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
