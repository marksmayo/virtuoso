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
    /// The tab the flow drives, once it has launched.
    page: Option<Page>,
}

struct Page {
    target: String,
    session: String,
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
        Ok(WebDevice { browser: Browser::launch(path, headless)?, page: None })
    }

    /// The Chrome already running at `ws_url`.
    pub fn attach(ws_url: &str) -> Result<WebDevice> {
        Ok(WebDevice { browser: Browser::attach(ws_url)?, page: None })
    }

    /// `expression`'s value in the page (awaited when it is a promise).
    pub fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let r = self.page_call("Runtime.evaluate", json!({"expression": expression, "returnByValue": true, "awaitPromise": true}))?;
        if let Some(e) = r.get("exceptionDetails") {
            let text = e.pointer("/exception/description").or_else(|| e.get("text")).and_then(Value::as_str).unwrap_or("an exception");
            bail!("the page threw: {text}");
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// `method` on the page's session.
    fn page_call(&mut self, method: &str, params: Value) -> Result<Value> {
        let session = self.page.as_ref().map(|p| p.session.clone()).ok_or_else(|| anyhow!("no page is open: launchApp first"))?;
        self.browser.cdp.call(Some(&session), method, params)
    }

    /// A fresh tab, attached to with a session of its own, its page events on.
    fn open_page(&mut self) -> Result<()> {
        let target = self.browser.cdp.call(None, "Target.createTarget", json!({"url": "about:blank"}))?["targetId"].as_str().unwrap_or_default().to_string();
        let session = self.browser.cdp.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}))?["sessionId"].as_str().unwrap_or_default().to_string();
        self.page = Some(Page { target, session });
        self.page_call("Page.enable", json!({}))?;
        self.page_call("Page.addScriptToEvaluateOnNewDocument", json!({"source": TRACKER_JS}))?;
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
        let mut p = json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": clicks});
        if kind == "mouseMoved" {
            p["button"] = json!("none");
        }
        if kind == "mousePressed" {
            p["buttons"] = json!(1);
        }
        self.page_call("Input.dispatchMouseEvent", p)?;
        Ok(())
    }

    fn key(&mut self, key: &str, code: &str, vk: u32, text: &str) -> Result<()> {
        let mut down = json!({"type": "keyDown", "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk});
        if !text.is_empty() {
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
        }
        self.page_call("Input.dispatchKeyEvent", down)?;
        self.page_call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk}))?;
        Ok(())
    }
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
}

pub fn target(device: &str) -> Target {
    if device.eq_ignore_ascii_case("chrome") {
        Target::Chrome
    } else if device.starts_with("ws://") {
        Target::Attach(device.to_string())
    } else {
        Target::Path(device.to_string())
    }
}

/// The browser `--device` names, headless unless VIRTUOSO_HEADED is set (`--headed`).
pub fn connect(device: &str) -> Result<WebDevice> {
    let headless = std::env::var_os("VIRTUOSO_HEADED").is_none();
    match target(device) {
        Target::Chrome => WebDevice::launch(&crate::chrome::find_chrome()?, headless),
        Target::Attach(url) => WebDevice::attach(&url),
        Target::Path(p) => WebDevice::launch(Path::new(&p), headless),
    }
}

impl Device for WebDevice {
    fn platform(&self) -> &'static str {
        "web"
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        let v = self.evaluate(SCREEN_JS)?;
        let origin = self.evaluate("location.origin")?;
        tree_from(&v, origin.as_str().unwrap_or_default())
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<()> {
        self.taps(x, y, 1, Duration::ZERO)
    }

    /// Clicks with a rising click count: two make a double click.
    fn taps(&mut self, x: i32, y: i32, count: u32, interval: Duration) -> Result<()> {
        self.mouse("mouseMoved", x, y, 0)?;
        for n in 1..=count.max(1) {
            if n > 1 {
                std::thread::sleep(interval);
            }
            self.mouse("mousePressed", x, y, n)?;
            self.mouse("mouseReleased", x, y, n)?;
        }
        Ok(())
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.mouse("mouseMoved", x, y, 0)?;
        self.mouse("mousePressed", x, y, 1)?;
        std::thread::sleep(Duration::from_millis(ms));
        self.mouse("mouseReleased", x, y, 1)
    }

    /// The mouse wheel, as a desktop user scrolls: over where the swipe starts, by as far as it
    /// goes (a swipe up shows what is below). A drag with a mouse would select text instead.
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), _ms: u64, _end: Lift) -> Result<()> {
        self.mouse("mouseMoved", from.0, from.1, 0)?;
        self.page_call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseWheel", "x": from.0, "y": from.1, "deltaX": from.0 - to.0, "deltaY": from.1 - to.1}),
        )?;
        // The scroll lands on a frame after the event: two frames on, it has.
        self.evaluate("new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))")?;
        Ok(())
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
        if key == Key::Back {
            self.evaluate("history.back()")?;
            return Ok(());
        }
        let Some((k, code, vk, text)) = key_event(key) else { bail!("the {key:?} key is not on the web") };
        self.key(k, code, vk, text)
    }

    fn input_text(&mut self, text: &str) -> Result<()> {
        self.page_call("Input.insertText", json!({"text": text}))?;
        Ok(())
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        for _ in 0..chars {
            self.key("Backspace", "Backspace", 8, "")?;
        }
        Ok(())
    }

    fn mark(&mut self) -> Result<()> {
        self.evaluate("window.__virtuoso && window.__virtuoso.mark()")?;
        Ok(())
    }

    /// Back once the page has been still for `no_change` (see `still`), or at `timeout`.
    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let deadline = Instant::now() + timeout;
        let mut changed = false;
        loop {
            // A read during a navigation can find no page context: the page is not still yet.
            let state = self.evaluate("window.__virtuoso ? window.__virtuoso.state() : null").unwrap_or(Value::Null);
            let (quiet, c) = still(&state, no_change);
            changed |= c || state.is_null();
            if quiet {
                return Ok(Settled { changed, timed_out: false });
            }
            if Instant::now() >= deadline {
                return Ok(Settled { changed, timed_out: true });
            }
            std::thread::sleep(SETTLE_POLL);
        }
    }

    fn launch_app(&mut self, app: &str, opts: &LaunchOptions) -> Result<()> {
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
        if let Some(p) = self.page.take() {
            self.browser.cdp.call(None, "Target.closeTarget", json!({"targetId": p.target}))?;
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
    fn a_device_name_is_chrome_a_browser_to_attach_to_or_a_browsers_path() {
        assert_eq!(target("chrome"), Target::Chrome);
        assert_eq!(target("Chrome"), Target::Chrome);
        assert_eq!(target("ws://127.0.0.1:9222/devtools/browser/x"), Target::Attach("ws://127.0.0.1:9222/devtools/browser/x".into()));
        assert_eq!(target("/opt/chrome/chrome"), Target::Path("/opt/chrome/chrome".into()));
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
