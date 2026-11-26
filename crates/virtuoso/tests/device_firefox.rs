//! Tests against a real Firefox: run with VIRTUOSO_WEB=1 (and VIRTUOSO_FIREFOX=<path> where
//! Firefox is not installed in the usual place). Without it they pass without running.

use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use virtuoso::firefox::{find_firefox, Firefox};

fn firefox() -> Option<std::path::PathBuf> {
    if std::env::var("VIRTUOSO_WEB").is_err() {
        return None;
    }
    Some(find_firefox().expect("Firefox, for VIRTUOSO_WEB"))
}

/// A server on a free port that answers every request with `page`: its base URL.
fn serve(page: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_page_opened_in_firefox_is_read_as_a_screen_and_tapped() {
    use virtuoso::device::{Device, LaunchOptions};
    let Some(path) = firefox() else { return };
    let mut d = virtuoso::web::WebDevice::launch_firefox(&path, true).unwrap();
    let url = serve("<button onclick=\"this.textContent='Pressed'\" style=\"position:absolute;left:10px;top:10px;width:120px;height:40px\">Press me</button>");
    d.launch_app(&url, &LaunchOptions { clear_state: true, stop_app: true, ..Default::default() }).unwrap();
    let s = d.screen().unwrap();
    let b = s.elements.iter().find(|e| e.text == "Press me").unwrap_or_else(|| panic!("the button on the screen: {:?}", s.elements.iter().map(|e| &e.text).collect::<Vec<_>>()));
    let (x, y) = ((b.bounds.left + b.bounds.right) / 2, (b.bounds.top + b.bounds.bottom) / 2);
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(10), std::time::Duration::from_secs(5)).unwrap();
    let s = d.screen().unwrap();
    assert!(s.elements.iter().any(|e| e.text == "Pressed"), "{:?}", s.elements.iter().map(|e| &e.text).collect::<Vec<_>>());
}

#[test]
fn a_stopped_firefox_leaves_none_of_its_processes_behind() {
    // Each process Firefox starts is ended with it: one left running would hold the profile's
    // files (and pile up over a suite, slowing every later launch).
    let Some(path) = firefox() else { return };
    for _ in 0..3 {
        let b = virtuoso::chrome::Browser::firefox(&path, true).unwrap();
        let profile = b.profile().expect("a launched Firefox's profile").to_path_buf();
        assert!(profile.exists());
        drop(b);
        assert!(!profile.exists(), "the profile is still held after the stop: {}", profile.display());
    }
}

#[test]
fn firefoxes_launched_together_each_open_their_session() {
    // Tests run side by side, each with a Firefox of its own: none may hold up another's start.
    let Some(path) = firefox() else { return };
    let started = std::time::Instant::now();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                let t = std::time::Instant::now();
                let r = virtuoso::chrome::Browser::firefox(&path, true).map(|_| ());
                (r.map_err(|e| e.to_string()), t.elapsed())
            })
        })
        .collect();
    for h in handles {
        let (r, took) = h.join().unwrap();
        assert!(r.is_ok(), "a Firefox launched beside three others: {r:?} after {took:?}");
        // (Four Firefoxes starting at once on a busy machine take their time: a hang is what is caught.)
        assert!(took < std::time::Duration::from_secs(45), "a Firefox launched beside three others took {took:?}");
    }
    println!("four Firefoxes launched together in {:?}", started.elapsed());
}

#[test]
fn a_launched_firefox_is_headless_answers_and_opens_a_page() {
    let Some(path) = firefox() else { return };
    let mut f = Firefox::launch(&path, true).unwrap();
    assert_eq!(f.capabilities["moz:headless"], json!(true), "headless: {}", f.capabilities);
    assert_eq!(f.capabilities["browserName"], json!("firefox"), "{}", f.capabilities);
    let tree = f.bidi.call(None, "browsingContext.getTree", json!({})).unwrap();
    let context = tree["contexts"][0]["context"].as_str().expect("a tab").to_string();
    let url = serve("<title>hello</title><p>hi</p>");
    f.bidi.call(None, "browsingContext.navigate", json!({"context": context, "url": url, "wait": "complete"})).unwrap();
    let r = f.bidi.call(None, "script.evaluate", json!({"expression": "document.title", "target": {"context": context}, "awaitPromise": false})).unwrap();
    assert_eq!(r["result"]["value"], json!("hello"), "{r}");
}
