//! WebDriver BiDi under the DevTools Protocol client: each CDP call the web device makes goes to
//! the browser as its BiDi command, and BiDi's replies and events come back in CDP's shape. The
//! web device's behaviour (its settle, screen reads, taps, typing, dialogs, tabs) is then the
//! same on Firefox as on Chrome. A tab's browsing context stands for both CDP's target and its
//! session; an isolated world is a BiDi sandbox in a frame.

use crate::chrome::Transport;
use anyhow::Result;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// The events the device follows: tabs coming and going, pages loading, dialogs.
pub const EVENTS: &[&str] = &[
    "browsingContext.contextCreated",
    "browsingContext.contextDestroyed",
    "browsingContext.load",
    "browsingContext.fragmentNavigated",
    "browsingContext.userPromptOpened",
    "browsingContext.userPromptClosed",
];

/// How a BiDi reply becomes the CDP call's result.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// A BiDi command sent as it is (`session.new`): its result as it is.
    Raw,
    /// Nothing the caller reads: `{}`.
    Empty,
    /// browsingContext.create's context as CDP's `targetId`.
    Target,
    /// browsingContext.getTree's tabs as `Target.getTargets`' `targetInfos`.
    Targets,
    /// script.evaluate's result as `Runtime.evaluate`'s (`result.value` or `exceptionDetails`).
    Evaluated,
    /// browsingContext.navigate's answer as `Page.navigate`'s (`loaderId`, or `errorText`).
    Navigated,
    /// A screenshot's `data`.
    Data,
    /// browsingContext.getTree from one context as `Page.getFrameTree`'s `frameTree`.
    FrameTree,
}

/// What a CDP call becomes: answered here, or a BiDi command whose reply is turned back.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Local(Value),
    Command(String, Value, Reply),
}

/// The key WebDriver sends for a key CDP names (`Enter`, `ArrowUp`): its code point in
/// WebDriver's private range, or the character itself.
pub fn webdriver_key(key: &str) -> String {
    let special = match key {
        "Enter" | "\r" | "\n" => '\u{E007}',
        "Backspace" => '\u{E003}',
        "Tab" => '\u{E004}',
        "Escape" => '\u{E00C}',
        "ArrowLeft" => '\u{E012}',
        "ArrowUp" => '\u{E013}',
        "ArrowRight" => '\u{E014}',
        "ArrowDown" => '\u{E015}',
        "Delete" => '\u{E017}',
        _ => return key.to_string(),
    };
    special.to_string()
}

/// A BiDi value (script.evaluate's serialization) as the JSON CDP's `returnByValue` gives.
pub fn remote_to_json(v: &Value) -> Value {
    let inner = &v["value"];
    match v["type"].as_str().unwrap_or_default() {
        "string" | "boolean" => inner.clone(),
        "number" => match inner {
            Value::Number(_) => inner.clone(),
            Value::String(s) if s == "-0" => json!(0),
            // NaN and the infinities have no JSON: null, as JSON.stringify makes them.
            _ => Value::Null,
        },
        "bigint" => inner.as_str().and_then(|s| s.parse::<i64>().ok()).map(Value::from).unwrap_or(Value::Null),
        "array" | "set" => Value::Array(inner.as_array().map(|a| a.iter().map(remote_to_json).collect()).unwrap_or_default()),
        "object" | "map" => {
            let mut m = Map::new();
            for pair in inner.as_array().into_iter().flatten() {
                let key = match &pair[0] {
                    Value::String(s) => s.clone(),
                    k => match remote_to_json(k) {
                        Value::String(s) => s,
                        other => other.to_string(),
                    },
                };
                m.insert(key, remote_to_json(&pair[1]));
            }
            Value::Object(m)
        }
        "date" => inner.clone(),
        // undefined, null, and what has no JSON (a node, a window, a function): null.
        _ => Value::Null,
    }
}

/// A pointer's actions for one CDP mouse or touch event.
fn pointer_actions(kind: &str, x: Option<i64>, y: Option<i64>) -> Vec<Value> {
    let mv = || json!({"type": "pointerMove", "x": x.unwrap_or(0), "y": y.unwrap_or(0), "duration": 0});
    match kind {
        "mouseMoved" | "touchMove" => vec![mv()],
        "mousePressed" | "touchStart" => vec![mv(), json!({"type": "pointerDown", "button": 0})],
        "mouseReleased" => vec![mv(), json!({"type": "pointerUp", "button": 0})],
        // A touch lifted (or cancelled: BiDi has no cancel) where it is.
        _ => vec![json!({"type": "pointerUp", "button": 0})],
    }
}

fn round(v: &Value) -> Option<i64> {
    v.as_f64().map(|f| f.round() as i64)
}

/// The BiDi for CDP's `method` with `params` on the tab `session` (a browsing context), or an
/// error naming what Firefox cannot do. `worlds` holds the isolated worlds made so far.
pub fn plan(method: &str, params: &Value, session: Option<&str>, worlds: &mut HashMap<i64, (String, String)>) -> Result<Plan, String> {
    let ctx = session.unwrap_or_default();
    let cmd = |m: &str, p: Value, r: Reply| Ok(Plan::Command(m.to_string(), p, r));
    // BiDi's own commands (a lower-case module: `session.new`) go as they are.
    if method.split('.').next().is_some_and(|m| m.chars().next().is_some_and(|c| c.is_ascii_lowercase())) && method.contains('.') {
        return cmd(method, params.clone(), Reply::Raw);
    }
    match method {
        // Subscribed to at the start; a tab's session is its context.
        "Target.setDiscoverTargets" | "Page.enable" | "Network.enable" | "Page.screencastFrameAck" | "Emulation.setTouchEmulationEnabled" => Ok(Plan::Local(json!({}))),
        "Target.attachToTarget" => Ok(Plan::Local(json!({"sessionId": params["targetId"]}))),
        "Target.getTargets" => cmd("browsingContext.getTree", json!({"maxDepth": 0}), Reply::Targets),
        "Target.createTarget" => cmd("browsingContext.create", json!({"type": "tab", "background": true}), Reply::Target),
        "Target.closeTarget" => cmd("browsingContext.close", json!({"context": params["targetId"]}), Reply::Empty),
        "Browser.getVersion" => Ok(Plan::Local(json!({"product": "Firefox", "userAgent": ""}))),
        "Page.addScriptToEvaluateOnNewDocument" => {
            let source = params["source"].as_str().unwrap_or_default();
            cmd("script.addPreloadScript", json!({"functionDeclaration": format!("() => {{\n{source}\n}}"), "contexts": [ctx]}), Reply::Empty)
        }
        "Runtime.evaluate" => {
            let target = match params["contextId"].as_i64() {
                Some(id) => match worlds.get(&id) {
                    Some((frame, name)) => json!({"context": frame, "sandbox": name}),
                    None => return Err(format!("no isolated world {id}")),
                },
                None => json!({"context": ctx}),
            };
            let p = json!({
                "expression": params["expression"],
                "target": target,
                "awaitPromise": params["awaitPromise"].as_bool().unwrap_or(false),
                "resultOwnership": "none",
                "serializationOptions": {"maxDomDepth": 0},
            });
            cmd("script.evaluate", p, Reply::Evaluated)
        }
        "Page.navigate" => cmd("browsingContext.navigate", json!({"context": ctx, "url": params["url"], "wait": "none"}), Reply::Navigated),
        "Page.captureScreenshot" => cmd("browsingContext.captureScreenshot", json!({"context": ctx}), Reply::Data),
        "Page.getFrameTree" => cmd("browsingContext.getTree", json!({"root": ctx}), Reply::FrameTree),
        "Page.createIsolatedWorld" => {
            let id = worlds.len() as i64 + 1;
            worlds.insert(id, (params["frameId"].as_str().unwrap_or_default().to_string(), params["worldName"].as_str().unwrap_or("virtuoso").to_string()));
            Ok(Plan::Local(json!({"executionContextId": id})))
        }
        "Page.handleJavaScriptDialog" => {
            let mut p = json!({"context": ctx, "accept": params["accept"].as_bool().unwrap_or(true)});
            if let Some(t) = params["promptText"].as_str() {
                p["userText"] = json!(t);
            }
            cmd("browsingContext.handleUserPrompt", p, Reply::Empty)
        }
        "Input.dispatchMouseEvent" => {
            let actions = pointer_actions(params["type"].as_str().unwrap_or_default(), round(&params["x"]), round(&params["y"]));
            cmd("input.performActions", json!({"context": ctx, "actions": [{"type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"}, "actions": actions}]}), Reply::Empty)
        }
        "Input.dispatchTouchEvent" => {
            let at = &params["touchPoints"][0];
            let actions = pointer_actions(params["type"].as_str().unwrap_or_default(), round(&at["x"]), round(&at["y"]));
            cmd("input.performActions", json!({"context": ctx, "actions": [{"type": "pointer", "id": "touch", "parameters": {"pointerType": "touch"}, "actions": actions}]}), Reply::Empty)
        }
        "Input.dispatchKeyEvent" => {
            let key = params["key"].as_str().or_else(|| params["text"].as_str()).unwrap_or_default();
            let kind = if params["type"] == "keyUp" { "keyUp" } else { "keyDown" };
            cmd("input.performActions", json!({"context": ctx, "actions": [{"type": "key", "id": "keyboard", "actions": [{"type": kind, "value": webdriver_key(key)}]}]}), Reply::Empty)
        }
        "Emulation.setDeviceMetricsOverride" => {
            let scale = params["deviceScaleFactor"].as_f64().filter(|s| *s > 0.0);
            cmd("browsingContext.setViewport", json!({"context": ctx, "viewport": {"width": params["width"], "height": params["height"]}, "devicePixelRatio": scale}), Reply::Empty)
        }
        "Emulation.setUserAgentOverride" => cmd("emulation.setUserAgentOverride", json!({"userAgent": params["userAgent"], "contexts": [ctx]}), Reply::Empty),
        "Emulation.setGeolocationOverride" => cmd(
            "emulation.setGeolocationOverride",
            json!({"coordinates": {"latitude": params["latitude"], "longitude": params["longitude"], "accuracy": params["accuracy"]}, "contexts": [ctx]}),
            Reply::Empty,
        ),
        "Network.emulateNetworkConditions" => {
            let conditions = if params["offline"] == true { json!({"type": "offline"}) } else { Value::Null };
            cmd("emulation.setNetworkConditions", json!({"networkConditions": conditions, "contexts": [ctx]}), Reply::Empty)
        }
        "Browser.setPermission" => cmd(
            "permissions.setPermission",
            json!({"descriptor": {"name": params["permission"]["name"]}, "state": params["setting"], "origin": params["origin"]}),
            Reply::Empty,
        ),
        "Storage.clearDataForOrigin" => cmd("storage.deleteCookies", json!({"partition": {"type": "storageKey", "sourceOrigin": params["origin"]}}), Reply::Empty),
        "Emulation.setEmulatedMedia" => Err("dark mode is not on Firefox yet".to_string()),
        "Page.startScreencast" | "Page.stopScreencast" => Err("recording is not on Firefox yet".to_string()),
        other => Err(format!("{other} has no WebDriver BiDi counterpart here")),
    }
}

/// A BiDi reply's result as CDP's, by how the call was planned.
pub fn cdp_result(reply: Reply, mut result: Value) -> Value {
    match reply {
        Reply::Raw => result,
        Reply::Empty => json!({}),
        Reply::Target => json!({"targetId": result["context"]}),
        Reply::Targets => {
            let infos: Vec<Value> = result["contexts"].as_array().into_iter().flatten().map(|c| json!({"targetId": c["context"], "type": "page", "url": c["url"]})).collect();
            json!({"targetInfos": infos})
        }
        Reply::Evaluated => {
            if result["type"] == "exception" {
                let d = &result["exceptionDetails"];
                let text = d["exception"]["value"].as_str().map(String::from).or_else(|| d["text"].as_str().map(String::from)).unwrap_or_else(|| "an exception".to_string());
                return json!({"exceptionDetails": {"text": text}});
            }
            json!({"result": {"value": remote_to_json(&result["result"])}})
        }
        Reply::Navigated => match result["navigation"].as_str() {
            Some(n) => json!({"frameId": "", "loaderId": n}),
            None => json!({"frameId": ""}),
        },
        Reply::Data => json!({"data": result["data"].take()}),
        Reply::FrameTree => {
            fn frame(c: &Value) -> Value {
                let children: Vec<Value> = c["children"].as_array().into_iter().flatten().map(frame).collect();
                json!({"frame": {"id": c["context"], "url": c["url"]}, "childFrames": children})
            }
            json!({"frameTree": frame(&result["contexts"][0])})
        }
    }
}

/// A BiDi error's message as CDP's: a navigation's error is its result's `errorText` (an aborted
/// one as Chrome names it), any other an error.
pub fn cdp_error(reply: &Reply, error: &str, message: &str) -> Result<Value, String> {
    if *reply == Reply::Navigated {
        let text = if message.contains("NS_BINDING_ABORTED") { "net::ERR_ABORTED".to_string() } else { format!("{error}: {message}") };
        return Ok(json!({"frameId": "", "errorText": text}));
    }
    Err(format!("{error}: {message}"))
}

/// A BiDi event as the CDP event the device follows, if it follows it.
pub fn cdp_event(e: &Value) -> Option<Value> {
    let p = &e["params"];
    let ctx = p["context"].clone();
    let top = p["parent"].is_null();
    match e["method"].as_str()? {
        "browsingContext.load" | "browsingContext.fragmentNavigated" => Some(json!({"method": "Page.loadEventFired", "sessionId": ctx, "params": {}})),
        "browsingContext.userPromptOpened" => Some(json!({
            "method": "Page.javascriptDialogOpening",
            "sessionId": ctx,
            "params": {"type": p["type"], "message": p["message"], "defaultPrompt": p["defaultValue"]},
        })),
        "browsingContext.userPromptClosed" => Some(json!({"method": "Page.javascriptDialogClosed", "sessionId": ctx, "params": {"result": p["accepted"]}})),
        "browsingContext.contextCreated" if top => Some(json!({"method": "Target.targetCreated", "params": {"targetInfo": {"targetId": ctx, "type": "page", "url": p["url"]}}})),
        "browsingContext.contextDestroyed" if top => Some(json!({"method": "Target.detachedFromTarget", "params": {"sessionId": ctx, "targetId": ctx}})),
        _ => None,
    }
}

/// The CDP client's transport over a BiDi one (see the module).
pub struct Bidi<T: Transport> {
    inner: T,
    /// The replies made here, and the translated ones, for the client to read.
    ready: VecDeque<String>,
    /// The calls sent, by id: how each reply becomes CDP's.
    pending: HashMap<u64, Reply>,
    worlds: HashMap<i64, (String, String)>,
}

impl<T: Transport> Bidi<T> {
    pub fn new(inner: T) -> Bidi<T> {
        Bidi { inner, ready: VecDeque::new(), pending: HashMap::new(), worlds: HashMap::new() }
    }

    /// A CDP message to the browser: its BiDi sent, or its answer made here.
    fn send_one(&mut self, text: &str, out: &mut Vec<String>) -> Result<()> {
        let m: Value = serde_json::from_str(text)?;
        let id = m["id"].as_u64().unwrap_or(0);
        match plan(m["method"].as_str().unwrap_or_default(), &m["params"], m["sessionId"].as_str(), &mut self.worlds) {
            Ok(Plan::Local(result)) => self.ready.push_back(json!({"id": id, "result": result}).to_string()),
            Ok(Plan::Command(method, params, reply)) => {
                self.pending.insert(id, reply);
                out.push(json!({"id": id, "method": method, "params": params}).to_string());
            }
            Err(e) => self.ready.push_back(json!({"id": id, "error": {"message": e}}).to_string()),
        }
        Ok(())
    }

    /// A BiDi message from the browser as the CDP one to hand on, if any.
    fn translate(&mut self, text: &str) -> Result<Option<String>> {
        let v: Value = serde_json::from_str(text)?;
        if let Some(id) = v["id"].as_u64() {
            let Some(reply) = self.pending.remove(&id) else { return Ok(None) };
            let answer = if v["type"] == "error" {
                match cdp_error(&reply, v["error"].as_str().unwrap_or("error"), v["message"].as_str().unwrap_or_default()) {
                    Ok(result) => json!({"id": id, "result": result}),
                    Err(e) => json!({"id": id, "error": {"message": e}}),
                }
            } else {
                let result = v.get("result").cloned().unwrap_or(Value::Null);
                json!({"id": id, "result": cdp_result(reply, result)})
            };
            return Ok(Some(answer.to_string()));
        }
        Ok(cdp_event(&v).map(|e| e.to_string()))
    }
}

impl<T: Transport> Transport for Bidi<T> {
    fn send_text(&mut self, text: &str) -> Result<()> {
        self.send_texts(&[text])
    }

    fn send_texts(&mut self, texts: &[&str]) -> Result<()> {
        let mut out = Vec::new();
        for t in texts {
            self.send_one(t, &mut out)?;
        }
        if !out.is_empty() {
            self.inner.send_texts(&out.iter().map(String::as_str).collect::<Vec<_>>())?;
        }
        Ok(())
    }

    fn recv_text(&mut self, timeout: Duration) -> Result<Option<String>> {
        if let Some(t) = self.ready.pop_front() {
            return Ok(Some(t));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(text) = self.inner.recv_text(left)? else { return Ok(None) };
            if let Some(t) = self.translate(&text)? {
                return Ok(Some(t));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(method: &str, params: Value) -> Result<Plan, String> {
        plan(method, &params, Some("tab-1"), &mut HashMap::new())
    }

    #[test]
    fn a_page_script_is_evaluated_in_its_tab_and_its_value_comes_back_as_plain_json() {
        let Ok(Plan::Command(m, p, r)) = planned("Runtime.evaluate", json!({"expression": "1+1", "returnByValue": true, "awaitPromise": true})) else { panic!() };
        assert_eq!(m, "script.evaluate");
        assert_eq!(p["target"], json!({"context": "tab-1"}));
        assert_eq!(p["awaitPromise"], json!(true));
        let value = json!({"type": "object", "value": [["state", {"type": "array", "value": [{"type": "number", "value": 3}, {"type": "boolean", "value": true}, {"type": "null"}]}], ["screen", {"type": "string", "value": "{}"}]]});
        assert_eq!(cdp_result(r, json!({"type": "success", "result": value})), json!({"result": {"value": {"state": [3, true, null], "screen": "{}"}}}));
    }

    #[test]
    fn a_script_that_throws_comes_back_as_cdp_exception_details() {
        let r = cdp_result(Reply::Evaluated, json!({"type": "exception", "exceptionDetails": {"text": "Error: boom", "exception": {"type": "error"}}}));
        assert_eq!(r, json!({"exceptionDetails": {"text": "Error: boom"}}));
    }

    #[test]
    fn values_json_cannot_hold_become_null_as_json_stringify_makes_them() {
        assert_eq!(remote_to_json(&json!({"type": "number", "value": "NaN"})), Value::Null);
        assert_eq!(remote_to_json(&json!({"type": "number", "value": "-0"})), json!(0));
        assert_eq!(remote_to_json(&json!({"type": "undefined"})), Value::Null);
        assert_eq!(remote_to_json(&json!({"type": "node", "sharedId": "x"})), Value::Null);
        assert_eq!(remote_to_json(&json!({"type": "bigint", "value": "12"})), json!(12));
        assert_eq!(remote_to_json(&json!({"type": "map", "value": [[{"type": "number", "value": 1}, {"type": "string", "value": "a"}]]})), json!({"1": "a"}));
    }

    #[test]
    fn an_isolated_world_is_a_sandbox_in_its_frame() {
        let mut worlds = HashMap::new();
        let Ok(Plan::Local(w)) = plan("Page.createIsolatedWorld", &json!({"frameId": "frame-2", "worldName": "virtuoso"}), Some("tab-1"), &mut worlds) else { panic!() };
        let id = w["executionContextId"].as_i64().unwrap();
        let Ok(Plan::Command(_, p, _)) = plan("Runtime.evaluate", &json!({"expression": "1", "contextId": id}), Some("tab-1"), &mut worlds) else { panic!() };
        assert_eq!(p["target"], json!({"context": "frame-2", "sandbox": "virtuoso"}));
        assert!(plan("Runtime.evaluate", &json!({"expression": "1", "contextId": 99}), Some("tab-1"), &mut worlds).is_err());
    }

    #[test]
    fn a_click_is_a_mouse_pointer_moving_pressing_and_releasing() {
        let Ok(Plan::Command(m, p, _)) = planned("Input.dispatchMouseEvent", json!({"type": "mousePressed", "x": 10.4, "y": 20.6, "button": "left", "clickCount": 1})) else { panic!() };
        assert_eq!(m, "input.performActions");
        let source = &p["actions"][0];
        assert_eq!(source["parameters"]["pointerType"], json!("mouse"));
        assert_eq!(source["actions"], json!([{"type": "pointerMove", "x": 10, "y": 21, "duration": 0}, {"type": "pointerDown", "button": 0}]));
        let Ok(Plan::Command(_, p, _)) = planned("Input.dispatchMouseEvent", json!({"type": "mouseReleased", "x": 10, "y": 21})) else { panic!() };
        assert_eq!(p["actions"][0]["actions"][1], json!({"type": "pointerUp", "button": 0}));
        let Ok(Plan::Command(_, p, _)) = planned("Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": 3, "y": 4})) else { panic!() };
        assert_eq!(p["actions"][0]["actions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_touch_is_a_touch_pointer_and_a_cancel_lifts_it() {
        let Ok(Plan::Command(_, p, _)) = planned("Input.dispatchTouchEvent", json!({"type": "touchStart", "touchPoints": [{"x": 5, "y": 6}]})) else { panic!() };
        assert_eq!(p["actions"][0]["parameters"]["pointerType"], json!("touch"));
        assert_eq!(p["actions"][0]["actions"][1], json!({"type": "pointerDown", "button": 0}));
        let Ok(Plan::Command(_, p, _)) = planned("Input.dispatchTouchEvent", json!({"type": "touchCancel", "touchPoints": []})) else { panic!() };
        assert_eq!(p["actions"][0]["actions"], json!([{"type": "pointerUp", "button": 0}]));
    }

    #[test]
    fn keys_are_webdriver_keys_named_or_typed() {
        let Ok(Plan::Command(_, p, _)) = planned("Input.dispatchKeyEvent", json!({"type": "keyDown", "key": "Enter", "text": "\r"})) else { panic!() };
        assert_eq!(p["actions"][0]["actions"], json!([{"type": "keyDown", "value": "\u{E007}"}]));
        let Ok(Plan::Command(_, p, _)) = planned("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": "a"})) else { panic!() };
        assert_eq!(p["actions"][0]["actions"], json!([{"type": "keyUp", "value": "a"}]));
        for (k, c) in [("Backspace", '\u{E003}'), ("Tab", '\u{E004}'), ("Escape", '\u{E00C}'), ("ArrowLeft", '\u{E012}'), ("ArrowUp", '\u{E013}'), ("ArrowRight", '\u{E014}'), ("ArrowDown", '\u{E015}'), ("Delete", '\u{E017}'), ("\n", '\u{E007}')] {
            assert_eq!(webdriver_key(k), c.to_string(), "{k}");
        }
    }

    #[test]
    fn tabs_are_browsing_contexts() {
        let Ok(Plan::Local(a)) = planned("Target.attachToTarget", json!({"targetId": "tab-9", "flatten": true})) else { panic!() };
        assert_eq!(a, json!({"sessionId": "tab-9"}));
        let Ok(Plan::Command(m, _, r)) = planned("Target.createTarget", json!({"url": "about:blank"})) else { panic!() };
        assert_eq!(m, "browsingContext.create");
        assert_eq!(cdp_result(r, json!({"context": "tab-2"})), json!({"targetId": "tab-2"}));
        let Ok(Plan::Command(m, p, _)) = planned("Target.closeTarget", json!({"targetId": "tab-2"})) else { panic!() };
        assert_eq!((m.as_str(), &p["context"]), ("browsingContext.close", &json!("tab-2")));
        let Ok(Plan::Command(_, _, r)) = planned("Target.getTargets", json!({})) else { panic!() };
        assert_eq!(cdp_result(r, json!({"contexts": [{"context": "tab-1", "url": "about:blank", "children": null}]})), json!({"targetInfos": [{"targetId": "tab-1", "type": "page", "url": "about:blank"}]}));
        for m in ["Target.setDiscoverTargets", "Page.enable", "Network.enable", "Page.screencastFrameAck", "Emulation.setTouchEmulationEnabled"] {
            assert_eq!(planned(m, json!({})), Ok(Plan::Local(json!({}))), "{m}");
        }
        assert_eq!(planned("Browser.getVersion", json!({})), Ok(Plan::Local(json!({"product": "Firefox", "userAgent": ""}))));
    }

    #[test]
    fn a_navigation_answers_at_once_and_its_load_is_an_event() {
        let Ok(Plan::Command(m, p, r)) = planned("Page.navigate", json!({"url": "http://x/"})) else { panic!() };
        assert_eq!((m.as_str(), &p["wait"]), ("browsingContext.navigate", &json!("none")));
        assert_eq!(cdp_result(r.clone(), json!({"navigation": "n1", "url": "http://x/"})), json!({"frameId": "", "loaderId": "n1"}));
        assert_eq!(cdp_result(r.clone(), json!({"navigation": null, "url": "http://x/#a"})), json!({"frameId": ""}));
        assert_eq!(cdp_error(&r, "unknown error", "NS_BINDING_ABORTED"), Ok(json!({"frameId": "", "errorText": "net::ERR_ABORTED"})));
        assert_eq!(cdp_error(&r, "unknown error", "NS_ERROR_CONNECTION_REFUSED"), Ok(json!({"frameId": "", "errorText": "unknown error: NS_ERROR_CONNECTION_REFUSED"})));
        assert_eq!(cdp_error(&Reply::Empty, "no such frame", "gone"), Err("no such frame: gone".to_string()));
        let load = cdp_event(&json!({"type": "event", "method": "browsingContext.load", "params": {"context": "tab-1", "navigation": "n1", "url": "http://x/"}})).unwrap();
        assert_eq!(load, json!({"method": "Page.loadEventFired", "sessionId": "tab-1", "params": {}}));
        let same_document = cdp_event(&json!({"method": "browsingContext.fragmentNavigated", "params": {"context": "tab-1"}})).unwrap();
        assert_eq!(same_document["method"], json!("Page.loadEventFired"));
    }

    #[test]
    fn dialogs_open_and_close_and_are_answered() {
        let open = cdp_event(&json!({"method": "browsingContext.userPromptOpened", "params": {"context": "tab-1", "type": "prompt", "message": "Name?", "defaultValue": "x"}})).unwrap();
        assert_eq!(open, json!({"method": "Page.javascriptDialogOpening", "sessionId": "tab-1", "params": {"type": "prompt", "message": "Name?", "defaultPrompt": "x"}}));
        let closed = cdp_event(&json!({"method": "browsingContext.userPromptClosed", "params": {"context": "tab-1", "accepted": true}})).unwrap();
        assert_eq!(closed["method"], json!("Page.javascriptDialogClosed"));
        let Ok(Plan::Command(m, p, _)) = planned("Page.handleJavaScriptDialog", json!({"accept": false, "promptText": "Ann"})) else { panic!() };
        assert_eq!((m.as_str(), p), ("browsingContext.handleUserPrompt", json!({"context": "tab-1", "accept": false, "userText": "Ann"})));
    }

    #[test]
    fn only_top_level_contexts_are_tabs() {
        let made = cdp_event(&json!({"method": "browsingContext.contextCreated", "params": {"context": "tab-3", "url": "about:blank", "parent": null}})).unwrap();
        assert_eq!(made, json!({"method": "Target.targetCreated", "params": {"targetInfo": {"targetId": "tab-3", "type": "page", "url": "about:blank"}}}));
        let gone = cdp_event(&json!({"method": "browsingContext.contextDestroyed", "params": {"context": "tab-3", "parent": null}})).unwrap();
        assert_eq!(gone, json!({"method": "Target.detachedFromTarget", "params": {"sessionId": "tab-3", "targetId": "tab-3"}}));
        assert_eq!(cdp_event(&json!({"method": "browsingContext.contextCreated", "params": {"context": "f", "parent": "tab-3"}})), None);
        assert_eq!(cdp_event(&json!({"method": "log.entryAdded", "params": {}})), None);
    }

    #[test]
    fn the_page_set_up_and_emulation_go_to_their_bidi_commands() {
        let cases = [
            ("Page.addScriptToEvaluateOnNewDocument", json!({"source": "x()"}), "script.addPreloadScript"),
            ("Page.captureScreenshot", json!({"format": "png"}), "browsingContext.captureScreenshot"),
            ("Page.getFrameTree", json!({}), "browsingContext.getTree"),
            ("Emulation.setDeviceMetricsOverride", json!({"width": 412, "height": 839, "deviceScaleFactor": 2.625, "mobile": true}), "browsingContext.setViewport"),
            ("Emulation.setUserAgentOverride", json!({"userAgent": "UA"}), "emulation.setUserAgentOverride"),
            ("Emulation.setGeolocationOverride", json!({"latitude": 1.0, "longitude": 2.0, "accuracy": 1}), "emulation.setGeolocationOverride"),
            ("Network.emulateNetworkConditions", json!({"offline": true}), "emulation.setNetworkConditions"),
            ("Browser.setPermission", json!({"permission": {"name": "geolocation"}, "setting": "granted", "origin": "http://x"}), "permissions.setPermission"),
            ("Storage.clearDataForOrigin", json!({"origin": "http://x"}), "storage.deleteCookies"),
        ];
        for (cdp, params, bidi) in cases {
            let Ok(Plan::Command(m, _, _)) = planned(cdp, params) else { panic!("{cdp}") };
            assert_eq!(m, bidi, "{cdp}");
        }
        let Ok(Plan::Command(_, p, _)) = planned("Page.addScriptToEvaluateOnNewDocument", json!({"source": "x()"})) else { panic!() };
        assert_eq!(p["functionDeclaration"], json!("() => {\nx()\n}"));
        let Ok(Plan::Command(_, p, _)) = planned("Emulation.setDeviceMetricsOverride", json!({"width": 800, "height": 600, "deviceScaleFactor": 0})) else { panic!() };
        assert_eq!(p["devicePixelRatio"], Value::Null);
        let Ok(Plan::Command(_, p, _)) = planned("Network.emulateNetworkConditions", json!({"offline": false})) else { panic!() };
        assert_eq!(p["networkConditions"], Value::Null);
        assert_eq!(cdp_result(Reply::Data, json!({"data": "iVBOR"})), json!({"data": "iVBOR"}));
        let tree = json!({"contexts": [{"context": "tab-1", "url": "http://x/", "children": [{"context": "f1", "url": "http://y/", "children": []}]}]});
        assert_eq!(cdp_result(Reply::FrameTree, tree), json!({"frameTree": {"frame": {"id": "tab-1", "url": "http://x/"}, "childFrames": [{"frame": {"id": "f1", "url": "http://y/"}, "childFrames": []}]}}));
    }

    #[test]
    fn what_firefox_cannot_do_says_so() {
        assert!(planned("Emulation.setEmulatedMedia", json!({})).unwrap_err().contains("dark mode"));
        assert!(planned("Page.startScreencast", json!({})).unwrap_err().contains("recording"));
        assert!(planned("Fetch.enable", json!({})).unwrap_err().contains("Fetch.enable"));
        // BiDi's own commands go as they are, and so does their result.
        assert_eq!(planned("session.subscribe", json!({"events": ["log.entryAdded"]})), Ok(Plan::Command("session.subscribe".into(), json!({"events": ["log.entryAdded"]}), Reply::Raw)));
        assert_eq!(cdp_result(Reply::Raw, json!({"subscription": "s1"})), json!({"subscription": "s1"}));
    }

    /// A browser's side of the WebSocket: what was sent, and what it says next.
    struct Fake {
        sent: Vec<String>,
        incoming: VecDeque<String>,
    }

    impl Transport for Fake {
        fn send_text(&mut self, text: &str) -> Result<()> {
            self.sent.push(text.to_string());
            Ok(())
        }
        fn recv_text(&mut self, _timeout: Duration) -> Result<Option<String>> {
            Ok(self.incoming.pop_front())
        }
    }

    #[test]
    fn the_cdp_client_talks_bidi_through_it() {
        let fake = Fake {
            sent: vec![],
            incoming: VecDeque::from([
                r#"{"type":"event","method":"browsingContext.contextCreated","params":{"context":"f","parent":"tab-1"}}"#.to_string(),
                r#"{"type":"event","method":"browsingContext.load","params":{"context":"tab-1"}}"#.to_string(),
                r#"{"type":"success","id":2,"result":{"type":"success","result":{"type":"string","value":"hello"}}}"#.to_string(),
                r#"{"type":"error","id":3,"error":"no such frame","message":"gone"}"#.to_string(),
            ]),
        };
        let mut cdp = crate::chrome::Cdp::new(Bidi::new(fake));
        // Answered here, without the browser.
        assert_eq!(cdp.call(Some("tab-1"), "Page.enable", json!({})).unwrap(), json!({}));
        // Sent as BiDi; the iframe's creation passed over, the load kept for the device.
        assert_eq!(cdp.call(Some("tab-1"), "Runtime.evaluate", json!({"expression": "document.title", "returnByValue": true})).unwrap(), json!({"result": {"value": "hello"}}));
        assert_eq!(cdp.next_event(Duration::ZERO).unwrap().unwrap()["method"], json!("Page.loadEventFired"));
        let e = cdp.call(Some("tab-1"), "Page.captureScreenshot", json!({})).unwrap_err().to_string();
        assert!(e.contains("no such frame: gone"), "{e}");
        assert!(cdp.call(Some("tab-1"), "Emulation.setEmulatedMedia", json!({})).unwrap_err().to_string().contains("dark mode"));
    }

    #[test]
    fn calls_answered_here_and_sent_go_in_one_batch_and_answer_in_order() {
        let fake = Fake { sent: vec![], incoming: VecDeque::from([r#"{"type":"success","id":2,"result":{}}"#.to_string()]) };
        let mut b = Bidi::new(fake);
        b.send_texts(&[r#"{"id":1,"method":"Page.enable","params":{},"sessionId":"tab-1"}"#, r#"{"id":2,"method":"Page.captureScreenshot","params":{},"sessionId":"tab-1"}"#]).unwrap();
        assert_eq!(b.inner.sent.len(), 1, "only the BiDi command went: {:?}", b.inner.sent);
        assert!(b.inner.sent[0].contains("browsingContext.captureScreenshot"));
        assert_eq!(serde_json::from_str::<Value>(&b.recv_text(Duration::ZERO).unwrap().unwrap()).unwrap()["id"], json!(1));
        assert_eq!(serde_json::from_str::<Value>(&b.recv_text(Duration::ZERO).unwrap().unwrap()).unwrap()["id"], json!(2));
        // A reply to nothing sent (a call given up on) is passed over.
        b.inner.incoming.push_back(r#"{"type":"success","id":7,"result":{}}"#.to_string());
        assert_eq!(b.recv_text(Duration::ZERO).unwrap(), None);
    }
}
