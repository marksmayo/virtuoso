//! The Android device against a stand-in agent on a local socket: no adb, no device. What it
//! asks the agent for, and what it makes of the replies.

use serde_json::{json, Value};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use virtuoso::agent::{read_frame, write_frame, AgentClient};
use virtuoso::android::{AndroidAgent, Display};
use virtuoso::android_device::AndroidDevice;
use virtuoso::device::Device;

/// A screen of `n` rows, as the agent sends it.
fn screen(n: usize) -> Value {
    let nodes: Vec<Value> = (0..n).map(|i| json!({"d": 1, "w": 0, "b": [0, i * 10, 1080, i * 10 + 10], "f": 34, "k": "android.widget.TextView", "t": format!("Row {i}")})).collect();
    json!({"ok": true, "seq": 1, "windows": [{"pkg": "com.app", "type": 1}], "nodes": nodes})
}

/// A device on a stand-in agent that answers each request with `reply`; the requests it got.
fn device(reply: fn(&Value) -> Value) -> (AndroidDevice, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let _hello = read_frame(&mut s).unwrap();
        while let Ok(req) = read_frame(&mut s) {
            log.lock().unwrap().push(req.clone());
            if write_frame(&mut s, &reply(&req)).is_err() {
                return;
            }
        }
    });
    let client = AgentClient::connect(addr, "t").unwrap();
    let agent = AndroidAgent::stand_in(client, Display { width: 1080, height: 2400, rotation: 0, sdk: 34 });
    (AndroidDevice::stand_in(agent), seen)
}

#[test]
fn a_screen_read_comes_from_the_agent_and_its_buffer_is_kept_for_the_next() {
    let (mut d, seen) = device(|_| screen(200));
    let tree = d.screen().unwrap();
    assert_eq!(tree.elements.len(), 200);
    assert_eq!(tree.elements[3].text, "Row 3");
    assert_eq!(seen.lock().unwrap()[0]["op"], "screen");
    // The reply's bytes go back to the client: the next ~100-300 KB reply is read into them.
    assert!(d.spare_buffer() > 10_000, "kept: {}", d.spare_buffer());
}

#[test]
fn a_checked_tap_brings_its_settles_outcome_with_the_screen() {
    let (mut d, seen) = device(|req| {
        let mut r = screen(3);
        if req.get("tapIf").is_some() {
            r["tapped"] = json!(true);
            r["status"] = json!(2);
            r["changed"] = json!(true);
        }
        r
    });
    let first = d.screen().unwrap();
    let settled = d.tap_if_unchanged_and_settle(&first, 5, 5, Duration::from_millis(300), Duration::from_secs(2)).unwrap().unwrap();
    assert!(settled.changed && settled.timed_out, "{settled:?}");
    let req = seen.lock().unwrap()[1].clone();
    assert_eq!((req["fresh"].clone(), req["tapIf"]["x"].clone(), req["then"]["op"].clone()), (json!(true), json!(5), json!("settle")));
}
