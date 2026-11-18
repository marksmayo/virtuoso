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
    let b = Browser::launch(&path, true).unwrap();
    // Its own profile, by name: the other tests' browsers make theirs alongside it meanwhile.
    let profile = b.profile().expect("a profile of its own").to_path_buf();
    assert!(profile.is_dir(), "{} is there while it runs", profile.display());
    drop(b);
    assert!(!profile.exists(), "{} left behind", profile.display());
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
fn each_element_of_a_screen_is_of_the_pages_origin_its_app() {
    let Some(mut d) = device() else { return };
    let url = serve(SCREEN);
    launch(&mut d, &url, false);
    let s = d.screen().unwrap();
    assert!(!s.elements.is_empty());
    assert!(s.elements.iter().all(|e| *e.package == *url), "{url}: {:?}", s.elements.iter().map(|e| &e.package).collect::<std::collections::HashSet<_>>());
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



fn opened(page: &str) -> Option<WebDevice> {
    let mut d = device()?;
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/{page}"), false);
    Some(d)
}

fn num(d: &mut WebDevice, js: &str) -> f64 {
    d.evaluate(js).unwrap().as_f64().unwrap_or(f64::NAN)
}

#[test]
fn a_swipe_up_scrolls_the_page_down_by_about_its_length() {
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    d.swipe((300, 600), (300, 200), 300, Lift::Still).unwrap();
    let y = num(&mut d, "scrollY");
    assert!((350.0..=450.0).contains(&y), "scrollY {y}");
    d.swipe((300, 200), (300, 600), 300, Lift::Still).unwrap();
    assert_eq!(num(&mut d, "scrollY"), 0.0, "and back");
}

#[test]
fn a_swipe_over_a_scrolling_box_scrolls_the_box_not_the_page() {
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    let (x, y) = centre(&mut d, "inner");
    d.swipe((x, y + 60), (x, y - 60), 300, Lift::Still).unwrap();
    assert!(num(&mut d, "document.getElementById('inner').scrollTop") >= 100.0);
    assert_eq!(num(&mut d, "scrollY"), 0.0);
    let (x, y) = centre(&mut d, "wide");
    d.swipe((x + 100, y), (x - 100, y), 300, Lift::Still).unwrap();
    assert!(num(&mut d, "document.getElementById('wide').scrollLeft") >= 150.0, "sideways too");
}

#[test]
fn a_drag_moves_what_follows_the_pointer() {
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    d.drag((x, y), (x + 200, y + 100), 100, 300).unwrap();
    assert_eq!(out(&mut d), "Knob at 220,120");
}

#[test]
fn a_drag_drops_an_html5_draggable_on_its_target() {
    let Some(mut d) = opened("drag.html") else { return };
    let from = centre(&mut d, "card");
    let to = centre(&mut d, "zone");
    d.drag(from, to, 100, 300).unwrap();
    assert_eq!(out(&mut d), "Dropped card");
}

/// `virtuoso --platform web test` on a flow written to a temporary folder: its exit status and output.
fn run_flow(flow: &str) -> (Option<i32>, String) {
    run_flow_on(None, flow)
}

/// The same on a device (`--device`), when one is named.
fn run_flow_on(device: Option<&str>, flow: &str) -> (Option<i32>, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, flow).unwrap();
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso"));
    c.args(["--platform", "web"]);
    if let Some(d) = device {
        c.args(["--device", d]);
    }
    let o = c.args(["test", "--no-ansi"]).arg(&path).output().unwrap();
    (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn a_button_is_found_by_its_accessible_name_as_maestro_finds_it() {
    // Maestro matches a selector's text against an element's accessibility text as well as its
    // own: a glyph button is found by its aria-label.
    if chrome().is_none() {
        return;
    }
    let url = serve(
        "<!doctype html><title>Menus</title><p id=s>none</p>\
         <button aria-label='More actions for a.pdf' onclick=\"s.textContent='a chosen'\">⋯</button>\
         <button aria-label='More actions for b.pdf' onclick=\"s.textContent='b chosen'\">⋯</button>",
    );
    let (code, out) = run_flow(&format!("url: {url}\n---\n- launchApp\n- tapOn: More actions for b.pdf\n- assertVisible: b chosen\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn a_maestro_flow_runs_on_the_web_from_the_command_line() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!(
        "url: {url}\n---\n- launchApp:\n    clearState: true\n- tapOn: Form\n- tapOn:\n    id: name\n- inputText: Ada\n- pressKey: Enter\n- assertVisible: Signed up Ada\n- assertNotVisible: Not signed up\n"
    ));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_scrolls_a_long_page_to_the_element() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/list.html\n---\n- launchApp\n- scrollUntilVisible:\n    element:\n      id: row-150\n- tapOn: Row 150\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn an_element_under_a_fixed_footer_is_not_visible() {
    use virtuoso::screen::VISIBLE;
    let Some(mut d) = opened("sticky.html") else { return };
    let s = d.screen().unwrap();
    assert!(!by_id(&s, "accept").has(VISIBLE), "in the window but covered by the footer");
    assert!(by_id(&s, "footer").has(VISIBLE));
    assert!(by_id(&s, "state").has(VISIBLE));
}

#[test]
fn an_element_a_scrolling_box_hides_is_not_visible() {
    use virtuoso::screen::VISIBLE;
    let Some(mut d) = opened("clipped.html") else { return };
    let s = d.screen().unwrap();
    assert!(!by_id(&s, "low").has(VISIBLE), "in the window but below the box's bottom");
    assert!(by_id(&s, "top").has(VISIBLE));
}

#[test]
fn scroll_until_visible_drags_inside_the_box_under_the_middle_when_its_usual_drag_would_miss_it() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/shortbox.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Row 100\n- assertVisible: Row 100\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_scrolls_a_box_until_its_element_shows() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/clipped.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Low button\n- tapOn: Low button\n- assertVisible: Pressed\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_waits_for_rows_a_feed_loads_after_each_scroll() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/lazy.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Last row\n    timeout: 60000\n- assertVisible: Last row\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_brings_an_element_out_from_under_a_fixed_footer() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/sticky.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Accept\n- tapOn: Accept\n- assertVisible: Accepted\n"));
    assert_eq!(code, Some(0), "{out}");
}

/// The element with `text` on the screen: its centre.
fn centre_of_text(d: &mut WebDevice, text: &str) -> (i32, i32) {
    let s = d.screen().unwrap();
    s.elements.iter().find(|e| e.text == text).unwrap_or_else(|| panic!("no {text:?} on {:?}", s.elements.iter().map(|e| &e.text).filter(|t| !t.is_empty()).collect::<Vec<_>>())).bounds.center()
}

#[test]
fn an_alert_shows_on_the_screen_over_the_page_and_a_tap_on_ok_closes_it() {
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "alert");
    d.tap(x, y).unwrap();
    let s = d.screen().unwrap();
    let message = s.elements.iter().find(|e| e.text == "Saved!").expect("the alert's message on screen");
    assert!(message.window > 0, "in a window over the page");
    let (x, y) = centre_of_text(&mut d, "OK");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('out').textContent === 'Alert closed'");
}

#[test]
fn a_confirm_is_answered_by_the_button_tapped() {
    for (button, said) in [("OK", "Confirmed"), ("Cancel", "Cancelled")] {
        let Some(mut d) = opened("dialogs.html") else { return };
        let (x, y) = centre(&mut d, "confirm");
        d.tap(x, y).unwrap();
        assert!(d.screen().unwrap().elements.iter().any(|e| e.text == "Delete the draft?"));
        let (x, y) = centre_of_text(&mut d, button);
        d.tap(x, y).unwrap();
        wait_for(&mut d, &format!("document.getElementById('out').textContent === '{said}'"));
    }
}

#[test]
fn a_prompt_takes_the_text_typed_into_it() {
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "prompt");
    d.tap(x, y).unwrap();
    d.input_text("Ada").unwrap();
    let (x, y) = centre_of_text(&mut d, "OK");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('out').textContent === 'Hello Ada'");
}

#[test]
fn a_new_tab_is_followed_and_closing_it_comes_back() {
    for opener in ["newtab", "popup"] {
        let Some(mut d) = opened("dialogs.html") else { return };
        let (x, y) = centre(&mut d, opener);
        d.tap(x, y).unwrap();
        d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
        assert!(d.screen().unwrap().elements.iter().any(|e| e.text == "Second tab"), "{opener}: the new tab is the screen");
        let (x, y) = centre(&mut d, "close");
        d.tap(x, y).unwrap();
        d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
        assert!(d.screen().unwrap().elements.iter().any(|e| e.id == "dialogs-marker" || e.text == "Nothing yet"), "{opener}: back on the first tab");
    }
}

/// The frames page, with its second frame served from another origin.
fn frames() -> Option<WebDevice> {
    let mut d = device()?;
    let url = serve_fixtures();
    let other = serve_fixtures();
    let port = other.rsplit(':').next().unwrap().to_string();
    launch(&mut d, &format!("{url}/frames.html?other={port}"), false);
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(10)).unwrap();
    Some(d)
}

fn texts(s: &virtuoso::screen::ScreenTree) -> Vec<String> {
    s.elements.iter().map(|e| e.text.clone()).filter(|t| !t.is_empty()).collect()
}

#[test]
fn a_same_origin_frame_is_read_and_typed_into_where_it_is_on_screen() {
    let Some(mut d) = frames() else { return };
    let s = d.screen().unwrap();
    let frame = by_id(&s, "same").bounds;
    let input = s.elements.iter().find(|e| e.hint == "Inner field" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).expect("the frame's field, inside the frame");
    let (x, y) = input.bounds.center();
    d.tap(x, y).unwrap();
    d.input_text("one").unwrap();
    let s = d.screen().unwrap();
    let (x, y) = s.elements.iter().find(|e| e.text == "Inner button" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).unwrap().bounds.center();
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Inner one".to_string()));
}

#[test]
fn a_frame_from_another_origin_is_read_and_typed_into_too() {
    let Some(mut d) = frames() else { return };
    let s = d.screen().unwrap();
    let frame = by_id(&s, "other").bounds;
    let input = s.elements.iter().find(|e| e.hint == "Inner field" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).unwrap_or_else(|| panic!("the other-origin frame's field: {:?}", texts(&s)));
    let (x, y) = input.bounds.center();
    d.tap(x, y).unwrap();
    d.input_text("two").unwrap();
    let s = d.screen().unwrap();
    let (x, y) = s.elements.iter().find(|e| e.text == "Inner button" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).unwrap().bounds.center();
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Inner two".to_string()), "{:?}", texts(&d.screen().unwrap()));
}

#[test]
fn a_shadow_dom_form_is_read_and_typed_into() {
    let Some(mut d) = frames() else { return };
    let (x, y) = centre(&mut d, "shadow-input");
    d.tap(x, y).unwrap();
    d.input_text("three").unwrap();
    let (x, y) = centre(&mut d, "shadow-button");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Shadow three");
}

#[test]
fn a_screenshot_is_a_png_of_the_viewport() {
    let Some(mut d) = opened("device.html") else { return };
    let png = d.screenshot().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    let (vw, vh) = (num(&mut d, "innerWidth * devicePixelRatio"), num(&mut d, "innerHeight * devicePixelRatio"));
    assert_eq!((w as f64, h as f64), (vw, vh));
}

#[test]
fn set_location_is_where_the_page_finds_itself() {
    let Some(mut d) = opened("device.html") else { return };
    d.set_location(-36.8485, 174.7633).unwrap();
    assert_eq!(d.evaluate("where()").unwrap(), json!([-36.8485, 174.7633]));
}

#[test]
fn permissions_are_granted_and_denied_for_the_site() {
    let Some(mut d) = opened("device.html") else { return };
    let url = d.evaluate("location.origin").unwrap().as_str().unwrap().to_string();
    d.set_permissions(&url, &[("notifications".into(), "allow".into()), ("camera".into(), "deny".into())]).unwrap();
    assert_eq!(d.evaluate("permission('notifications')").unwrap(), json!("granted"));
    assert_eq!(d.evaluate("permission('camera')").unwrap(), json!("denied"));
}

#[test]
fn dark_mode_is_the_pages_colour_scheme() {
    let Some(mut d) = opened("device.html") else { return };
    d.set_dark_mode(Some(true)).unwrap();
    assert_eq!(d.evaluate("matchMedia('(prefers-color-scheme: dark)').matches").unwrap(), json!(true));
    assert!(d.dark_mode().unwrap());
    d.set_dark_mode(Some(false)).unwrap();
    assert!(!d.dark_mode().unwrap());
}

#[test]
fn airplane_mode_takes_the_page_offline_and_back() {
    let Some(mut d) = opened("device.html") else { return };
    d.set_airplane_mode(Some(true)).unwrap();
    assert_eq!(d.evaluate("navigator.onLine").unwrap(), json!(false));
    assert_eq!(d.evaluate("fetch('device.html').then(() => 'got', () => 'failed')").unwrap(), json!("failed"));
    d.set_airplane_mode(Some(false)).unwrap();
    assert_eq!(d.evaluate("fetch('device.html').then(() => 'got', () => 'failed')").unwrap(), json!("got"));
}

#[test]
fn orientation_turns_the_viewport() {
    use virtuoso::device::Orientation;
    let Some(mut d) = opened("device.html") else { return };
    d.set_orientation(Orientation::Portrait).unwrap();
    assert!(num(&mut d, "innerHeight") > num(&mut d, "innerWidth"));
    d.set_orientation(Orientation::LandscapeLeft).unwrap();
    assert!(num(&mut d, "innerWidth") > num(&mut d, "innerHeight"));
}

#[test]
fn open_link_goes_to_the_page_and_hide_keyboard_waits_for_nothing() {
    let Some(mut d) = opened("device.html") else { return };
    let url = d.evaluate("location.origin").unwrap().as_str().unwrap().to_string();
    d.open_link(&format!("{url}/form.html"), None, false).unwrap();
    wait_for(&mut d, "document.title === 'Form'");
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    let t = std::time::Instant::now();
    d.await_keyboard(std::time::Duration::from_secs(5), std::time::Duration::from_millis(100)).unwrap();
    d.hide_keyboard().unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(1), "took {:?}", t.elapsed());
}

#[test]
fn a_recording_is_a_video_of_the_page() {
    let Some(mut d) = opened("list.html") else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.mp4");
    d.start_recording(&path).unwrap();
    for _ in 0..5 {
        d.swipe((300, 600), (300, 300), 200, virtuoso::device::Lift::Still).unwrap();
        d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(5)).unwrap();
    }
    d.stop_recording().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 1000 && &bytes[4..8] == b"ftyp", "an mp4 of {} bytes", bytes.len());
}

fn virtuoso_web(args: &[&str]) -> (Option<i32>, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(["--platform", "web"]).args(args).output().unwrap();
    (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn a_session_drives_a_page_one_command_at_a_time() {
    if chrome().is_none() {
        return;
    }
    /// Stops the session however the test ends: a session left running holds the test's pipes.
    struct Stop;
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = virtuoso_web(&["session", "stop"]);
        }
    }
    let url = serve_fixtures();
    let _ = virtuoso_web(&["session", "stop"]);
    let (code, out) = virtuoso_web(&["session", "start", "--app", &url]);
    let _stop = Stop;
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = virtuoso_web(&["do", "launchApp", "tapOn: Form"]);
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = virtuoso_web(&["look"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Sign up"), "the form page: {out}");
}

#[test]
fn mcp_runs_flow_commands_on_a_page() {
    use std::io::{BufRead, BufReader, Write as _};
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .args(["--platform", "web", "mcp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "run", "arguments": {"appId": url, "commands": "- launchApp\n- assertVisible: Fixture"}}});
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
    writeln!(stdin, "{call}").unwrap();
    drop(stdin);
    let lines: Vec<String> = BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok).collect();
    child.wait().unwrap();
    assert!(lines[0].contains("web page"), "the instructions say it drives web pages too: {}", lines[0]);
    let reply: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
    assert_ne!(reply["result"]["isError"], json!(true), "{}", lines[1]);
}

#[test]
fn a_closed_shadow_root_is_read_and_tapped_into_too() {
    let Some(mut d) = frames() else { return };
    let s = d.screen().unwrap();
    assert_eq!(by_id(&s, "closed-hint").text, "Closed code: CLOSED-7");
    let (x, y) = centre(&mut d, "closed-button");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Closed pressed");
}

#[test]
fn a_settle_on_the_web_needs_no_more_than_100_ms_of_quiet() {
    // The tracker sees requests, timers and animations itself: the 300 ms a phone needs, to be
    // sure its accessibility events have stopped, would only slow every web step.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/form.html"), false);
    d.mark().unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    let took = t.elapsed();
    assert!(!s.timed_out);
    assert!(took < std::time::Duration::from_millis(230), "took {took:?}");
}

#[test]
fn a_page_taller_than_the_window_is_a_scrolling_list() {
    use virtuoso::screen::SCROLLABLE;
    let Some(mut d) = opened("list.html") else { return };
    let s = d.screen().unwrap();
    assert!(s.elements[0].has(SCROLLABLE) && &*s.elements[0].class == "html", "the page scrolls: {:?}", s.elements[0]);
    let row = s.elements.iter().position(|e| e.id == "row-20").unwrap();
    assert_eq!(s.vertical_list_around(row), Some(0), "its rows are in it");
    let Some(mut d) = opened("form.html") else { return };
    assert!(!d.screen().unwrap().elements[0].has(SCROLLABLE), "a page that fits does not");
}


#[test]
fn a_swipe_on_something_that_takes_pointer_drags_drags_it_as_maestro_does() {
    // Maestro's web swipe is a finger's: it drags what takes pointer drags (touch-action: none)
    // and scrolls the rest.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    d.swipe((x, y), (x + 200, y + 100), 400, Lift::Still).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Knob at 220,120");
}

#[test]
fn back_on_a_tab_with_no_history_closes_it_and_comes_back_as_mobile_chrome_does() {
    use virtuoso::device::Key;
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "popup");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(10)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Second tab".to_string()));
    d.press_key(Key::Back).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(10)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Nothing yet".to_string()), "back on the first tab");
}


const BOXES: &str = r#"<!doctype html><title>Boxes</title>
<input id="b0" maxlength="1"><input id="b1" maxlength="1"><input id="b2" maxlength="1"><p id="keys"></p>
<script>
const b = [0, 1, 2].map((i) => document.getElementById('b' + i));
b.forEach((el, i) => el.addEventListener('input', () => { if (el.value && b[i + 1]) b[i + 1].focus(); }));
addEventListener('keydown', (e) => document.getElementById('keys').textContent += e.key);
</script>"#;

#[test]
fn text_is_typed_a_key_at_a_time_as_a_keyboard_types_it() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(BOXES), false);
    let (x, y) = centre(&mut d, "b0");
    d.tap(x, y).unwrap();
    d.input_text("123").unwrap();
    assert_eq!(d.evaluate("b.map((el) => el.value).join(',')").unwrap(), json!("1,2,3"), "focus moved with each key");
    assert_eq!(d.evaluate("document.getElementById('keys').textContent").unwrap(), json!("123"), "each a key the page heard");
}

#[test]
fn an_emulated_phone_has_its_size_touch_and_user_agent_in_every_tab() {
    let Some(mut d) = device() else { return };
    d.emulate(virtuoso::web::emulation("pixel-7").unwrap()).unwrap();
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/dialogs.html"), false);
    let check = |d: &mut WebDevice, tab: &str| {
        assert_eq!((num(d, "innerWidth"), num(d, "innerHeight")), (412.0, 839.0), "{tab}");
        assert!(d.evaluate("navigator.userAgent").unwrap().as_str().unwrap().contains("Pixel 7"), "{tab}");
        assert!(num(d, "navigator.maxTouchPoints") > 0.0, "{tab}: touch");
        assert_eq!(d.evaluate("matchMedia('(pointer: coarse)').matches").unwrap(), json!(true), "{tab}: a finger, not a mouse");
    };
    check(&mut d, "first tab");
    let (x, y) = centre(&mut d, "newtab");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
    assert!(d.screen().unwrap().elements.iter().any(|e| e.text == "Second tab"));
    check(&mut d, "a tab the page opened");
}

#[test]
fn a_launched_chrome_has_a_desktop_window_of_1280_by_800() {
    let Some(mut d) = opened("form.html") else { return };
    assert_eq!((num(&mut d, "outerWidth"), num(&mut d, "outerHeight")), (1280.0, 800.0));
}

/// Chrome on the Android device VIRTUOSO_WEB_ANDROID names (a serial), else no test.
fn android_chrome() -> Option<WebDevice> {
    let serial = std::env::var("VIRTUOSO_WEB_ANDROID").ok().filter(|s| !s.is_empty())?;
    Some(virtuoso::web::connect(&format!("android:{serial}")).unwrap())
}

#[test]
fn chrome_on_an_android_device_opens_a_page_this_machine_serves_and_takes_taps() {
    let Some(mut d) = android_chrome() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/index.html"), true);
    assert!(d.evaluate("navigator.userAgent").unwrap().as_str().unwrap().contains("Android"), "the device's own Chrome");
    // A tap on the link (by touch on the phone's own screen) opens the form.
    let (x, y) = centre(&mut d, "nav-form");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "location.pathname.endsWith('/form.html')");
}

#[test]
fn the_part_of_the_page_the_phones_keyboard_covers_is_not_on_screen() {
    let Some(mut d) = android_chrome() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/form.html"), true);
    let full = d.screen().unwrap().height;
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "visualViewport.height < innerHeight - 100");
    let s = d.screen().unwrap();
    assert!(s.height < full - 100, "the keyboard's part is off the screen: {} of {full}", s.height);
}

#[test]
fn on_a_phone_a_tap_after_typing_is_a_click_on_its_target() {
    // A phone's tap is a finger's: a mouse press there takes the typed field's focus, the page
    // shifts back from it, and the release and its click fall elsewhere. And Chrome makes the
    // click of a touch a little after it: a tap is done once the page has had it, or what the
    // flow does next comes first and the click is lost.
    let Ok(serial) = std::env::var("VIRTUOSO_WEB_ANDROID") else { return };
    if serial.is_empty() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow_on(
        Some(&format!("android:{serial}")),
        &format!(
            "url: {url}/focusshift.html\n---\n- launchApp:\n    clearState: true\n- tapOn: Full name\n- pressKey: Tab\n- inputText: Maria Novak\n- pressKey: Tab\n- inputText: maria.novak@example.com\n- pressKey: Tab\n- inputText: +48 601 222 333\n- tapOn: Next\n- assertVisible: Pressed\n"
        ),
    );
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn with_the_visible_part_of_the_page_moved_a_tap_lands_where_the_screen_says() {
    // A phone's keyboard moves what shows of the page (the visual viewport) to keep its field in
    // sight; a pinch zoom moves it the same way, on any Chrome.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/keyboard.html"), true);
    d.devtools("Input.synthesizePinchGesture", json!({"x": 640, "y": 700, "scaleFactor": 2.0, "relativeSpeed": 800})).unwrap();
    wait_for(&mut d, "visualViewport.offsetTop > 50");
    let (x, y) = centre(&mut d, "go");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('status').textContent === 'Pressed'");
}
