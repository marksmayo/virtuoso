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

#[test]
fn typing_followed_by_more_typing_goes_without_a_settle_between_them() {
    // Keys go to the focused window in order: typing, an erase and typing again need no wait for
    // quiet between them (the web device does the same). The last typing step keeps its settle.
    let (mut d, seen) = device(|_| json!({"ok": true, "seq": 1, "settled": true, "skipped": 0}));
    let quiet = Duration::from_millis(100);
    let timeout = Duration::from_secs(5);
    d.next_step_types(true);
    d.input_text("display").unwrap();
    d.settle(quiet, timeout).unwrap();
    d.next_step_types(true);
    d.erase_text(20).unwrap();
    d.settle(quiet, timeout).unwrap();
    d.next_step_types(false);
    d.input_text("wifi").unwrap();
    d.settle(quiet, timeout).unwrap();
    let reqs = seen.lock().unwrap().clone();
    let ops: Vec<(String, bool)> = reqs.iter().filter(|r| r["op"] != "hello").map(|r| (r["op"].as_str().unwrap_or("").to_string(), r.get("then").is_some())).collect();
    assert_eq!(ops, [("type".to_string(), false), ("erase".to_string(), false), ("type".to_string(), true)], "{reqs:#?}");
}

#[test]
fn a_tap_with_typing_next_settles_until_a_field_has_input_focus() {
    // The typing step waits for input focus itself: the tap's settle need not also wait out
    // the keyboard window's coming up. Without typing next the settle is as before.
    let (mut d, seen) = device(|req| {
        let mut r = screen(3);
        if req.get("tapIf").is_some() {
            r["tapped"] = json!(true);
            r["status"] = json!(0);
            r["changed"] = json!(true);
        }
        r
    });
    let quiet = Duration::from_millis(300);
    let timeout = Duration::from_secs(5);
    let tree = d.screen().unwrap();
    d.next_step_types(true);
    d.tap_if_unchanged_and_settle(&tree, 10, 15, quiet, timeout).unwrap().unwrap();
    d.next_step_types(false);
    d.tap_if_unchanged_and_settle(&tree, 10, 15, quiet, timeout).unwrap().unwrap();
    let reqs = seen.lock().unwrap().clone();
    let taps: Vec<&Value> = reqs.iter().filter(|r| r.get("tapIf").is_some()).collect();
    assert_eq!(taps[0]["then"]["focus"], true, "{reqs:#?}");
    assert!(taps[1]["then"].get("focus").is_none(), "{reqs:#?}");
}

#[test]
fn a_launch_with_a_lookup_next_settles_only_briefly_once_its_window_is_up() {
    // The lookup waits for its element on its own, event by event: the launch need not wait
    // for the whole app to go quiet first. Anything else next keeps the launch's full settle.
    let (mut d, seen) = device(|req| match req["op"].as_str() {
        Some("exec") => json!({"ok": true, "code": 0, "out": "launcher com.app/.Main
", "seq": 1, "windows": [{"pkg": "com.app", "type": 1, "active": true}]}),
        _ => {
            let mut r = screen(3);
            r["status"] = json!(0);
            r["changed"] = json!(true);
            r
        }
    });
    let o = virtuoso::device::LaunchOptions::default();
    d.next_step_reads(true);
    d.launch_app("com.app", &o).unwrap();
    d.next_step_reads(false);
    d.launch_app("com.app", &o).unwrap();
    let reqs = seen.lock().unwrap().clone();
    let settles: Vec<u64> = reqs.iter().filter(|r| r["op"] == "settle").map(|r| r["timeout"].as_u64().unwrap()).collect();
    assert_eq!(settles.len(), 2, "{reqs:#?}");
    assert!(settles[0] <= 300, "{settles:?}");
    assert_eq!(settles[1], 10_000);
}

#[test]
fn a_settle_before_a_step_that_waits_for_a_text_may_end_once_the_screen_shows_it() {
    let (mut d, seen) = device(|req| {
        let mut r = screen(3);
        r["tapped"] = json!(true);
        r["status"] = json!(0);
        r["changed"] = json!(true);
        let _ = req;
        r
    });
    let quiet = Duration::from_millis(300);
    let timeout = Duration::from_secs(5);
    let tree = d.screen().unwrap();
    d.next_step_reads(true);
    d.next_step_expects(Some("Internet"));
    d.tap_if_unchanged_and_settle(&tree, 10, 15, quiet, timeout).unwrap().unwrap();
    d.next_step_expects(None);
    d.tap_if_unchanged_and_settle(&tree, 10, 15, quiet, timeout).unwrap().unwrap();
    let reqs = seen.lock().unwrap().clone();
    let taps: Vec<&Value> = reqs.iter().filter(|r| r.get("tapIf").is_some()).collect();
    assert_eq!(taps[0]["then"]["until"], "Internet", "{reqs:#?}");
    assert!(taps[1]["then"].get("until").is_none(), "{reqs:#?}");
}

#[test]
fn an_open_link_settles_once_its_window_is_active_and_quiet_100_ms() {
    // A deep link into the app: its settle may end once the app's window is in front and no
    // event has come for 100 ms, without waiting for the app's main thread to go idle too (a
    // React Native screen still loading kept it busy 0.15-2.4 s past its last event on Windows).
    // A browser link, or one with no app, keeps the usual settle; so does the settle after it.
    let (mut d, seen) = device(ran);
    let (quiet, timeout) = (Duration::from_millis(300), Duration::from_secs(10));
    d.open_link_and_settle("bench://product/7", Some("com.app"), false, quiet, timeout).unwrap();
    d.open_link_and_settle("https://example.com", Some("com.app"), true, quiet, timeout).unwrap();
    d.open_link_and_settle("bench://cart", None, false, quiet, timeout).unwrap();
    d.press_key(virtuoso::device::Key::Back).unwrap();
    d.settle(quiet, timeout).unwrap();
    let reqs = seen.lock().unwrap().clone();
    let settles: Vec<&Value> = reqs.iter().filter_map(|r| r.get("then")).collect();
    assert_eq!(settles.len(), 4, "{reqs:#?}");
    assert_eq!(settles[0]["link"], "com.app", "{reqs:#?}");
    assert!(settles[1..].iter().all(|s| s.get("link").is_none()), "{reqs:#?}");
}

/// A stand-in agent answering each request with `reply`, as `device` makes one, but handed over
/// by a start still under way (the device's agent comes up in the background).
fn starting_agent(reply: fn(&Value) -> Value, start: Duration) -> (std::thread::JoinHandle<anyhow::Result<AndroidAgent>>, Arc<Mutex<Vec<Value>>>) {
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
    let started = thread::spawn(move || {
        thread::sleep(start);
        let client = AgentClient::connect(addr, "t")?;
        Ok(AndroidAgent::stand_in(client, Display { width: 1080, height: 2400, rotation: 0, sdk: 34 }))
    });
    (started, seen)
}

/// An adb server that answers every request OKAY, and a shell with `output`; the requests it got.
fn fake_adb(output: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { return };
            loop {
                let mut len = [0u8; 4];
                if s.read_exact(&mut len).is_err() {
                    break;
                }
                let n = usize::from_str_radix(std::str::from_utf8(&len).unwrap(), 16).unwrap();
                let mut body = vec![0u8; n];
                s.read_exact(&mut body).unwrap();
                let body = String::from_utf8(body).unwrap();
                log.lock().unwrap().push(body.clone());
                s.write_all(b"OKAY").unwrap();
                if body.starts_with("shell:") {
                    s.write_all(output.as_bytes()).unwrap();
                    break;
                }
            }
        }
    });
    (port, seen)
}

fn launched(req: &Value) -> Value {
    match req["op"].as_str() {
        Some("windows") => json!({"ok": true, "seq": 1, "windows": [{"pkg": "com.app", "type": 1, "active": true}]}),
        _ => json!({"ok": true, "seq": 1, "status": 0, "changed": true}),
    }
}

#[test]
fn a_device_whose_agent_is_still_starting_waits_for_it_at_its_first_request() {
    let (start, seen) = starting_agent(|_| screen(3), Duration::from_millis(100));
    let (port, _) = fake_adb("");
    let mut d = AndroidDevice::stand_in_starting(start, port, false);
    // The display is the agent's: known once it is up.
    let rect = d.screen_rect().unwrap();
    assert_eq!((rect.right, rect.bottom), (1080, 2400));
    let fresh = d.fresh_screen().unwrap();
    assert_eq!(fresh.elements.len(), 3);
    let again = d.screen_after_change(fresh, Duration::from_millis(50)).unwrap();
    assert_eq!(again.elements.len(), 3);
    assert_eq!(seen.lock().unwrap().len(), 2, "two reads, no request to start with");
}

#[test]
fn an_agent_that_did_not_start_says_so_at_every_request() {
    let start = thread::spawn(|| -> anyhow::Result<AndroidAgent> { Err(anyhow::anyhow!("no device called stand-in")) });
    let (port, _) = fake_adb("");
    let mut d = AndroidDevice::stand_in_starting(start, port, false);
    for _ in 0..2 {
        let e = d.screen().unwrap_err().to_string();
        assert!(e.contains("the agent did not start") && e.contains("no device called stand-in"), "{e}");
    }
}

#[test]
fn a_launch_while_the_agent_starts_goes_over_adb_with_animations_off_first() {
    // Side by side with the agent's start: the launch script in a shell of its own, its exit
    // status read from its last line; then the agent looks for the app's window and settles.
    let (start, seen) = starting_agent(launched, Duration::from_millis(50));
    let (port, adb) = fake_adb("Starting: Intent\nlauncher com.app/.Main\nexit 0\n");
    let mut d = AndroidDevice::stand_in_starting(start, port, true);
    d.launch_app("com.app", &virtuoso::device::LaunchOptions { stop_app: true, ..Default::default() }).unwrap();
    let adb = adb.lock().unwrap().clone();
    assert!(adb.iter().any(|r| r == "host:transport:stand-in"), "{adb:?}");
    let shell = adb.iter().find(|r| r.starts_with("shell:")).expect("a shell");
    assert!(shell.contains("settings put global") && shell.contains("cmd activity start"), "animations off, then the start: {shell}");
    let ops: Vec<String> = seen.lock().unwrap().iter().map(|r| r["op"].as_str().unwrap_or("").to_string()).collect();
    assert!(!ops.contains(&"exec".to_string()) && ops.contains(&"settle".to_string()), "{ops:?}");
}

#[test]
fn a_launch_that_fails_while_the_agent_starts_says_why() {
    let (start, _) = starting_agent(launched, Duration::ZERO);
    let (port, _) = fake_adb("Error: Activity not started\nexit 1\n");
    let mut d = AndroidDevice::stand_in_starting(start, port, false);
    let e = d.launch_app("com.app", &virtuoso::device::LaunchOptions::default()).unwrap_err().to_string();
    assert!(e.contains("launching com.app failed") && e.contains("Activity not started"), "{e}");
}

#[test]
fn a_device_dropped_while_its_agent_starts_waits_for_the_start() {
    // The start finishes and its agent goes with the device: left to start on its own, it would
    // hold the UiAutomation the next agent asks for.
    let (start, _) = starting_agent(|_| screen(1), Duration::from_millis(200));
    let (port, _) = fake_adb("");
    let t = std::time::Instant::now();
    drop(AndroidDevice::stand_in_starting(start, port, false));
    assert!(t.elapsed() >= Duration::from_millis(190), "{:?}", t.elapsed());
}

#[test]
fn connecting_to_a_device_that_is_not_there_returns_at_once_and_its_first_request_says_why() {
    let t = std::time::Instant::now();
    let mut d = AndroidDevice::connect_with("no-such-device-virtuoso", false).unwrap();
    assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
    let e = d.screen().unwrap_err().to_string();
    assert!(e.contains("the agent did not start"), "{e}");
}

#[test]
fn turning_animations_off_writes_only_the_scales_not_off_yet() {
    let script = virtuoso::android::animations_off_script();
    for key in ["window_animation_scale", "transition_animation_scale", "animator_duration_scale"] {
        assert!(script.contains(key), "{script}");
    }
    assert!(script.contains("cmd settings list global") && script.contains("cmd settings put global $k 0"), "{script}");
}

#[test]
fn an_agent_lost_mid_request_is_started_again_and_a_failed_restart_says_so() {
    // The stand-in drops the connection on a read; its device has no adb behind it, so the new
    // start fails too, and the error says both.
    let (mut d, _) = device(|req| if req["op"] == "screen" { panic!("the agent went away") } else { json!({"ok": true}) });
    let e = format!("{:#}", d.screen().unwrap_err());
    assert!(e.contains("restarting the agent failed"), "{e}");
}

#[test]
fn a_launch_whose_shell_says_only_its_exit_status_is_read_too() {
    let (start, _) = starting_agent(launched, Duration::ZERO);
    let (port, _) = fake_adb("exit 4");
    let mut d = AndroidDevice::stand_in_starting(start, port, false);
    let e = d.launch_app("com.app", &virtuoso::device::LaunchOptions::default()).unwrap_err().to_string();
    assert!(e.contains("launching com.app failed (4)"), "{e}");
}

#[test]
fn a_launch_while_the_agent_starts_uses_adb_itself_when_its_server_does_not_answer() {
    // No adb server on that port: the launch goes through `adb shell`, which, for a device
    // that is not there, fails and says so.
    let (start, _) = starting_agent(launched, Duration::ZERO);
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut d = AndroidDevice::stand_in_starting(start, closed, false);
    assert!(d.launch_app("com.app", &virtuoso::device::LaunchOptions::default()).is_err());
}

#[test]
fn an_agent_lost_during_a_settle_is_started_again_and_a_failed_restart_says_so() {
    let (mut d, _) = device(|req| if req["op"] == "settle" { panic!("the agent went away") } else { json!({"ok": true}) });
    let e = format!("{:#}", d.settle(Duration::from_millis(100), Duration::from_secs(1)).unwrap_err());
    assert!(e.contains("restarting the agent failed"), "{e}");
}

#[test]
fn a_screenshot_reply_without_an_image_is_an_error() {
    let (mut d, _) = device(|_| json!({"ok": true}));
    let e = d.screenshot().unwrap_err().to_string();
    assert!(e.contains("the agent sent no screenshot"), "{e}");
}
