//! The iOS device's host side against stand-ins: an agent on a local port, and a simctl that
//! succeeds and does nothing (DEVELOPER_DIR names a folder with it), so what reaches each can be
//! counted with no simulator. One test at a time: they share the process's simctl and its count.

use serde_json::Value;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use virtuoso::device::Device;
use virtuoso::ios::{simctl_runs, IosDevice};

/// The process's do-nothing simctl, set up once; held for the test's length.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    static SETUP: std::sync::Once = std::sync::Once::new();
    SETUP.call_once(|| {
        let dev = std::env::temp_dir().join(format!("virtuoso-fake-xcode-{}", std::process::id()));
        let bin = dev.join("usr/bin");
        std::fs::create_dir_all(&bin).unwrap();
        if cfg!(windows) {
            // `simctl` must be a file; a path with no extension runs `simctl.exe` beside it. cmd
            // with no /c reads its (empty) input and exits 0.
            std::fs::write(bin.join("simctl"), b"").unwrap();
            std::fs::copy(r"C:\Windows\System32\cmd.exe", bin.join("simctl.exe")).unwrap();
        } else {
            std::fs::write(bin.join("simctl"), b"#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(bin.join("simctl"), std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        std::env::set_var("DEVELOPER_DIR", &dev);
    });
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A request the stand-in agent got, and whether the one after it had already come in when it
/// answered (sent without waiting for its reply).
#[derive(Clone, Debug)]
struct Seen {
    req: Value,
    next_waiting: bool,
}

type Log = Arc<Mutex<Vec<Seen>>>;

/// A reply as the agent frames it: its length, then its bytes, in one buffer.
fn framed(r: &str) -> Vec<u8> {
    let mut f = (r.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(r.as_bytes());
    f
}

/// A device on a stand-in agent that answers each request with `reply` (`info` is answered with
/// a 390x844 screen first), taking `delay` before each answer.
fn device(name: &str, delay: Duration, reply: impl Fn(&Value) -> String + Send + 'static) -> (IosDevice, Log) {
    let udid = format!("test-host-{name}-{}", std::process::id());
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = log.clone();
    std::thread::spawn(move || {
        let (mut c, _) = listener.accept().unwrap();
        // As the agent: no Nagle wait, a reply in one write (a length then a body held the body
        // back for Linux's delayed ACK, ~40 ms a reply).
        c.set_nodelay(true).unwrap();
        fn frame(c: &mut std::net::TcpStream) -> Option<Value> {
            let mut len = [0u8; 4];
            c.read_exact(&mut len).ok()?;
            let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
            c.read_exact(&mut body).ok()?;
            serde_json::from_slice(&body).ok()
        }
        frame(&mut c); // the token: not answered
        while let Some(req) = frame(&mut c) {
            let r = if req["op"] == "info" { r#"{"ok":true,"width":390,"height":844}"#.to_string() } else { reply(&req) };
            std::thread::sleep(delay);
            c.set_nonblocking(true).unwrap();
            let next_waiting = c.peek(&mut [0u8; 1]).is_ok_and(|n| n > 0);
            c.set_nonblocking(false).unwrap();
            seen.lock().unwrap().push(Seen { req, next_waiting });
            if c.write_all(&framed(&r)).is_err() {
                return;
            }
        }
    });
    let build = format!("{}/host-{}.xctestrun", std::env::temp_dir().display(), std::process::id());
    std::env::set_var("VIRTUOSO_IOS_AGENT", &build);
    let state = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"));
    std::fs::write(&state, serde_json::json!({"port": port, "token": "t", "build": build}).to_string()).unwrap();
    let d = IosDevice::connect(&udid);
    std::fs::remove_file(&state).unwrap();
    (d.expect("connected to the stand-in agent"), log)
}

fn ok(_: &Value) -> String {
    r#"{"ok":true}"#.to_string()
}

fn ops(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|s| s.req["op"].as_str().unwrap_or("").to_string()).collect()
}

#[test]
fn the_appearance_this_run_set_is_known_without_asking_simctl() {
    // setDarkMode, then assertDarkMode / toggleDarkMode: each `simctl ui` is a process and a
    // CoreSimulator round trip; what this run set itself is not read back.
    let _one = serial();
    let (mut d, _) = device("appearance", Duration::ZERO, ok);
    d.set_dark_mode(Some(true)).unwrap();
    let n = simctl_runs();
    assert!(d.dark_mode().unwrap());
    assert_eq!(simctl_runs(), n, "read back what was just set");
    d.set_dark_mode(None).unwrap();
    assert_eq!(simctl_runs(), n + 1, "a toggle only sets");
    assert!(!d.dark_mode().unwrap());
    assert_eq!(simctl_runs(), n + 1);
    // A touch can flip it in Settings: read again after one.
    d.tap(10, 10).unwrap();
    let _ = d.dark_mode();
    assert_eq!(simctl_runs(), n + 2, "read after an action");
}

#[test]
fn reading_the_appearance_keeps_the_screen_just_read() {
    // `simctl ui appearance` only reads: the next look still has the screen read a moment ago.
    // Here, where simctl does nothing at once: a real one can take longer than a plain read
    // stands (300 ms), as on the macOS CI runner.
    let _one = serial();
    let (mut d, log) = device("appearanceread", Duration::ZERO, |req| match req["op"].as_str() {
        Some("screen") => front("com.app", 1),
        _ => ok(req),
    });
    d.screen().unwrap();
    let n = simctl_runs();
    let _ = d.dark_mode();
    assert_eq!(simctl_runs(), n + 1, "simctl asked");
    d.screen().unwrap();
    assert_eq!(ops(&log).iter().filter(|op| *op == "screen").count(), 1, "{:?}", ops(&log));
}

/// A screen read's reply: `app` in front, as the read numbered `seq`.
fn front(app: &str, seq: u64) -> String {
    format!(r#"{{"ok":true,"seq":{seq},"windows":[{{"pkg":"{app}","type":1}}],"nodes":[{{"d":0,"k":"Application","b":[0,0,390,844]}}]}}"#)
}

#[test]
fn a_link_that_opened_another_app_is_not_waited_on_for_a_prompt() {
    // A web link opens Safari with no "Open in" prompt: once another app is in front, the
    // prompt cannot come (SpringBoard asks before the switch), and the 8 s wait for it stops.
    let _one = serial();
    let reads = std::sync::atomic::AtomicU64::new(0);
    let (mut d, _) = device("weblink", Duration::ZERO, move |req| {
        let n = reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match req["op"].as_str() {
            Some("screen") if n == 0 => front("com.app", 1),
            Some("screen") => front("com.apple.mobilesafari", 2),
            _ => ok(req),
        }
    });
    let start = Instant::now();
    d.open_link("https://example.com", Some("com.app"), false).unwrap();
    assert!(start.elapsed() < Duration::from_secs(2), "waited {:?}", start.elapsed());
}

#[test]
fn the_app_a_link_opened_is_seen_without_reading_its_screen() {
    // The agent names the app in front without a read: Safari's tree (a web page, and XCTest
    // waiting seconds for a just-launched app's accessibility) is never fetched to see it.
    let _one = serial();
    let reads = std::sync::atomic::AtomicU64::new(0);
    let (mut d, log) = device("frontop", Duration::ZERO, move |req| match req["op"].as_str() {
        Some("front") => r#"{"ok":true,"app":"com.apple.mobilesafari"}"#.to_string(),
        Some("screen") => front("com.app", 1 + reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
        _ => ok(req),
    });
    let start = Instant::now();
    d.open_link("https://example.com", None, true).unwrap();
    assert!(start.elapsed() < Duration::from_secs(2), "waited {:?}: {:?}", start.elapsed(), ops(&log));
    // The look before the link alone.
    assert_eq!(ops(&log).iter().filter(|op| *op == "screen").count(), 1, "{:?}", ops(&log));
}

#[test]
fn a_link_to_the_app_in_front_still_waits_for_a_prompt() {
    // Still the same app in front: the prompt may yet come; it is answered when it does.
    let _one = serial();
    let reads = std::sync::atomic::AtomicU64::new(0);
    let (mut d, log) = device("samelink", Duration::ZERO, move |req| {
        let n = reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let prompt = r#"{"ok":true,"seq":9,"windows":[{"pkg":"com.app","type":1},{"pkg":"com.apple.springboard","type":1}],"nodes":[{"d":0,"k":"Application","b":[0,0,390,844]},{"d":0,"w":1,"t":"Open in “Other”?","k":"StaticText","b":[60,320,330,350]},{"d":0,"w":1,"t":"Open","k":"Button","f":7,"b":[195,450,350,500]}]}"#;
        match req["op"].as_str() {
            Some("screen") if n < 4 => front("com.app", 1 + n),
            Some("screen") => prompt.to_string(),
            _ => ok(req),
        }
    });
    d.open_link("https://example.com/other", None, true).unwrap();
    assert_eq!(ops(&log).last().map(String::as_str), Some("tap"), "{:?}", ops(&log));
}

#[test]
fn the_prompt_poll_asks_only_for_a_screen_that_changed() {
    // Each poll after the first sends the last read's number: an unchanged screen comes back
    // as a few bytes, not the whole tree.
    let _one = serial();
    let reads = std::sync::atomic::AtomicU64::new(0);
    let (mut d, log) = device("pollifnot", Duration::ZERO, move |req| {
        // Screens counted alone (the agent is asked for the app in front between them).
        let n = if req["op"] == "screen" { reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed) } else { 0 };
        match req["op"].as_str() {
            Some("screen") if n < 2 => front("com.app", 5),
            Some("screen") if n < 6 => r#"{"ok":true,"unchanged":true}"#.to_string(),
            Some("screen") => front("com.apple.mobilesafari", 6),
            _ => ok(req),
        }
    });
    d.open_link("https://example.com", None, true).unwrap();
    let screens: Vec<Value> = log.lock().unwrap().iter().filter(|s| s.req["op"] == "screen").map(|s| s.req.clone()).collect();
    assert!(screens.len() >= 4, "{screens:?}");
    // The look before the link, the first poll: whole reads. Every poll after: `ifNot`.
    assert!(screens[2..].iter().all(|r| r["ifNot"].is_u64()), "{screens:?}");
}

/// Launched (by the do-nothing simctl): the wait for the app to be in front is due.
fn launched(d: &mut IosDevice) {
    use virtuoso::device::LaunchOptions;
    let o = LaunchOptions { stop_app: true, permissions: vec![("location".into(), "allow".into())], ..Default::default() };
    d.launch_app("com.app", &o).unwrap();
}

#[test]
fn the_launchs_wait_goes_with_the_read_after_it() {
    // The wait for the app just launched and the read that pays it go in one write: one round
    // trip, not two. The agent answers them in order.
    let _one = serial();
    let (mut d, log) = device("pipeline", Duration::from_millis(20), |req| match req["op"].as_str() {
        Some("screen") => front("com.app", 1),
        _ => ok(req),
    });
    launched(&mut d);
    d.screen().unwrap();
    let seen = log.lock().unwrap().clone();
    let wait = seen.iter().find(|s| s.req["op"] == "launched").expect("the launch's wait");
    assert!(wait.next_waiting, "the read came only after the wait's answer: {seen:?}");
    assert_eq!(ops(&log), vec!["info", "launched", "screen"]);
}

#[test]
fn a_failed_launch_wait_fails_the_read_and_the_next_request_is_answered_in_step() {
    let _one = serial();
    let (mut d, log) = device("pipefail", Duration::ZERO, |req| match req["op"].as_str() {
        Some("launched") => r#"{"ok":false,"error":"the app did not come to the front"}"#.to_string(),
        Some("screen") => front("com.app", 1),
        _ => ok(req),
    });
    launched(&mut d);
    let e = d.screen().unwrap_err().to_string();
    assert!(e.contains("did not come to the front"), "{e}");
    // The read's own answer was taken off the connection: the next request gets its own.
    assert_eq!(d.fresh_screen().unwrap().elements.len(), 1);
    assert_eq!(ops(&log).last().map(String::as_str), Some("screen"));
}

#[test]
fn an_action_after_a_launch_waits_for_it_first() {
    // A touch is not sent before the app is known to be in front.
    let _one = serial();
    let (mut d, log) = device("pipetap", Duration::from_millis(20), ok);
    launched(&mut d);
    d.tap(5, 5).unwrap();
    let seen = log.lock().unwrap().clone();
    assert!(!seen.iter().find(|s| s.req["op"] == "launched").unwrap().next_waiting, "{seen:?}");
}

#[test]
fn an_action_and_its_settle_go_in_one_write() {
    // A swipe, then the settle after it: the settle is already waiting when the swipe is
    // answered (no round trip of its own), and the agent takes them in order.
    use virtuoso::device::{Act, Lift};
    let _one = serial();
    let (mut d, log) = device("actsettle", Duration::from_millis(20), |req| match req["op"].as_str() {
        Some("settle") => r#"{"ok":true,"changed":true,"timedOut":false}"#.to_string(),
        _ => r#"{"ok":true,"busy":true}"#.to_string(),
    });
    let s = d.act_and_settle(Act::Swipe((10, 500), (10, 100), 300, Lift::Fling), Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    assert!(s.changed && !s.timed_out);
    let seen = log.lock().unwrap().clone();
    assert_eq!(ops(&log), vec!["info", "swipe", "settle"]);
    assert!(seen[1].next_waiting, "the settle came only after the swipe's answer: {seen:?}");
    assert_eq!(seen[2].req["nochange"], 300);
    // A tap's busy flag is kept from its own answer.
    d.act_and_settle(Act::Tap(5, 5), Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    assert!(d.touched_while_busy());
}

#[test]
fn a_failed_action_fails_and_the_next_request_is_answered_in_step() {
    use virtuoso::device::Act;
    let _one = serial();
    let (mut d, log) = device("actfail", Duration::ZERO, |req| match req["op"].as_str() {
        Some("type") => r#"{"ok":false,"error":"no field has focus"}"#.to_string(),
        Some("screen") => front("com.app", 1),
        _ => ok(req),
    });
    let e = d.act_and_settle(Act::Type("hi"), Duration::from_millis(100), Duration::from_secs(10)).unwrap_err().to_string();
    assert!(e.contains("no field has focus"), "{e}");
    assert_eq!(d.fresh_screen().unwrap().elements.len(), 1);
    assert_eq!(ops(&log).last().map(String::as_str), Some("screen"));
}

#[test]
fn a_settle_that_ends_on_the_screen_held_gets_no_tree_back() {
    // A tap that changed nothing: the settle names the screen the host holds, and the agent,
    // ending on that same screen, sends no tree; the host keeps its own as the settle's.
    use virtuoso::device::Act;
    let _one = serial();
    let (mut d, log) = device("settleifnot", Duration::ZERO, |req| match req["op"].as_str() {
        Some("screen") => front("com.app", 7),
        Some("settle") if req["ifNot"] == 7 => r#"{"ok":true,"unchanged":true,"seq":7,"changed":false,"timedOut":false}"#.to_string(),
        Some("settle") => r#"{"ok":true,"changed":false,"timedOut":false}"#.to_string(),
        _ => ok(req),
    });
    let before = d.fresh_screen().unwrap();
    let s = d.act_and_settle(Act::Tap(5, 5), Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    assert!(!s.changed);
    let settle = log.lock().unwrap().iter().find(|s| s.req["op"] == "settle").unwrap().req.clone();
    assert_eq!(settle["ifNot"], 7, "{settle}");
    // The settle's screen, kept: the next look needs no read.
    let n = ops(&log).len();
    let after = d.screen_to_act_on().unwrap();
    assert_eq!(ops(&log).len(), n, "{:?}", ops(&log));
    assert_eq!((after.seq, after.elements.len()), (before.seq, before.elements.len()));
}

#[test]
fn a_wait_whose_first_look_was_the_kept_screen_reads_at_once() {
    // The kept screen can be most of a second old: the first fresh look of a lookup that missed
    // it is not held back by the pause between polls.
    let _one = serial();
    let (mut d, _) = device("firstlook", Duration::ZERO, |req| match req["op"].as_str() {
        Some("screen") if req["ifNot"].is_u64() => r#"{"ok":true,"unchanged":true}"#.to_string(),
        Some("screen") => front("com.app", 3),
        _ => ok(req),
    });
    d.fresh_screen().unwrap();
    let start = Instant::now();
    for _ in 0..10 {
        let kept = d.screen().unwrap();
        d.screen_after_change(kept, Duration::from_secs(1)).unwrap();
    }
    assert!(start.elapsed() < Duration::from_millis(60), "10 first looks took {:?}", start.elapsed());
    // A later poll keeps its pause.
    let s = d.fresh_screen().unwrap();
    let start = Instant::now();
    d.screen_after_change(s, Duration::from_secs(1)).unwrap();
    assert!(start.elapsed() >= Duration::from_millis(10), "{:?}", start.elapsed());
}

#[test]
fn a_recording_starts_as_soon_as_its_file_shows() {
    // startRecording returns once simctl has created the video file: looked for every few ms,
    // not every 20.
    let _one = serial();
    let (mut d, _) = device("record", Duration::ZERO, ok);
    // Up to three tries: other work on the host can hold up a thread's wake-up for a while; a
    // 20 ms look puts the median near 10 ms however quiet the host is.
    let mut medians = Vec::new();
    for _ in 0..3 {
        let median = recording_lag(&mut d);
        if median < Duration::from_millis(5) {
            return;
        }
        medians.push(median);
    }
    panic!("back this long after the file showed (medians of three tries): {medians:?}");
}

/// The median time startRecording comes back after its file shows, over 11 recordings.
fn recording_lag(d: &mut IosDevice) -> Duration {
    let mut lags = Vec::new();
    for k in 0..11 {
        let path = std::env::temp_dir().join(format!("virtuoso-host-video-{}-{k}.mp4", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let shown = {
            let path = path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(250));
                std::fs::write(&path, b"").unwrap();
                Instant::now()
            })
        };
        d.start_recording(&path).unwrap();
        let back = Instant::now();
        lags.push(back.saturating_duration_since(shown.join().unwrap()));
        let _ = std::fs::remove_file(&path);
    }
    // The median: starting simctl itself can take longer than the file's delay here.
    lags.sort();
    lags[lags.len() / 2]
}

#[test]
fn another_builds_agent_is_stopped_on_the_connection_that_found_it() {
    // The device check found an agent, but another build's (virtuoso was upgraded): it is told
    // to quit on the connection the check made, not on a second one made just for that.
    let _one = serial();
    let udid = format!("test-host-oldbuild-{}", std::process::id());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<(usize, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let agent = std::thread::spawn(move || {
        fn frame(c: &mut std::net::TcpStream) -> Option<Value> {
            let mut len = [0u8; 4];
            c.read_exact(&mut len).ok()?;
            let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
            c.read_exact(&mut body).ok()?;
            serde_json::from_slice(&body).ok()
        }
        for n in 1.. {
            let Ok((mut c, _)) = listener.accept() else { return };
            c.set_nodelay(true).unwrap();
            frame(&mut c); // the token
            while let Some(req) = frame(&mut c) {
                let op = req["op"].as_str().unwrap_or("").to_string();
                log.lock().unwrap().push((n, op.clone()));
                if op == "quit" {
                    return; // its port closes
                }
                let r = r#"{"ok":true,"width":390,"height":844}"#;
                let _ = c.write_all(&framed(&r));
            }
        }
    });
    let build = format!("{}/host-{}.xctestrun", std::env::temp_dir().display(), std::process::id());
    std::env::set_var("VIRTUOSO_IOS_AGENT", &build);
    let state = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"));
    std::fs::write(&state, serde_json::json!({"port": port, "token": "t", "build": "/old/agent.xctestrun"}).to_string()).unwrap();
    assert!(virtuoso::ios::agent_answers(&udid));
    // This build's agent is then started afresh: no xcodebuild here, so that fails.
    let _ = IosDevice::connect(&udid);
    let _ = std::fs::remove_file(&state);
    agent.join().unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![(1, "info".to_string()), (1, "quit".to_string())]);
}

#[test]
fn the_appearance_is_set_and_read_through_the_agent_without_simctl() {
    // setDarkMode, toggleDarkMode and assertDarkMode: XCUIDevice.appearance in the agent, a
    // request each, where every `simctl ui appearance` was a process and a CoreSimulator round
    // trip (about a second on CI).
    use std::sync::atomic::{AtomicBool, Ordering};
    let _one = serial();
    let dark = Arc::new(AtomicBool::new(false));
    let state = dark.clone();
    let (mut d, log) = device("appearanceagent", Duration::ZERO, move |req| match req["op"].as_str() {
        Some("appearance") => {
            match req["set"].as_str() {
                Some("dark") => state.store(true, Ordering::Relaxed),
                Some("light") => state.store(false, Ordering::Relaxed),
                _ => {}
            }
            format!(r#"{{"ok":true,"dark":{}}}"#, state.load(Ordering::Relaxed))
        }
        _ => ok(req),
    });
    let n = simctl_runs();
    d.set_dark_mode(Some(true)).unwrap();
    assert!(d.dark_mode().unwrap());
    d.set_dark_mode(None).unwrap();
    assert!(!dark.load(Ordering::Relaxed), "toggled to light");
    // A touch can flip it in Settings: asked again after one, of the agent.
    d.tap(10, 10).unwrap();
    assert!(!d.dark_mode().unwrap());
    assert_eq!(simctl_runs(), n, "no simctl");
    let asked: Vec<String> = log.lock().unwrap().iter().filter(|s| s.req["op"] == "appearance").map(|s| s.req["set"].as_str().unwrap_or("read").to_string()).collect();
    assert_eq!(asked, vec!["dark", "light", "read"]);
}

#[test]
fn an_agent_that_cannot_set_the_appearance_leaves_it_to_simctl() {
    // XCUIDevice.appearance is iOS 17's: an agent on an older iOS says so once, and simctl sets
    // it from then on.
    let _one = serial();
    let (mut d, log) = device("appearancesimctl", Duration::ZERO, |req| match req["op"].as_str() {
        Some("appearance") => r#"{"ok":false,"error":"unsupported: XCUIDevice.appearance needs iOS 17"}"#.to_string(),
        _ => ok(req),
    });
    let n = simctl_runs();
    d.set_dark_mode(Some(true)).unwrap();
    assert_eq!(simctl_runs(), n + 1, "simctl set it");
    d.set_dark_mode(Some(false)).unwrap();
    assert_eq!(simctl_runs(), n + 2);
    assert_eq!(ops(&log).iter().filter(|op| *op == "appearance").count(), 1, "{:?}", ops(&log));
}

#[test]
fn hide_keyboard_and_its_settle_go_in_one_write() {
    // The settle after hideKeyboard is already waiting when hideKeyboard is answered, and the
    // screen it ended on is kept for the next step's look.
    let _one = serial();
    let (mut d, log) = device("hidesettle", Duration::from_millis(20), |req| match req["op"].as_str() {
        Some("settle") => r#"{"ok":true,"seq":4,"changed":true,"timedOut":false,"windows":[{"pkg":"com.app","type":1}],"nodes":[{"d":0,"k":"Application","b":[0,0,390,844]}]}"#.to_string(),
        _ => ok(req),
    });
    d.hide_keyboard_and_settle(Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    let seen = log.lock().unwrap().clone();
    assert_eq!(ops(&log), vec!["info", "hideKeyboard", "settle"]);
    assert!(seen[1].next_waiting, "the settle came only after hideKeyboard's answer: {seen:?}");
    let n = ops(&log).len();
    assert_eq!(d.screen_to_act_on().unwrap().seq, Some(4));
    assert_eq!(ops(&log).len(), n, "the settle's screen is kept: {:?}", ops(&log));
}

/// A screen with a focused field, and the keyboard up or not.
fn typing(seq: u64, keyboard: bool) -> String {
    let kb = if keyboard { r#",{"d":1,"w":1,"k":"Keyboard","b":[0,500,390,844]}"# } else { "" };
    format!(
        r#"{{"ok":true,"seq":{seq},"changed":true,"timedOut":false,"windows":[{{"pkg":"com.app","type":1}},{{"pkg":"com.app","type":2}}],"nodes":[{{"d":0,"k":"Application","b":[0,0,390,844]}},{{"d":1,"k":"TextField","f":166,"b":[20,100,370,140]}}{kb}]}}"#
    )
}

#[test]
fn hide_keyboards_wait_for_a_keyboard_looks_at_the_screen_kept_first() {
    // inputText's settle has just ended on a screen with the keyboard up: hideKeyboard's wait
    // for one coming up needs no read of its own.
    use std::sync::atomic::{AtomicBool, Ordering};
    use virtuoso::device::Act;
    let _one = serial();
    let up = Arc::new(AtomicBool::new(true));
    let shows = up.clone();
    let (mut d, log) = device("kbkept", Duration::ZERO, move |req| match req["op"].as_str() {
        Some("settle") | Some("screen") => typing(5, shows.load(Ordering::Relaxed)),
        _ => ok(req),
    });
    d.act_and_settle(Act::Type("hi"), Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    let n = ops(&log).len();
    d.await_keyboard(Duration::from_millis(500), Duration::from_millis(10)).unwrap();
    assert_eq!(ops(&log).len(), n, "{:?}", ops(&log));
    // A focused field with no keyboard yet: waited for, by reads.
    up.store(false, Ordering::Relaxed);
    d.act_and_settle(Act::Type("hi"), Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    let n = ops(&log).len();
    d.await_keyboard(Duration::from_millis(100), Duration::from_millis(10)).unwrap();
    assert!(ops(&log)[n..].iter().any(|op| op == "screen"), "{:?}", ops(&log));
}

#[test]
fn a_link_into_the_app_and_its_settle_go_in_one_write() {
    // openLink into the flow's app: the agent's openURL and the runner's settle in one write.
    let _one = serial();
    let (mut d, log) = device("linksettle", Duration::from_millis(20), |req| match req["op"].as_str() {
        Some("settle") => r#"{"ok":true,"changed":true,"timedOut":false}"#.to_string(),
        _ => ok(req),
    });
    let s = d.open_link_and_settle("app://deep", Some("com.app"), false, Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    assert!(s.changed);
    let seen = log.lock().unwrap().clone();
    assert_eq!(ops(&log), vec!["info", "openURL", "settle"]);
    assert!(seen[1].next_waiting, "the settle came only after openURL's answer: {seen:?}");
}
