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
        // A thread per connection: Chrome opens one ahead of time and may send nothing on it.
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let _ = request_path(&mut s);
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// Reads a request's head: its path and query.
fn request_path(s: &mut std::net::TcpStream) -> Option<String> {
    let mut r = BufReader::new(s.try_clone().ok()?);
    let mut first = String::new();
    r.read_line(&mut first).ok()?;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
    }
    Some(first.split_whitespace().nth(1)?.to_string())
}

/// A server for the fixture site (tests/fixtures/web): its base URL.
fn serve_fixtures() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("web");
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let root = root.clone();
            std::thread::spawn(move || {
                let full = request_path(&mut s).unwrap_or_default();
                // slow?ms=N answers "ok" after N ms (the path keeps its query here).
                if let Some(ms) = full.split("slow?ms=").nth(1).and_then(|n| n.parse::<u64>().ok()) {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    let _ = write!(s, "HTTP/1.1 200 OK
Content-Type: text/plain
Content-Length: 2
Connection: close

ok");
                    return;
                }
                let path = full.split(['?', '#']).next().unwrap_or_default().to_string();
                let path = path.trim_start_matches('/');
                let path = if path.is_empty() { "index.html" } else { path };
                match std::fs::read(root.join(path)).ok().filter(|_| !path.contains("..")) {
                    Some(body) => {
                        let kind = if path.ends_with(".html") { "text/html; charset=utf-8" } else { "application/octet-stream" };
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(&body);
                    }
                    None => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            });
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

const INPUT: &str = r#"<!doctype html><title>Input</title>
<style>body{margin:0} button,input{display:block;width:200px;height:40px;margin:4px}</style>
<script>window.log=[]; const L=(e)=>log.push(e.type+(e.isTrusted?'':'(untrusted)'));</script>
<button id="b" onclick="L(event)" ondblclick="L(event)" onmousedown="window.down=performance.now()" onmouseup="window.held=performance.now()-window.down">Tap me</button>
<form id="f" onsubmit="event.preventDefault(); log.push('submit')"><input id="name" oninput="L(event)"><input id="next"><button id="go">Go</button></form>
"#;

fn centre(d: &mut WebDevice, id: &str) -> (i32, i32) {
    let s = d.screen().unwrap();
    by_id(&s, id).bounds.center()
}

fn log(d: &mut WebDevice) -> Vec<String> {
    serde_json::from_value(d.evaluate("log").unwrap()).unwrap()
}

#[test]
fn a_tap_is_a_trusted_click_and_two_taps_a_double_click() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    d.tap(x, y).unwrap();
    assert_eq!(log(&mut d), ["click"]);
    d.taps(x, y, 2, std::time::Duration::from_millis(50)).unwrap();
    assert_eq!(log(&mut d), ["click", "click", "click", "dblclick"]);
}

#[test]
fn a_long_press_holds_the_button_down() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    d.long_press(x, y, 600).unwrap();
    let held = d.evaluate("window.held").unwrap().as_f64().unwrap();
    assert!(held >= 550.0, "held {held} ms");
}

#[test]
fn text_is_typed_into_the_focused_field_and_erased() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.input_text("Grace Hopper").unwrap();
    assert_eq!(d.evaluate("document.getElementById('name').value").unwrap(), json!("Grace Hopper"));
    assert!(log(&mut d).contains(&"input".to_string()), "the page heard it: {:?}", log(&mut d));
    d.erase_text(7).unwrap();
    assert_eq!(d.evaluate("document.getElementById('name').value").unwrap(), json!("Grace"));
}

#[test]
fn keys_reach_the_page_as_a_user_presses_them() {
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.press_key(Key::Tab).unwrap();
    assert_eq!(d.evaluate("document.activeElement.id").unwrap(), json!("next"));
    d.press_key(Key::Enter).unwrap();
    assert!(log(&mut d).contains(&"submit".to_string()), "Enter submits the form: {:?}", log(&mut d));
    assert!(d.press_key(Key::VolumeUp).is_err(), "no volume on a page");
}

#[test]
fn back_goes_back_in_the_pages_history() {
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    let url = serve(INPUT);
    launch(&mut d, &format!("{url}/a"), false);
    d.evaluate("location.href = '/b'").unwrap();
    wait_for(&mut d, "location.pathname === '/b'");
    d.press_key(Key::Back).unwrap();
    wait_for(&mut d, "location.pathname === '/a'");
}

fn wait_for(d: &mut WebDevice, condition: &str) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < end {
        if d.evaluate(condition).map(|v| v == json!(true)).unwrap_or(false) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("never true: {condition}");
}

#[test]
fn the_fixture_site_is_served_and_a_form_on_it_filled_in() {
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("document.title").unwrap(), json!("Fixture"));
    let (x, y) = centre(&mut d, "nav-form");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.title === 'Form' && document.readyState === 'complete'");
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.input_text("Ada").unwrap();
    d.press_key(virtuoso::device::Key::Enter).unwrap();
    wait_for(&mut d, "document.getElementById('status').textContent === 'Signed up Ada'");
}

/// The settle page, opened and marked, with `button` tapped: how long a settle took and what it said.
fn settle_after(button: &str, no_change_ms: u64, timeout_ms: u64) -> Option<(WebDevice, std::time::Duration, virtuoso::device::Settled)> {
    let mut d = device()?;
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/settle.html"), false);
    let (x, y) = centre(&mut d, button);
    d.mark().unwrap();
    d.tap(x, y).unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(no_change_ms), std::time::Duration::from_millis(timeout_ms)).unwrap();
    Some((d, t.elapsed(), s))
}

fn out(d: &mut WebDevice) -> String {
    d.evaluate("document.getElementById('out').textContent").unwrap().as_str().unwrap_or_default().to_string()
}

#[test]
fn a_settle_waits_for_a_change_that_comes_a_moment_after_the_tap() {
    let Some((mut d, _, s)) = settle_after("later", 150, 5000) else { return };
    assert_eq!(out(&mut d), "Appeared later");
    assert!(s.changed && !s.timed_out, "{s:?}");
}

#[test]
fn a_settle_waits_for_fetch_and_xhr_requests() {
    for (button, text) in [("fetch", "Fetched ok"), ("xhr", "Xhr ok")] {
        let Some((mut d, took, s)) = settle_after(button, 100, 5000) else { return };
        assert_eq!(out(&mut d), text, "after {took:?}");
        assert!(!s.timed_out);
    }
}

#[test]
fn a_settle_waits_for_an_animation_to_end() {
    let Some((mut d, took, _)) = settle_after("animate", 100, 5000) else { return };
    assert_eq!(out(&mut d), "Grown", "after {took:?}");
}

#[test]
fn neither_an_endless_animation_nor_a_long_poll_holds_a_settle() {
    // The page has a spinner that never stops; the long poll answers after 15 s.
    let Some((_, took, s)) = settle_after("longpoll", 100, 10_000) else { return };
    assert!(took < std::time::Duration::from_secs(4), "took {took:?}");
    assert!(!s.timed_out);
}

#[test]
fn a_still_page_settles_at_once_and_unchanged() {
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/form.html"), false);
    d.mark().unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(100), std::time::Duration::from_secs(5)).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_millis(600), "took {:?}", t.elapsed());
    assert!(!s.changed && !s.timed_out, "{s:?}");
}

#[test]
fn a_page_that_never_stops_changing_times_out() {
    let Some((_, took, s)) = settle_after("forever", 200, 1500) else { return };
    assert!(s.timed_out, "{s:?}");
    assert!(took >= std::time::Duration::from_millis(1400) && took < std::time::Duration::from_millis(3000), "took {took:?}");
}

#[test]
fn a_poll_re_armed_for_ever_holds_a_settle_only_briefly() {
    let Some((_, took, s)) = settle_after("poll", 200, 10_000) else { return };
    assert!(!s.timed_out, "{s:?}");
    assert!(took < std::time::Duration::from_secs(3), "took {took:?}");
}


