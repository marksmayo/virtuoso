//! The web as a device: a page in Chrome, driven over the DevTools Protocol. A flow's `url:`
//! is its app: `launchApp` opens it in a fresh tab (`clearState` first forgets what the site
//! stored), `stopApp` closes the tab.

use crate::chrome::Browser;
use crate::device::{Device, Key, LaunchOptions, Lift, Settled};
use crate::screen::ScreenTree;
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

fn not_yet<T>(what: &str) -> Result<T> {
    bail!("{what} is not supported on the web yet")
}

impl Device for WebDevice {
    fn platform(&self) -> &'static str {
        "web"
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        not_yet("reading the screen")
    }

    fn tap(&mut self, _x: i32, _y: i32) -> Result<()> {
        not_yet("tapOn")
    }

    fn long_press(&mut self, _x: i32, _y: i32, _ms: u64) -> Result<()> {
        not_yet("longPressOn")
    }

    fn swipe(&mut self, _from: (i32, i32), _to: (i32, i32), _ms: u64, _end: Lift) -> Result<()> {
        not_yet("swipe")
    }

    fn press_key(&mut self, _key: Key) -> Result<()> {
        not_yet("pressKey")
    }

    fn input_text(&mut self, _text: &str) -> Result<()> {
        not_yet("inputText")
    }

    fn erase_text(&mut self, _chars: usize) -> Result<()> {
        not_yet("eraseText")
    }

    fn mark(&mut self) -> Result<()> {
        Ok(())
    }

    fn settle(&mut self, _no_change: Duration, _timeout: Duration) -> Result<Settled> {
        not_yet("waiting for the page to settle")
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
    fn a_sites_origin_is_its_scheme_host_and_port() {
        assert_eq!(origin("http://127.0.0.1:4280/e/login?x=1#top").unwrap(), "http://127.0.0.1:4280");
        assert_eq!(origin("https://example.com").unwrap(), "https://example.com");
        assert_eq!(origin("https://example.com?q").unwrap(), "https://example.com");
        assert!(origin("example.com").is_err());
        assert!(origin("http://").is_err());
    }
}
