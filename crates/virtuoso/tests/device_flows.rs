//! Whole flows on a real Android device or emulator (Settings, which every image has). They
//! run when VIRTUOSO_DEVICE names the device and pass trivially otherwise:
//!
//!   VIRTUOSO_DEVICE=emulator-5554 cargo test --test device_flows -- --test-threads=1

use virtuoso::android::AndroidDevice;
use virtuoso::flow::parse_flow;
use virtuoso::runner::{FlowStatus, RunOptions, Runner};
use std::time::{Duration, Instant};

#[path = "common/location.rs"]
mod location;
use location::gps_fix;

fn device() -> Option<AndroidDevice> {
    let serial = std::env::var("VIRTUOSO_DEVICE").ok()?;
    let mut d = AndroidDevice::connect(&serial).expect("connect");
    d.disable_animations().unwrap();
    Some(d)
}

fn run(dev: &mut AndroidDevice, yaml: &str) -> virtuoso::runner::FlowResult {
    let flow = parse_flow(yaml, None).unwrap();
    let opts = RunOptions { lookup_timeout: Duration::from_secs(10), ..Default::default() };
    Runner::new(dev, opts).run_flow(&flow)
}

fn passes(dev: &mut AndroidDevice, yaml: &str) {
    let r = run(dev, yaml);
    let steps: Vec<String> = r.steps.iter().map(|s| format!("{:?} {} {:?}", s.status, s.name, s.duration)).collect();
    assert_eq!(r.status, FlowStatus::Passed, "{:?}\n{}", r.error, steps.join("\n"));
}

const SETTINGS: &str = "appId: com.android.settings\n---\n";

#[test]
fn launch_tap_and_back_in_settings() {
    let Some(mut d) = device() else { return };
    passes(
        &mut d,
        &format!("{SETTINGS}- launchApp\n- tapOn: Network & internet\n- assertVisible: Internet\n- back\n- assertVisible: Network & internet\n"),
    );
}

#[test]
fn a_tap_that_opens_a_screen_returns_quickly() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n"));
    let start = Instant::now();
    passes(&mut d, &format!("{SETTINGS}- tapOn: Network & internet\n- assertVisible: Internet\n- back\n"));
    assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
}

#[test]
fn scroll_until_visible_finds_the_bottom_of_settings() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- scrollUntilVisible:\n    element: About emulated device\n- tapOn: About emulated device\n- assertVisible: Device name\n"));
}

#[test]
fn input_text_types_into_the_search_field() {
    let Some(mut d) = device() else { return };
    passes(
        &mut d,
        &format!("{SETTINGS}- launchApp\n- tapOn: Search settings\n- inputText: display\n- assertVisible: Display\n- eraseText: 20\n- inputText: wifi 2\n- hideKeyboard\n- back\n"),
    );
}

#[test]
fn stop_app_and_launch_with_clear_state() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp:\n    clearState: true\n- assertVisible: Network & internet\n- stopApp\n- assertNotVisible: Network & internet\n"));
}

#[test]
fn keys_swipes_and_screenshots_work() {
    let Some(mut d) = device() else { return };
    let out = tempfile::tempdir().unwrap();
    let flow = parse_flow(
        &format!("{SETTINGS}- launchApp\n- swipe:\n    direction: UP\n- scroll\n- pressKey: Home\n- takeScreenshot: home\n"),
        None,
    )
    .unwrap();
    let r = Runner::new(&mut d, RunOptions { output_dir: Some(out.path().into()), ..Default::default() }).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    let png = std::fs::read(out.path().join("home.png")).unwrap();
    let img = virtuoso::image_util::decode(&png).unwrap();
    assert!(img.width > 300 && img.height > img.width);
}

#[test]
fn dark_mode_and_orientation_change_and_restore() {
    let Some(mut d) = device() else { return };
    passes(
        &mut d,
        &format!("{SETTINGS}- setDarkMode: true\n- assertDarkMode\n- setDarkMode: false\n- assertLightMode\n- setOrientation: LANDSCAPE_LEFT\n- setOrientation: PORTRAIT\n"),
    );
}

#[test]
fn open_link_opens_settings_screens_by_intent_url() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- openLink:\n    link: https://www.example.com\n    browser: true\n- pressKey: Home\n"));
}

#[test]
fn location_airplane_mode_media_and_recording_work_on_a_device() {
    let Some(mut d) = device() else { return };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dot.png"), virtuoso::image_util::solid_png(8, 8, [255, 0, 0])).unwrap();
    let flow_path = dir.path().join("device.yaml");
    let yaml = format!(
        "{SETTINGS}- setLocation:\n    latitude: 52.3676\n    longitude: 4.9041\n- travel:\n    points:\n      - 52.3676,4.9041\n      - 52.3680,4.9045\n    speed: 1000\n- setAirplaneMode: enabled\n- setAirplaneMode: disabled\n- toggleAirplaneMode\n- toggleAirplaneMode\n- addMedia:\n    - dot.png\n- setPermissions:\n    permissions:\n      location: allow\n- startRecording: clip\n- launchApp\n- stopRecording\n"
    );
    std::fs::write(&flow_path, &yaml).unwrap();
    let flow = parse_flow(&yaml, Some(&flow_path)).unwrap();
    let out = tempfile::tempdir().unwrap();
    let r = Runner::new(&mut d, RunOptions { output_dir: Some(out.path().into()), ..Default::default() }).run_flow(&flow);
    let steps: Vec<String> = r.steps.iter().map(|s| format!("{:?} {} {:?}", s.status, s.name, s.error)).collect();
    assert_eq!(r.status, FlowStatus::Passed, "{:?}\n{}", r.error, steps.join("\n"));
    assert!(std::fs::metadata(out.path().join("clip.mp4")).unwrap().len() > 1000, "a video was pulled");
    let airplane = d.exec(&["settings", "get", "global", "airplane_mode_on"]).unwrap().1;
    assert_eq!(airplane.trim(), "0", "airplane mode is back off");
    let loc = d.exec(&["dumpsys", "location"]).unwrap().1;
    assert!(loc.contains("52.36") || loc.contains("52.37"), "a test location was set");
}

#[test]
fn set_clipboard_puts_text_on_the_device_clipboard() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- setClipboard: virtuoso-clip-7\n"));
    assert_eq!(d.clipboard().unwrap(), "virtuoso-clip-7");
}

#[test]
fn open_notifications_pulls_down_the_shade_and_drag_and_drop_runs() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- openNotifications\n"));
    let windows = d.exec(&["sh", "-c", "dumpsys window | grep mCurrentFocus"]).unwrap().1;
    let shade = windows.lines().any(|l| l.contains("mCurrentFocus") && l.contains("NotificationShade"));
    d.exec(&["cmd", "statusbar", "collapse"]).unwrap();
    assert!(shade, "the notification shade has focus: {windows}");
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- dragAndDrop:\n    from: Network & internet\n    to:\n      point: 50%,80%\n    holdDuration: 300\n    duration: 300\n"));
}

#[test]
fn a_crashed_agent_is_restarted_and_the_command_goes_through() {
    let Some(mut d) = device() else { return };
    let serial = std::env::var("VIRTUOSO_DEVICE").unwrap();
    let killed = std::process::Command::new("adb").args(["-s", &serial, "shell", "kill -9 $(pidof virtuoso-agent)"]).output().unwrap();
    assert!(killed.status.success(), "{}", String::from_utf8_lossy(&killed.stderr));
    std::thread::sleep(Duration::from_millis(300));
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- assertVisible: Network & internet\n"));
}

#[test]
fn erasing_a_whole_field_clears_it_at_once_and_typing_goes_on() {
    let Some(mut d) = device() else { return };
    let r = run(
        &mut d,
        &format!("{SETTINGS}- launchApp\n- tapOn: Search Settings\n- inputText: brightness level\n- eraseText: 20\n- inputText: dark\n- assertVisible: dark\n- assertNotVisible: brightness level\n- hideKeyboard\n- back\n"),
    );
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    let erase = r.steps.iter().find(|s| s.name.starts_with("EraseText")).unwrap();
    assert!(erase.duration < Duration::from_millis(400), "erase took {:?}", erase.duration);
}

#[test]
fn erasing_more_than_a_field_holds_stops_once_it_is_empty() {
    // Maestro's bare eraseText is 50 deletes: past the 16 characters here, each is a waited key
    // (25-70 ms on an emulator, with the search under the field) that changes nothing. Timing it
    // was too noisy to tell: the agent says how many it sent.
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp
- tapOn: Search Settings
- inputText: brightness level
"));
    let serial = std::env::var("VIRTUOSO_DEVICE").unwrap();
    drop(d);
    let mut agent = virtuoso::android::AndroidAgent::start(&virtuoso::android::Adb::new(&serial)).unwrap();
    let r = agent.client().call(serde_json::json!({"op": "erase", "count": 50})).unwrap();
    drop(agent);
    let mut d = device().unwrap();
    passes(&mut d, &format!("{SETTINGS}- inputText: dark
- assertVisible: dark
- assertNotVisible: brightness level
- hideKeyboard
- back
"));
    assert_eq!(r["deleted"], 16, "{r}");
}

#[test]
fn erasing_part_of_a_field_keeps_the_rest() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- tapOn: Search Settings\n- inputText: darkness\n- eraseText: 4\n- assertVisible: dark\n- hideKeyboard\n- back\n"));
}

#[test]
fn ungranted_permissions_match_a_full_dumpsys_read() {
    let Some(mut d) = device() else { return };
    let app = "com.android.settings";
    let full = d.exec(&["dumpsys", "package", app]).unwrap().1;
    let mut want = virtuoso::android::ungranted_runtime_permissions(&full);
    let mut got = d.ungranted_permissions(app).unwrap();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}

#[test]
fn launch_app_grants_the_apps_runtime_permissions_and_passes_arguments() {
    let Some(mut d) = device() else { return };
    let clock = "com.google.android.deskclock";
    let granted = |d: &mut AndroidDevice| d.exec(&["sh", "-c", &format!("dumpsys package {clock} | grep 'READ_CALENDAR: granted'")]).unwrap().1;
    if !granted(&mut d).contains("READ_CALENDAR") {
        return;
    }
    d.exec(&["pm", "revoke", clock, "android.permission.READ_CALENDAR"]).unwrap();
    assert!(granted(&mut d).contains("granted=false"));
    passes(&mut d, &format!("appId: {clock}\n---\n- launchApp:\n    arguments:\n      note: \"it's a 'quoted' value\"\n      count: 3\n- pressKey: Home\n"));
    let after = granted(&mut d);
    d.exec(&["pm", "revoke", clock, "android.permission.READ_CALENDAR"]).unwrap();
    d.exec(&["am", "force-stop", clock]).unwrap();
    assert!(after.contains("granted=true"), "{after}");
}

#[test]
fn waiting_for_an_element_asks_the_agent_when_something_changed_not_every_10_ms() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n"));
    let flow = parse_flow(&format!("{SETTINGS}- assertVisible: Nothing called this\n"), None).unwrap();
    let opts = RunOptions { lookup_timeout: Duration::from_secs(1), ..Default::default() };
    let before = d.requests();
    let r = Runner::new(&mut d, opts).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Failed);
    let sent = d.requests() - before;
    // Polling every 10 ms sent 40-70 requests in a second, each a full read of the screen.
    assert!(sent <= 20, "{sent} requests in a 1 s wait");
}

#[test]
fn hiding_the_keyboard_waits_for_it_to_go_without_polling() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- tapOn: Search settings\n- inputText: wifi\n"));
    let before = d.requests();
    virtuoso::device::Device::hide_keyboard(&mut d).unwrap();
    let sent = d.requests() - before;
    // Settings reopens where it was left: leave it on its main screen for the next test.
    passes(&mut d, &format!("{SETTINGS}- back\n"));
    assert!(sent <= 4, "{sent} requests to hide the keyboard");
}

/// Speedrun's benchmark app (github.com/hubastard/speedrun, bench/app), when it is installed.
const BENCH_APP: &str = "dev.speedrun.bench";

fn bench_app(d: &mut AndroidDevice) -> bool {
    d.exec(&["pm", "path", BENCH_APP]).is_ok_and(|(code, out)| code == 0 && out.contains("package:"))
}

#[test]
fn scrolling_sideways_drags_back_to_back_without_waiting_on_the_last_drag() {
    let Some(mut d) = device() else { return };
    if !bench_app(&mut d) {
        return;
    }
    let open = format!(
        "appId: {BENCH_APP}
---
- launchApp:
    clearState: true
- tapOn:
    id: onboarding-skip
- tapOn:
    id: guest
- tapOn:
    id: tab-lab
- tapOn:
    id: lab-drag
"
    );
    passes(&mut d, &open);
    // A dozen drags across the card list. Each was moved to start after the last one ended,
    // so it ended in the future and the next waited for it: 4.1-4.3 s.
    let start = Instant::now();
    passes(&mut d, &format!("appId: {BENCH_APP}
---
- scrollUntilVisible:
    element: Card 25
    direction: RIGHT
- assertVisible: Card 25
"));
    assert!(start.elapsed() < Duration::from_millis(3300), "took {:?}", start.elapsed());
}

#[test]
fn back_to_back_drags_in_a_react_native_list_move_it_by_the_drag_without_a_fling() {
    use virtuoso::device::Device;
    let Some(mut d) = device() else { return };
    if !bench_app(&mut d) {
        return;
    }
    let open = format!(
        "appId: {BENCH_APP}\n---\n- launchApp:\n    clearState: true\n- tapOn:\n    id: onboarding-skip\n- tapOn:\n    id: guest\n- tapOn:\n    id: tab-lab\n- tapOn:\n    id: lab-drag\n- tapOn:\n    id: open-long-list\n- assertVisible: Row 1\n"
    );
    passes(&mut d, &open);
    let rect = d.screen().unwrap().screen_rect();
    // Shorter than scrollUntilVisible's drags, so rows stay on screen across two.
    let (x, from, to) = (rect.width() / 2, rect.height() * 75 / 100, rect.height() * 45 / 100);
    let rows = |d: &mut AndroidDevice| -> std::collections::HashMap<String, i32> {
        d.fresh_screen().unwrap().elements.iter().filter(|e| e.text.starts_with("Row ")).map(|e| (e.text.clone(), e.bounds.top)).collect()
    };
    for round in 0..8 {
        let before = rows(&mut d);
        for _ in 0..2 {
            d.swipe((x, from), (x, to), 50, virtuoso::device::Lift::Cancel).unwrap();
            d.settle(Duration::from_millis(300), Duration::from_secs(5)).unwrap();
            d.screen().unwrap();
        }
        let after = rows(&mut d);
        // Each drag carries the rows its length less the touch slop (1320 px for two of 720);
        // a fling carries them further (1771), and scrollUntilVisible past what it looks for.
        // (A drag the list misses only costs it another.)
        let drags = 2 * (from - to);
        let moved: Vec<i32> = before.iter().filter_map(|(t, y)| after.get(t).map(|a| y - a)).collect();
        assert!(!moved.is_empty() && moved.iter().all(|&m| m <= drags), "round {round}: two drags of {}, rows moved {moved:?}", from - to);
    }
}

#[test]
fn set_location_keeps_the_location_current_for_an_app_that_watches_late() {
    let Some(mut d) = device() else { return };
    virtuoso::device::Device::set_location(&mut d, 45.5017, -73.5673).unwrap();
    std::thread::sleep(Duration::from_secs(5));
    let (fix, age) = gps_fix(&virtuoso::android::Adb::new(&std::env::var("VIRTUOSO_DEVICE").unwrap()));
    assert_eq!(fix, "45.501700,-73.567300");
    assert!(age < 3.0, "the fix is {age:.1} s old");
}

/// When the device got the last `n` touches down, in ms since boot (`dumpsys input`'s queue).
fn last_downs(d: &AndroidDevice, n: usize) -> Vec<f64> {
    let dump = d.adb().shell("dumpsys input").unwrap();
    let queue = dump.split("RecentQueue:").nth(1).expect("dumpsys input lists recent events");
    let downs: Vec<f64> = queue
        .lines()
        .filter(|l| l.contains("MotionEvent(") && l.contains("action=DOWN"))
        .map(|l| l.split("eventTime=").nth(1).unwrap().split(',').next().unwrap().parse::<f64>().unwrap() / 1e6)
        .collect();
    downs[downs.len().saturating_sub(n)..].to_vec()
}

#[test]
fn a_double_tap_reaches_the_device_its_interval_apart() {
    // Each tap was held back to go with what came next: the first then went out only with the
    // second, ~5 ms before it, under the 40 ms a double tap needs between its taps.
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n"));
    let r = virtuoso::device::Device::screen_rect(&mut d).unwrap();
    // The status bar takes a tap without doing anything (a tenth down was Settings' search bar,
    // which stayed open for the next test).
    let (x, y) = (r.width() / 2, 10);
    virtuoso::device::Device::taps(&mut d, x, y, 2, Duration::from_millis(100)).unwrap();
    virtuoso::device::Device::settle(&mut d, Duration::from_millis(300), Duration::from_secs(5)).unwrap();
    let downs = last_downs(&d, 2);
    let gap = downs[1] - downs[0];
    assert!(gap >= 80.0, "the taps went {gap} ms apart");
}
