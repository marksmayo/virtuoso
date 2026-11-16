//! Tests against a real Chrome: run with VIRTUOSO_WEB=1 (and VIRTUOSO_CHROME=<path> where
//! Chrome is not installed in the usual place). Without it they pass without running.

use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use virtuoso::chrome::{find_chrome, Browser};
use virtuoso::device::{Device, LaunchOptions};
use virtuoso::web::WebDevice;

fn chrome() -> Option<std::path::PathBuf> {
    if std::env::var("VIRTUOSO_WEB").is_err() {
        return None;
    }
    Some(find_chrome().expect("Chrome, for VIRTUOSO_WEB"))
}

/// A server on a free port that answers every request with `page`: its base URL.
fn serve(page: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn device() -> Option<WebDevice> {
    let path = chrome()?;
    Some(WebDevice::launch(&path, true).unwrap())
}

fn launch(d: &mut WebDevice, url: &str, clear_state: bool) {
    d.launch_app(url, &LaunchOptions { clear_state, stop_app: true, ..Default::default() }).unwrap();
}

#[test]
fn a_launched_chrome_is_headless_answers_and_opens_a_page() {
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    assert!(b.product.starts_with("Chrome/") || b.product.starts_with("HeadlessChrome/"), "{}", b.product);
    let ua = b.cdp.call(None, "Browser.getVersion", json!({})).unwrap()["userAgent"].as_str().unwrap_or_default().to_string();
    assert!(ua.contains("HeadlessChrome/"), "headless: {ua}");
    let t = b.cdp.call(None, "Target.createTarget", json!({"url": "about:blank"})).unwrap();
    assert!(t["targetId"].as_str().is_some_and(|s| !s.is_empty()), "{t}");
}

#[test]
fn a_running_chrome_can_be_attached_to_and_is_left_running() {
    let Some(path) = chrome() else { return };
    let mut launched = Browser::launch(&path, true).unwrap();
    let attached = Browser::attach(&launched.ws_url).unwrap();
    assert_eq!(attached.product, launched.product);
    drop(attached);
    // Still there: attaching does not stop the browser.
    assert!(launched.cdp.call(None, "Browser.getVersion", json!({})).is_ok());
}

#[test]
fn a_launched_chrome_takes_its_profile_with_it_when_dropped() {
    let Some(path) = chrome() else { return };
    let before: std::collections::HashSet<_> = profiles();
    let b = Browser::launch(&path, true).unwrap();
    assert!(profiles().len() > before.len(), "a profile of its own");
    drop(b);
    let left: Vec<_> = profiles().difference(&before).cloned().collect();
    assert!(left.is_empty(), "profiles left behind: {left:?}");
}

fn profiles() -> std::collections::HashSet<String> {
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with(&format!("virtuoso-chrome-{}-", std::process::id())))
        .collect()
}

const PROBE: &str = "<!doctype html><title>Probe</title><p id=greeting>hello</p>";

#[test]
fn launch_app_opens_the_flows_url_in_a_page_once_it_has_loaded() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    assert_eq!(d.evaluate("document.title").unwrap(), json!("Probe"));
    assert_eq!(d.evaluate("location.pathname").unwrap(), json!("/start"));
    assert_eq!(d.evaluate("document.readyState").unwrap(), json!("complete"));
}

#[test]
fn clear_state_forgets_what_the_site_stored_and_a_plain_launch_keeps_it() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    d.evaluate("localStorage.setItem('k', 'v'); document.cookie = 'c=1; path=/'").unwrap();
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!("v"), "kept without clearState");
    assert_eq!(d.evaluate("document.cookie").unwrap(), json!("c=1"));
    launch(&mut d, &url, true);
    assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!(null), "gone after clearState");
    assert_eq!(d.evaluate("document.cookie").unwrap(), json!(""));
}

#[test]
fn stop_app_closes_the_page_and_a_launch_after_opens_a_new_one() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    d.evaluate("window.marker = 42").unwrap();
    d.stop_app(&url).unwrap();
    assert!(d.evaluate("1").is_err(), "no page after stopApp");
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("window.marker === undefined").unwrap(), json!(true), "a fresh page");
}

#[test]
fn the_platform_is_web() {
    let Some(d) = device() else { return };
    assert_eq!(d.platform(), "web");
}


const SCREEN: &str = r#"<!doctype html><title>Screen</title>
<style>body{margin:0} #box{position:absolute;left:10px;top:20px;width:100px;height:50px}</style>
<button id="by-id">Alpha</button>
<button data-testid="by-testid">Bravo</button>
<button id="both-id" data-testid="both-testid">Golf</button>
<input id="field" placeholder="Delta placeholder">
<input id="filled" value="India value">
<input id="secret" type="password" value="hunter2">
<p id="mixed">Order <b>ready</b> now</p>
<div id="gone" style="display:none">Foxtrot hidden</div>
<div id="invisible" style="visibility:hidden">Juliet invisible</div>
<input id="tick" type="checkbox" checked>
<button id="off" disabled>Kilo</button>
<div id="box">Box</div>
<div id="pointer" style="cursor:pointer">Lima</div>
"#;

fn screen_of(page: &'static str) -> Option<(WebDevice, virtuoso::screen::ScreenTree)> {
    let mut d = device()?;
    let url = serve(page);
    launch(&mut d, &url, false);
    let s = d.screen().unwrap();
    Some((d, s))
}

fn by_id<'a>(s: &'a virtuoso::screen::ScreenTree, id: &str) -> &'a virtuoso::screen::Element {
    s.elements.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no element with id {id} in {:?}", s.elements.iter().map(|e| (&e.id, &e.text)).collect::<Vec<_>>()))
}

#[test]
fn the_screen_has_each_rendered_element_with_its_own_text_and_id() {
    use virtuoso::screen::*;
    let Some((_d, s)) = screen_of(SCREEN) else { return };
    let alpha = by_id(&s, "by-id");
    assert_eq!(alpha.text, "Alpha");
    assert_eq!(&*alpha.class, "button");
    assert!(alpha.has(CLICKABLE) && alpha.has(ENABLED) && alpha.has(VISIBLE));
    assert_eq!(by_id(&s, "by-testid").text, "Bravo", "data-testid stands in for a missing id");
    assert_eq!(by_id(&s, "both-id").text, "Golf", "the id, where it has both");
    assert!(!s.elements.iter().any(|e| e.id == "both-testid"));
    // Text inside inline tags stays theirs: the paragraph has only its own.
    assert_eq!(by_id(&s, "mixed").text, "Order now");
    assert!(s.elements.iter().any(|e| e.text == "ready" && e.depth > by_id(&s, "mixed").depth));
}

#[test]
fn fields_show_their_value_as_text_their_placeholder_as_hint() {
    use virtuoso::screen::*;
    let Some((_d, s)) = screen_of(SCREEN) else { return };
    let field = by_id(&s, "field");
    assert_eq!((field.text.as_str(), field.hint.as_str()), ("", "Delta placeholder"));
    assert!(field.has(EDITABLE) && field.has(CLICKABLE));
    assert_eq!(by_id(&s, "filled").text, "India value");
    assert!(by_id(&s, "secret").has(PASSWORD));
}

#[test]
fn hidden_elements_are_not_on_the_screen() {
    let Some((_d, s)) = screen_of(SCREEN) else { return };
    for id in ["gone", "invisible"] {
        assert!(!s.elements.iter().any(|e| e.id == id), "{id}");
    }
    assert!(!s.elements.iter().any(|e| e.text.contains("Foxtrot") || e.text.contains("Juliet")));
}

#[test]
fn states_bounds_and_the_viewport_are_read() {
    use virtuoso::screen::*;
    let Some((mut d, s)) = screen_of(SCREEN) else { return };
    assert!(by_id(&s, "tick").has(CHECKABLE) && by_id(&s, "tick").has(CHECKED));
    assert!(!by_id(&s, "off").has(ENABLED));
    assert!(by_id(&s, "pointer").has(CLICKABLE), "a pointer cursor marks a clickable div");
    assert!(!by_id(&s, "box").has(CLICKABLE));
    assert_eq!(by_id(&s, "box").bounds, Rect { left: 10, top: 20, right: 110, bottom: 70 });
    let (w, h) = (d.evaluate("innerWidth").unwrap(), d.evaluate("innerHeight").unwrap());
    assert_eq!((json!(s.width), json!(s.height)), (w, h));
    d.evaluate("document.getElementById('field').focus()").unwrap();
    assert!(by_id(&d.screen().unwrap(), "field").has(FOCUSED));
}
