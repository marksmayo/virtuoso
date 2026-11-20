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

/// Each exec request's command line, in order.
fn execs(seen: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|r| r["op"] == "exec")
        .map(|r| r["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap_or("")).collect::<Vec<_>>().join(" "))
        .collect()
}

/// A stand-in agent's answer that every request takes: the command ran, and the app is in front.
fn ran(_: &Value) -> Value {
    let mut r = screen(1);
    r["code"] = json!(0);
    r["out"] = json!("");
    r["windows"] = json!([{"pkg": "com.app", "type": 1, "active": true, "b": [0, 0, 1080, 2400]}]);
    r
}

#[test]
fn a_launch_that_clears_the_app_sets_its_permissions_after_the_clear() {
    // `pm clear` revokes an app's runtime permissions: set before it (as they were), the flow's
    // permissions were undone by the clear itself.
    use virtuoso::device::LaunchOptions;
    let (mut d, seen) = device(ran);
    assert!(d.launch_sets_permissions(), "the launch sets them, in their place");
    let o = LaunchOptions { clear_state: true, stop_app: true, permissions: vec![("camera".into(), "allow".into())], ..Default::default() };
    d.launch_app("com.app", &o).unwrap();
    let execs = execs(&seen);
    let clear = execs.iter().position(|e| e.contains("cmd package clear com.app")).unwrap_or_else(|| panic!("{execs:#?}"));
    let grant = execs.iter().position(|e| e.contains("grant com.app android.permission.CAMERA")).unwrap_or_else(|| panic!("{execs:#?}"));
    let start = execs.iter().position(|e| e.contains("cmd activity start")).unwrap_or_else(|| panic!("{execs:#?}"));
    assert!(clear < grant && grant < start, "{execs:#?}");
    assert!(!execs[start].contains("cmd package clear"), "not cleared again after the grant: {}", execs[start]);
}

#[test]
fn a_launch_that_keeps_the_app_sets_its_permissions_before_the_start() {
    use virtuoso::device::LaunchOptions;
    let (mut d, seen) = device(ran);
    let o = LaunchOptions { stop_app: true, permissions: vec![("camera".into(), "deny".into())], ..Default::default() };
    d.launch_app("com.app", &o).unwrap();
    let execs = execs(&seen);
    let revoke = execs.iter().position(|e| e.contains("revoke com.app android.permission.CAMERA")).unwrap_or_else(|| panic!("{execs:#?}"));
    let start = execs.iter().position(|e| e.contains("cmd activity start")).unwrap_or_else(|| panic!("{execs:#?}"));
    assert!(revoke < start && !execs.iter().any(|e| e.contains("cmd package clear")), "{execs:#?}");
}

#[test]
fn a_launch_whose_clear_fails_says_so_and_does_not_start_the_app() {
    use virtuoso::device::LaunchOptions;
    let (mut d, seen) = device(|req| {
        let mut r = ran(req);
        if req["argv"].as_array().is_some_and(|a| a.iter().any(|x| x == "clear")) {
            r["code"] = json!(1);
            r["out"] = json!("Failed\n");
        }
        r
    });
    let o = LaunchOptions { clear_state: true, stop_app: true, permissions: vec![("camera".into(), "allow".into())], ..Default::default() };
    let e = d.launch_app("com.app", &o).unwrap_err().to_string();
    assert_eq!(e, "clearing com.app failed: Failed");
    assert!(!execs(&seen).iter().any(|e| e.contains("cmd activity start")), "{:#?}", execs(&seen));
}

#[test]
fn a_launch_with_a_permission_virtuoso_does_not_know_says_so_and_does_not_start_the_app() {
    use virtuoso::device::LaunchOptions;
    let (mut d, seen) = device(ran);
    let o = LaunchOptions { stop_app: true, permissions: vec![("teleportation".into(), "allow".into())], ..Default::default() };
    let e = d.launch_app("com.app", &o).unwrap_err().to_string();
    assert!(e.contains("`teleportation` is not a permission virtuoso knows"), "{e}");
    assert!(!execs(&seen).iter().any(|e| e.contains("cmd activity start")), "{:#?}", execs(&seen));
}

#[test]
fn a_launch_without_permissions_is_its_one_script_granting_every_permission() {
    // As in Maestro: every permission, granted in the launch's own script, nothing set apart.
    use virtuoso::device::LaunchOptions;
    let (mut d, seen) = device(ran);
    d.launch_app("com.app", &LaunchOptions { clear_state: true, stop_app: true, ..Default::default() }).unwrap();
    let execs = execs(&seen);
    assert_eq!(execs.len(), 1, "{execs:#?}");
    assert!(execs[0].contains("cmd package clear com.app") && execs[0].contains("cmd package grant com.app $p") && execs[0].contains("cmd activity start"), "{}", execs[0]);
}
