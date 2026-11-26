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
