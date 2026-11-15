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

