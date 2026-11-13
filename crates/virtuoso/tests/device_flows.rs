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
fn launching_an_app_that_is_not_installed_says_so() {
    let Some(mut d) = device() else { return };
    let r = run(&mut d, "appId: dev.virtuoso.not.installed
---
- launchApp
");
    assert_eq!(r.status, FlowStatus::Failed);
    let e = r.error.unwrap_or_default();
    assert!(e.contains("dev.virtuoso.not.installed is not installed, or has no launcher activity"), "{e}");
}

#[test]
fn the_launch_scripts_permission_lists_match_a_full_dumpsys_read() {
    let Some(mut d) = device() else { return };
    let mut apps = vec!["com.android.settings", "com.google.android.deskclock"];
    if bench_app(&mut d) {
        // Cleared, so some are not granted.
        d.exec(&["pm", "clear", BENCH_APP]).unwrap();
        apps.push(BENCH_APP);
    }
    for app in apps {
        let full = d.exec(&["dumpsys", "package", app]).unwrap().1;
        for ungranted in [true, false] {
            let mut want = if ungranted { virtuoso::android::ungranted_runtime_permissions(&full) } else { virtuoso::android::runtime_permissions(&full) };
            let out = d.exec(&["sh", "-c", &virtuoso::android_device::permission_names(app, ungranted)]).unwrap().1;
            let mut got: Vec<String> = out.lines().map(str::to_string).collect();
            got.dedup();
            got.sort();
            got.dedup();
            want.sort();
            assert_eq!(got, want, "{app}, ungranted only: {ungranted}");
        }
    }
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
fn the_screen_size_needs_no_read() {
    // Every swipe, scroll and tap at a point asked for it: a whole read, its tree thrown away.
    use virtuoso::device::Device;
    let Some(mut d) = device() else { return };
    let read = d.screen().unwrap().screen_rect();
    let before = d.requests();
    let rect = d.screen_rect().unwrap();
    assert_eq!(d.requests() - before, 0);
    assert_eq!(rect, read);
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

#[test]
fn hide_keyboard_with_the_keyboard_up_asks_only_about_windows() {
    // It read the whole screen past the cache (the keyboard's keys and all) only to learn a
    // keyboard was up, which the windows it asks for next say.
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp
- tapOn: Search settings
- inputText: wifi
"));
    let before = d.requests();
    let start = Instant::now();
    passes(&mut d, &format!("{SETTINGS}- hideKeyboard
"));
    let (sent, took) = (d.requests() - before, start.elapsed());
    passes(&mut d, &format!("{SETTINGS}- back
"));
    eprintln!("hideKeyboard: {sent} requests, {took:?}");
    assert!(sent <= 3, "{sent} requests to hide the keyboard");
}

#[test]
fn typing_and_erasing_each_go_with_their_settle() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- tapOn: Search settings\n"));
    for step in ["inputText: wifi", "eraseText: 4"] {
        let before = d.requests();
        passes(&mut d, &format!("{SETTINGS}- {step}\n"));
        assert_eq!(d.requests() - before, 1, "{step}");
    }
    passes(&mut d, &format!("{SETTINGS}- hideKeyboard\n- back\n"));
}

#[test]
fn text_no_key_can_type_without_a_focused_field_still_fails_the_step() {
    let Some(mut d) = device() else { return };
    // Settings' search (another package) stays on top of a relaunch when a test left it open.
    d.exec(&["am", "force-stop", "com.google.android.settings.intelligence"]).unwrap();
    passes(&mut d, &format!("{SETTINGS}- launchApp\n"));
    let r = run(&mut d, &format!("{SETTINGS}- inputText: \"\u{6f22}\u{5b57}\"\n- assertVisible: Network & internet\n"));
    assert_eq!(r.status, FlowStatus::Failed);
    let e = r.error.unwrap_or_default();
    assert!(e.contains("2 characters of '\u{6f22}\u{5b57}' could not be typed"), "{e}");
    assert_eq!(r.steps.len(), 1, "the flow stopped at the inputText");
}

#[test]
fn a_long_press_and_a_drag_each_go_with_their_settle() {
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n"));
    // On the status bar, where they do nothing.
    let steps = [
        "longPressOn:\n    point: 50%,1%",
        "dragAndDrop:\n    from:\n      point: 40%,1%\n    to:\n      point: 60%,1%\n    holdDuration: 100\n    duration: 100",
    ];
    for step in steps {
        let before = d.requests();
        passes(&mut d, &format!("{SETTINGS}- {step}\n"));
        let sent = d.requests() - before;
        // A press or drag there can pull the notification shade down: closed for the next test.
        d.exec(&["cmd", "statusbar", "collapse"]).unwrap();
        assert_eq!(sent, 1, "{step}");
    }
}

#[test]
fn device_commands_take_one_request_each() {
    // Each was a request of its own followed by more: a mark, then the settle (or the exec,
    // then the settle).
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n"));
    let steps = [
        ("waitForAnimationToEnd", 1),
        ("stopApp", 1),
        ("launchApp", 0),
        ("killApp", 1),
        ("openLink:\n    link: https://www.example.com\n    browser: true", 1),
        ("pressKey: Home", 0),
        ("launchApp", 0),
    ];
    for (step, want) in steps {
        let before = d.requests();
        passes(&mut d, &format!("{SETTINGS}- {step}\n"));
        if want > 0 {
            assert_eq!(d.requests() - before, want, "{step}");
        }
    }
    // The browser openLink started holds a lot of the emulator's memory.
    d.exec(&["am", "force-stop", "com.android.chrome"]).unwrap();
}

#[test]
fn a_tap_on_an_element_checks_taps_and_settles_in_one_request_after_the_lookup() {
    // It was the lookup's read, a fresh read shipped whole to be compared on the host, then the
    // tap and its settle.
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- assertVisible: Network & internet\n"));
    let before = d.requests();
    passes(&mut d, &format!("{SETTINGS}- tapOn: Network & internet\n"));
    let sent = d.requests() - before;
    passes(&mut d, &format!("{SETTINGS}- assertVisible: Internet\n- back\n- assertVisible: Network & internet\n"));
    assert_eq!(sent, 2, "requests for a tapOn");
}

#[test]
fn a_lookup_right_after_a_settle_reads_the_screen_the_settle_brought() {
    // Each lookup after an action was a request of its own, for the screen the settle had just
    // waited on.
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp
- assertVisible: Network & internet
"));
    let before = d.requests();
    passes(&mut d, &format!("{SETTINGS}- tapOn: Network & internet
- assertVisible: Internet
- back
- assertVisible: Network & internet
"));
    let sent = d.requests() - before;
    // The lookup, the check with the tap and its settle (bringing the screen), the Back with its
    // settle (the same).
    assert_eq!(sent, 3, "requests for tapOn, assertVisible, back, assertVisible");
}

#[test]
fn a_screen_that_changed_since_the_lookup_is_not_tapped() {
    use virtuoso::device::Device;
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- assertVisible: Network & internet\n"));
    let screen = d.screen().unwrap();
    let i = screen.elements.iter().position(|e| e.text == "Network & internet").unwrap();
    let (x, y) = screen.elements[i].bounds.center();
    let mut changed = screen.clone();
    let mut elements = changed.elements.to_vec();
    elements[i].text.push_str(" (changed)");
    changed.elements = elements.into();
    let untapped = d.tap_if_unchanged_and_settle(&changed, x, y, Duration::from_millis(300), Duration::from_secs(5)).unwrap();
    let now = untapped.expect_err("tapped on a screen that changed");
    assert!(now.elements.iter().any(|e| e.text == "Network & internet"), "the screen as it is now");
    passes(&mut d, &format!("{SETTINGS}- assertVisible: Network & internet\n- assertNotVisible: Internet\n"));
    // The same screen as the lookup's: tapped, and the settle went with it.
    let tapped = d.tap_if_unchanged_and_settle(&screen, x, y, Duration::from_millis(300), Duration::from_secs(5)).unwrap();
    let Ok(settled) = tapped else { panic!("not tapped on the lookup's own screen") };
    assert!(settled.changed);
    passes(&mut d, &format!("{SETTINGS}- assertVisible: Internet\n- back\n- assertVisible: Network & internet\n"));
}

#[test]
fn set_permissions_changes_them_all_in_one_request() {
    // Each permission was an exec of its own, and `all` a whole dumpsys shipped to the host first.
    use virtuoso::device::Device;
    let Some(mut d) = device() else { return };
    if !bench_app(&mut d) {
        return;
    }
    let all = |v: &str| vec![("all".to_string(), v.to_string())];
    let before = d.requests();
    d.set_permissions(BENCH_APP, &all("deny")).unwrap();
    assert_eq!(d.requests() - before, 1, "all: deny");
    let denied = d.ungranted_permissions(BENCH_APP).unwrap();
    assert!(denied.iter().any(|p| p == "android.permission.CAMERA"), "{denied:?}");
    let before = d.requests();
    d.set_permissions(BENCH_APP, &[("location".to_string(), "allow".to_string()), ("camera".to_string(), "allow".to_string())]).unwrap();
    assert_eq!(d.requests() - before, 1, "location and camera: allow");
    let left = d.ungranted_permissions(BENCH_APP).unwrap();
    for p in ["android.permission.CAMERA", "android.permission.ACCESS_FINE_LOCATION", "android.permission.ACCESS_COARSE_LOCATION"] {
        assert!(!left.iter().any(|l| l == p), "{p} still denied: {left:?}");
    }
    assert!(left.len() < denied.len() && !left.is_empty(), "{left:?}");
    // In order: everything allowed, then the camera denied again.
    d.set_permissions(BENCH_APP, &[("all".to_string(), "allow".to_string()), ("camera".to_string(), "deny".to_string())]).unwrap();
    assert_eq!(d.ungranted_permissions(BENCH_APP).unwrap(), vec!["android.permission.CAMERA".to_string()]);
    d.set_permissions(BENCH_APP, &all("allow")).unwrap();
    assert!(d.ungranted_permissions(BENCH_APP).unwrap().is_empty());
}

#[test]
fn a_stopped_recording_is_pulled_whole() {
    // After the adb shell had exited (screenrecord done), it slept 300 ms more; now it checks
    // the process is gone.
    use virtuoso::device::Device;
    let Some(mut d) = device() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut took = Vec::new();
    for n in 0..3 {
        let clip = dir.path().join(format!("clip{n}.mp4"));
        d.start_recording(&clip).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let start = Instant::now();
        d.stop_recording().unwrap();
        took.push(start.elapsed());
        // Whole: screenrecord writes the moov atom last, as it exits.
        let video = std::fs::read(&clip).unwrap();
        assert!(video.len() > 1000 && video.windows(4).any(|w| w == b"moov"), "a whole video was pulled");
    }
    // Not a time limit: the pull alone took 0.4-2.1 s on the test emulator.
    eprintln!("stopRecording took {took:?}");
}

/// Speedrun's benchmark app (github.com/hubastard/speedrun, bench/app), when it is installed.
const BENCH_APP: &str = "dev.speedrun.bench";

fn bench_app(d: &mut AndroidDevice) -> bool {
    d.exec(&["pm", "path", BENCH_APP]).is_ok_and(|(code, out)| code == 0 && out.contains("package:"))
}

#[test]
fn connecting_turns_animations_off_before_it_returns() {
    let Some(serial) = std::env::var("VIRTUOSO_DEVICE").ok() else { return };
    let adb = virtuoso::android::Adb::new(&serial);
    const KEYS: &str = "window_animation_scale transition_animation_scale animator_duration_scale";
    adb.shell(&format!("for k in {KEYS}; do settings put global $k 1; done")).unwrap();
    let start = Instant::now();
    let d = AndroidDevice::connect_with(&serial, true).unwrap();
    let took = start.elapsed();
    let now = adb.shell(&format!("for k in {KEYS}; do settings get global $k; done")).unwrap();
    drop(d);
    eprintln!("connect took {took:?}");
    assert!(now.split_whitespace().all(|v| v == "0" || v == "0.0"), "{now}");
}

#[test]
fn a_clear_state_launch_grants_every_runtime_permission() {
    let Some(mut d) = device() else { return };
    if !bench_app(&mut d) {
        return;
    }
    passes(&mut d, &format!("appId: {BENCH_APP}
---
- launchApp:
    clearState: true
"));
    let left = d.ungranted_permissions(BENCH_APP).unwrap();
    assert!(left.is_empty(), "still ungranted: {left:?}");
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

#[test]
fn a_read_with_nothing_happened_since_the_last_one_gives_that_tree_back() {
    // Read-only commands in a row (assertVisible then a tapOn's lookup) each had the agent dump
    // the whole screen and the host parse it, though no event had come in between.
    use virtuoso::device::Device;
    let Some(mut d) = device() else { return };
    passes(&mut d, &format!("{SETTINGS}- launchApp\n- assertVisible: Network & internet\n"));
    // A status bar clock ticking over between two reads is an event: a few tries.
    let same = (0..5).any(|_| {
        let a = d.screen().unwrap();
        let b = d.screen().unwrap();
        std::sync::Arc::ptr_eq(&a.elements, &b.elements)
    });
    assert!(same, "every second read was a new dump");
    // What happens after is seen.
    passes(&mut d, &format!("{SETTINGS}- assertVisible: Network & internet\n- tapOn: Network & internet\n- assertVisible: Internet\n- back\n- assertVisible: Network & internet\n"));
}
