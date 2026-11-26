//! The Android device traced (VIRTUOSO_DEBUG), against a stand-in agent. A test binary of its
//! own: the device reads the variable once per process, and every test here sets it first.

use serde_json::{json, Value};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use virtuoso::agent::{read_frame, write_frame, AgentClient};
use virtuoso::android::{AndroidAgent, Display};
use virtuoso::android_device::AndroidDevice;
use virtuoso::device::{Device, LaunchOptions};

fn traced() {
    std::env::set_var("VIRTUOSO_DEBUG", "1");
}

/// A stand-in agent answering each request with `reply`, and the requests it got: `start` from
/// now, handed over as a device's agent start still under way would be.
fn agent(reply: fn(&Value) -> Value, start: Duration) -> (thread::JoinHandle<anyhow::Result<AndroidAgent>>, Arc<Mutex<Vec<Value>>>) {
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
        Ok(AndroidAgent::stand_in(AgentClient::connect(addr, "t")?, Display { width: 1080, height: 2400, rotation: 0, sdk: 34 }))
    });
    (started, seen)
}

/// A device on a stand-in agent already up.
fn device(reply: fn(&Value) -> Value) -> (AndroidDevice, Arc<Mutex<Vec<Value>>>) {
    let (started, seen) = agent(reply, Duration::ZERO);
    (AndroidDevice::stand_in(started.join().unwrap().unwrap()), seen)
}

/// An adb server whose shell prints `output`; the requests it got.
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
                let mut body = vec![0u8; usize::from_str_radix(std::str::from_utf8(&len).unwrap(), 16).unwrap()];
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

const LAUNCH_OUT: &str = "phase begin 1.000\nphase list 1.100\nphase clear 1.200\nphase grant 1.300\nTotalTime: 500\nphase start 1.900\n";

/// The app is up: the launch's exec printed its marks, and its window is the active one.
fn up(req: &Value) -> Value {
    let mut r = json!({"ok": true, "seq": 1, "status": 0, "changed": true, "method": "events", "events": 3, "sources": {"com.app": 3},
        "windows": [{"pkg": "com.app", "type": 1, "active": true}], "nodes": []});
    if req["op"] == "exec" {
        r["code"] = json!(0);
        r["out"] = json!(LAUNCH_OUT);
    }
    r
}

#[test]
fn a_traced_launch_sends_its_script_with_phase_marks() {
    traced();
    let (mut d, seen) = device(up);
    d.launch_app("com.app", &LaunchOptions { clear_state: true, stop_app: true, ..Default::default() }).unwrap();
    let reqs = seen.lock().unwrap().clone();
    let exec = reqs.iter().find(|r| r["op"] == "exec").unwrap_or_else(|| panic!("{reqs:#?}"));
    let script = exec["argv"][2].as_str().unwrap();
    assert!(script.contains("echo \"phase start $EPOCHREALTIME\"") && script.ends_with("exit $s"), "{script}");
}

#[test]
fn a_traced_launch_while_the_agent_starts_marks_its_phases_over_adb_too() {
    traced();
    let (start, _) = agent(up, Duration::from_millis(50));
    let out: &'static str = Box::leak(format!("{LAUNCH_OUT}exit 0\n").into_boxed_str());
    let (port, adb) = fake_adb(out);
    let mut d = AndroidDevice::stand_in_starting(start, port, false);
    d.launch_app("com.app", &LaunchOptions { stop_app: true, ..Default::default() }).unwrap();
    let adb = adb.lock().unwrap().clone();
    let shell = adb.iter().find(|r| r.starts_with("shell:")).expect("a shell");
    assert!(shell.contains("echo \"phase begin $EPOCHREALTIME\""), "{shell}");
}

#[test]
fn a_traced_settle_that_brings_the_screen_still_brings_it() {
    traced();
    let (mut d, _) = device(up);
    d.next_step_reads(true);
    let settled = d.settle(Duration::from_millis(300), Duration::from_secs(2)).unwrap();
    assert!(settled.changed, "{settled:?}");
    assert_eq!(d.screen().unwrap().elements.len(), 0);
}
