//! Tests against a real Safari, through safaridriver (macOS only; `sudo safaridriver --enable`
//! once):
//! - with VIRTUOSO_WEB=1, Safari on the Mac;
//! - with VIRTUOSO_SAFARI_IOS=<a booted simulator's UDID>, the spike: can safaridriver drive
//!   Safari in the iOS Simulator? It prints what happened (`SPIKE …`; run with --nocapture).
//!
//! Elsewhere, or without them, they pass without running.

use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use virtuoso::safari::{find_safaridriver, ios_simulator_capabilities, Safari};

const PAGE: &str = "<!doctype html><title>Safari page</title><p>hello";

/// A server on a free port that answers every request with `page`: its base URL.
fn serve(page: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                // The request's head, to its blank line.
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
    format!("http://127.0.0.1:{port}/")
}

#[test]
fn a_launched_safari_answers_and_opens_a_page() {
    if !cfg!(target_os = "macos") || std::env::var("VIRTUOSO_WEB").is_err() {
        return;
    }
    let url = serve(PAGE);
    let safari = Safari::launch().expect("Safari through safaridriver (`sudo safaridriver --enable` once)");
    println!("Safari: {}", safari.capabilities);
    safari.driver.navigate(&url).unwrap();
    assert_eq!(safari.driver.execute("return document.title", json!([])).unwrap(), json!("Safari page"));
    let sum = safari.driver.execute_async("arguments[2](arguments[0] + arguments[1])", json!([2, 3])).unwrap();
    assert_eq!(sum, json!(5));
}

#[test]
fn spike_safari_in_the_ios_simulator() {
    let Ok(udid) = std::env::var("VIRTUOSO_SAFARI_IOS") else { return };
    if !cfg!(target_os = "macos") {
        return;
    }
    let caps = ios_simulator_capabilities(&udid);
    println!("SPIKE capabilities asked for: {caps}");
    let t = std::time::Instant::now();
    let safari = match Safari::launch_with(&find_safaridriver(), caps) {
        Ok(s) => s,
        Err(e) => {
            println!("SPIKE no session after {:?}: {e:#}", t.elapsed());
            panic!("SPIKE failed: no session: {e:#}");
        }
    };
    println!("SPIKE session after {:?}: {}", t.elapsed(), safari.capabilities);
    // The simulator shares the Mac's network: its 127.0.0.1 is the Mac's.
    let url = serve(PAGE);
    let t = std::time::Instant::now();
    match safari.driver.navigate(&url) {
        Ok(()) => println!("SPIKE navigated to {url} in {:?}", t.elapsed()),
        Err(e) => panic!("SPIKE failed: navigating to {url}: {e:#}"),
    }
    match safari.driver.execute("return [document.title, navigator.userAgent, innerWidth, innerHeight]", json!([])) {
        Ok(v) => {
            println!("SPIKE title, user agent, viewport: {v}");
            assert_eq!(v[0], "Safari page");
            println!("SPIKE works: safaridriver drives Safari in the iOS Simulator");
        }
        Err(e) => panic!("SPIKE failed: execute: {e:#}"),
    }
}
