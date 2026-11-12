//! iOS behaviour on a booted simulator (needs VIRTUOSO_IOS_DEVICE, its UDID; virtuoso builds
//! its agent on first use). The fixture app (agent/ios/Fixture) must be installed.

use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use virtuoso::device::{Device, LaunchOptions, Lift};
use virtuoso::ios::IosDevice;
use virtuoso::screen::ScreenTree;

const SETTINGS: &str = "com.apple.Preferences";
const FIXTURE: &str = "dev.virtuoso.fixture";

/// One agent for every test: starting one takes most of a minute.
fn device() -> Option<MutexGuard<'static, IosDevice>> {
    static DEV: OnceLock<Option<Mutex<IosDevice>>> = OnceLock::new();
    DEV.get_or_init(|| {
        let udid = std::env::var("VIRTUOSO_IOS_DEVICE").ok()?;
        Some(Mutex::new(IosDevice::connect(&udid).expect("connect to the simulator")))
    })
    .as_ref()
    .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

fn fresh(d: &mut IosDevice, app: &str) {
    dismiss_prompts(d);
    d.launch_app(app, &LaunchOptions { stop_app: true, ..Default::default() }).unwrap();
}

/// A system prompt a failed test left up covers every app after it: answered "Don't Allow".
fn dismiss_prompts(d: &mut IosDevice) {
    let Ok(s) = d.app_screen("com.apple.springboard") else { return };
    if let Some(e) = s.elements.iter().find(|e| &*e.class == "Button" && (e.label() == "Don’t Allow" || e.label() == "Cancel")) {
        let (x, y) = e.bounds.center();
        let _ = d.tap(x, y);
        settled(d);
    }
}

fn find(s: &ScreenTree, label: &str) -> Option<(i32, i32)> {
    s.elements.iter().enumerate().find(|(i, e)| e.label() == label && s.matchable(*i)).map(|(_, e)| e.bounds.center())
}

/// Waits up to `secs` for an element labelled `label`.
fn wait_for(d: &mut IosDevice, label: &str, secs: u64) -> Option<(i32, i32)> {
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(p) = find(&d.screen().unwrap(), label) {
            return Some(p);
        }
        if std::time::Instant::now() > end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// What is on screen, for a failure message: the labels, and a screenshot in the temp folder
/// (CI uploads it).
fn seen(d: &mut IosDevice, name: &str) -> String {
    if let Ok(png) = d.screenshot() {
        let _ = std::fs::write(std::env::temp_dir().join(format!("ios-{name}.png")), png);
    }
    let s = d.screen().unwrap();
    let labels: Vec<String> = s
        .elements
        .iter()
        .filter(|e| !e.label().is_empty() && !e.label().contains("scroll bar"))
        .map(|e| format!("{} ({} {} flags {})", e.label(), e.package, e.bounds, e.flags))
        .collect();
    labels.join(", ")
}

/// Waits for the screen to stop changing after an action, as the runner does.
fn settled(d: &mut IosDevice) {
    d.settle(Duration::from_millis(300), Duration::from_secs(5)).unwrap();
}

fn tap_on(d: &mut IosDevice, label: &str) {
    let (x, y) = wait_for(d, label, 10).unwrap_or_else(|| panic!("no {label} on screen"));
    d.tap(x, y).unwrap();
    settled(d);
}

#[test]
fn reads_the_screen_of_a_launched_app() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, SETTINGS);
    assert!(wait_for(&mut d, "General", 10).is_some(), "Settings' main screen lists General");
}

#[test]
fn a_tap_opens_what_it_lands_on() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, SETTINGS);
    tap_on(&mut d, "General");
    let about = wait_for(&mut d, "About", 10).is_some();
    // Settings reopens where it was left: back to its main screen for the tests after this.
    let back = d.screen().unwrap().elements.iter().find(|e| e.id == "BackButton" || (&*e.class == "Button" && e.label() == "Settings")).map(|e| e.bounds.center());
    if let Some((x, y)) = back {
        d.tap(x, y).unwrap();
        settled(&mut d);
    }
    assert!(about, "General lists About");
}

#[test]
fn text_goes_into_the_focused_field_and_can_be_erased() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let s = d.screen().unwrap();
    let field = s.elements.iter().find(|e| e.id == "name-field").expect("the name field");
    let (x, y) = field.bounds.center();
    d.tap(x, y).unwrap();
    settled(&mut d);
    d.input_text("Ada").unwrap();
    assert!(wait_for(&mut d, "Name length: 3", 5).is_some());
    d.erase_text(1).unwrap();
    assert!(wait_for(&mut d, "Name length: 2", 5).is_some());
    tap_on(&mut d, "Submit");
    assert!(wait_for(&mut d, "Hello, Ad", 5).is_some());
}

#[test]
fn a_field_shows_its_text_and_placeholder() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let s = d.screen().unwrap();
    let field = s.elements.iter().find(|e| e.id == "password-field").expect("the password field");
    assert_eq!(field.hint, "Password");
    assert!(field.has(virtuoso::screen::PASSWORD) && field.has(virtuoso::screen::EDITABLE));
}

#[test]
fn a_swipe_scrolls_a_list() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let s = d.screen().unwrap();
    let (w, h) = (s.width, s.height);
    for _ in 0..3 {
        d.swipe((w / 2, h * 3 / 4), (w / 2, h / 4), 300, Lift::Still).unwrap();
    }
    assert!(find(&d.screen().unwrap(), "Row 1").is_none(), "Row 1 scrolled off");
}

#[test]
fn settle_reports_whether_the_screen_changed() {
    // On a screen that changes only when touched (Settings loads parts of its own late).
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    wait_for(&mut d, "Subscribe", 10).expect("the form");
    settled(&mut d);
    let still = d.settle(Duration::from_millis(300), Duration::from_secs(5)).unwrap();
    assert!(!still.changed && !still.timed_out, "{still:?}");
}

#[test]
fn a_number_changing_on_every_frame_does_not_hold_a_settle() {
    // Most of the screen holds still: the counter is left out, as an endless animation is.
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Counter");
    wait_for(&mut d, "Still text", 10).expect("the counter screen");
    let (x, y) = find(&d.screen().unwrap(), "Still text").unwrap();
    d.tap(x, y).unwrap();
    let start = std::time::Instant::now();
    let s = d.settle(Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    assert!(!s.timed_out && start.elapsed() < Duration::from_secs(3), "settled in {:?}: {s:?}", start.elapsed());
}

#[test]
fn a_screenshot_is_a_png() {
    let Some(mut d) = device() else { return };
    let png = d.screenshot().unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[test]
fn a_link_opens_the_app_that_handles_it() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    // As the runner opens it: into the flow's app.
    d.open_link("virtuoso-fixture://hello", Some(FIXTURE), false).unwrap();
    let opened = wait_for(&mut d, "Opened virtuoso-fixture://hello", 10).is_some();
    let sb = d.app_screen("com.apple.springboard").map(|s| {
        s.elements.iter().filter(|e| e.label().contains("Open") || &*e.class == "Alert" || &*e.class == "Sheet").map(|e| format!("{} [{}]", e.label(), e.class)).collect::<Vec<_>>().join(", ")
    });
    assert!(opened, "on screen: {}; in SpringBoard: {sb:?}", seen(&mut d, "link"));
}

#[test]
fn dark_mode_can_be_set_and_read() {
    let Some(mut d) = device() else { return };
    d.set_dark_mode(Some(true)).unwrap();
    assert!(d.dark_mode().unwrap());
    d.set_dark_mode(None).unwrap();
    assert!(!d.dark_mode().unwrap());
}

#[test]
fn location_permissions_and_clipboard_reach_the_simulator() {
    let Some(mut d) = device() else { return };
    d.set_location(-33.8688, 151.2093).unwrap();
    d.set_permissions(FIXTURE, &[("location".into(), "allow".into())]).unwrap();
    d.set_clipboard("copied").unwrap();
}

#[test]
fn a_relaunched_app_is_read_from_its_new_process() {
    // Each launch stops the app it runs (simctl launch --terminate-running-process): the read
    // right after must reach the new process, not the stopped one ("Lost connection").
    let Some(mut d) = device() else { return };
    for _ in 0..5 {
        fresh(&mut d, SETTINGS);
        let s = d.screen().expect("the relaunched app's screen");
        assert!(!s.elements.is_empty());
    }
}

#[test]
fn the_home_key_leaves_the_app() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, SETTINGS);
    wait_for(&mut d, "General", 10).expect("Settings");
    d.press_key(virtuoso::device::Key::Home).unwrap();
    settled(&mut d);
    let gone = find(&d.screen().unwrap(), "General").is_none();
    assert!(gone, "Settings is gone; on screen: {}", seen(&mut d, "home"));
}

#[test]
fn hide_keyboard_puts_the_keyboard_away() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let s = d.screen().unwrap();
    let (x, y) = s.elements.iter().find(|e| e.id == "name-field").expect("the name field").bounds.center();
    d.tap(x, y).unwrap();
    settled(&mut d);
    let keyboard = |d: &mut IosDevice| d.screen().unwrap().elements.iter().any(|e| e.has(virtuoso::screen::KEYBOARD) && e.bounds.area() > 0);
    let end = std::time::Instant::now() + Duration::from_secs(3);
    while !keyboard(&mut d) && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(keyboard(&mut d), "the keyboard is up; on screen: {}", seen(&mut d, "keyboard"));
    d.hide_keyboard().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    assert!(!keyboard(&mut d), "the keyboard is away");
}

/// The keyboard up for the fixture's email field on `screen` (Wizard or Stubborn).
fn email_keyboard(d: &mut IosDevice, screen: &str) -> impl Fn(&mut IosDevice) -> bool {
    fresh(d, FIXTURE);
    tap_on(d, screen);
    settled(d);
    let s = d.screen().unwrap();
    let (x, y) = s.elements.iter().find(|e| e.id == "wizard-email").expect("the email field").bounds.center();
    d.tap(x, y).unwrap();
    settled(d);
    let keyboard = |d: &mut IosDevice| d.screen().unwrap().elements.iter().any(|e| e.has(virtuoso::screen::KEYBOARD) && e.bounds.area() > 0);
    let end = std::time::Instant::now() + Duration::from_secs(3);
    while !keyboard(d) && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(keyboard(d), "the keyboard is up; on screen: {}", seen(d, "email-keyboard"));
    keyboard
}

#[test]
fn hide_keyboard_closes_one_only_a_touch_beside_the_field_closes() {
    let Some(mut d) = device() else { return };
    let keyboard = email_keyboard(&mut d, "Wizard");
    d.hide_keyboard().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    assert!(!keyboard(&mut d), "the keyboard is away; on screen: {}", seen(&mut d, "wizard"));
}

#[test]
fn hide_keyboard_leaves_a_keyboard_nothing_closes_without_failing() {
    // Settings' search field (at the bottom on iOS 26) keeps its keyboard whatever is tried, and
    // a flow that hides it goes on fine: hideKeyboard does what it can, and the flow carries on.
    let Some(mut d) = device() else { return };
    let _up = email_keyboard(&mut d, "Stubborn");
    d.hide_keyboard().expect("hideKeyboard carries on");
}

#[test]
fn launch_arguments_reach_the_app() {
    let Some(mut d) = device() else { return };
    let o = LaunchOptions { stop_app: true, arguments: vec![("greeting".into(), "hi".into())], ..Default::default() };
    d.launch_app(FIXTURE, &o).unwrap();
    let shown = wait_for(&mut d, "Greeting: hi", 10).is_some();
    assert!(shown, "on screen: {}", seen(&mut d, "arguments"));
}

#[test]
fn a_drag_moves_the_content_under_it() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let s = d.screen().unwrap();
    let (w, h) = (s.width, s.height);
    d.drag((w / 2, h * 3 / 4), (w / 2, h / 4), 500, 500).unwrap();
    assert!(find(&d.screen().unwrap(), "Row 1").is_none(), "Row 1 dragged off");
}

#[test]
fn media_recording_and_the_keychain() {
    let Some(mut d) = device() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("photo.png");
    std::fs::write(&png, d.screenshot().unwrap()).unwrap();
    d.add_media(&[png.as_path()]).unwrap();
    d.clear_keychain().unwrap();
    let video = dir.path().join("run.mp4");
    d.start_recording(&video).unwrap();
    fresh(&mut d, SETTINGS);
    std::thread::sleep(Duration::from_secs(2));
    d.stop_recording().unwrap();
    assert!(std::fs::metadata(&video).map(|m| m.len()).unwrap_or(0) > 1000, "a video was written");
}

#[test]
fn the_runners_scroll_gesture_scrolls_a_list() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let s = d.screen().unwrap();
    let (w, h) = (s.width, s.height);
    // scrollUntilVisible's drag: 50 ms, from 75% to 20% of the screen, cancelled at the end.
    d.swipe((w / 2, h * 75 / 100), (w / 2, h * 20 / 100), 50, Lift::Cancel).unwrap();
    settled(&mut d);
    let gone = find(&d.screen().unwrap(), "Row 1").is_none();
    assert!(gone, "Row 1 scrolled off; on screen: {}", seen(&mut d, "scroll"));
}

#[test]
fn the_screen_settles_soon_after_a_scroll() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let s = d.screen().unwrap();
    let (w, h) = (s.width, s.height);
    d.swipe((w / 2, h * 75 / 100), (w / 2, h * 20 / 100), 50, Lift::Cancel).unwrap();
    let start = std::time::Instant::now();
    let r = d.settle(Duration::from_millis(300), Duration::from_secs(10)).unwrap();
    let took = start.elapsed();
    assert!(!r.timed_out && took < Duration::from_secs(3), "settled {r:?} in {took:?}");
}

#[test]
fn a_screen_read_is_quick() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let mut took = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        d.screen().unwrap();
        took.push(start.elapsed().as_millis());
    }
    took.sort();
    assert!(took[2] < 700, "screen reads took {took:?} ms");
}

/// The first row fully on screen, by its number.
fn first_row(s: &ScreenTree) -> Option<u32> {
    s.elements.iter().filter(|e| e.bounds.top >= 100).filter_map(|e| e.text.strip_prefix("Row ")?.parse().ok()).min()
}

#[test]
fn a_drag_without_a_fling_moves_the_list_about_as_far_as_the_finger() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let s = d.screen().unwrap();
    let (w, h) = (s.width, s.height);
    let top = |s: &ScreenTree, t: &str| s.elements.iter().find(|e| e.text == t).map(|e| e.bounds.top);
    let row_h = top(&s, "Row 3").unwrap() - top(&s, "Row 2").unwrap();
    let before = first_row(&s).unwrap();
    // scrollUntilVisible's drag: 55% of the screen, cancelled at the end so nothing coasts.
    d.swipe((w / 2, h * 75 / 100), (w / 2, h * 20 / 100), 50, Lift::Cancel).unwrap();
    settled(&mut d);
    let after = first_row(&d.screen().unwrap()).unwrap();
    let rows = (after - before) as i32;
    let finger = h * 55 / 100 / row_h;
    assert!(rows >= finger / 2 && rows <= finger * 3 / 2, "moved {rows} rows for a {finger}-row drag");
}

#[test]
fn a_system_permission_prompt_shows_over_the_app_and_takes_a_tap() {
    let Some(mut d) = device() else { return };
    // Launched with location unset, as a flow says it (otherwise a launch grants everything).
    let unset = vec![("location".to_string(), "unset".to_string())];
    d.set_permissions(FIXTURE, &unset).unwrap();
    dismiss_prompts(&mut d);
    d.launch_app(FIXTURE, &LaunchOptions { stop_app: true, permissions: unset, ..Default::default() }).unwrap();
    tap_on(&mut d, "Location");
    tap_on(&mut d, "Ask for location");
    let allow = wait_for(&mut d, "Allow While Using App", 10);
    assert!(allow.is_some(), "the prompt; on screen: {}", seen(&mut d, "prompt"));
    tap_on(&mut d, "Allow While Using App");
    let allowed = wait_for(&mut d, "Location: allowed", 10).is_some();
    assert!(allowed, "on screen: {}", seen(&mut d, "allowed"));
}

#[test]
fn turning_the_device_turns_the_screen() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    wait_for(&mut d, "Form", 10).expect("the fixture");
    d.set_orientation(virtuoso::device::Orientation::LandscapeLeft).unwrap();
    settled(&mut d);
    let s = d.screen().unwrap();
    d.set_orientation(virtuoso::device::Orientation::Portrait).unwrap();
    settled(&mut d);
    assert!(s.width > s.height, "landscape is wider than tall: {}x{}", s.width, s.height);
    let form = s.elements.iter().find(|e| e.label() == "Form").expect("Form").bounds;
    assert!(form.right <= s.width && form.bottom <= s.height, "Form {form} is on the {}x{} screen", s.width, s.height);
}

#[test]
fn the_notification_centre_opens() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    wait_for(&mut d, "Form", 10).expect("the fixture");
    d.open_notifications().unwrap();
    settled(&mut d);
    let s = d.screen().unwrap();
    let springboard = s.elements.iter().any(|e| &*e.package == "com.apple.springboard" && e.bounds.area() > 0);
    let covered = find(&s, "Form").is_none();
    let what = seen(&mut d, "notifications");
    // What SpringBoard's own tree shows while it is open: the mark to tell it by.
    let sb = d.app_screen("com.apple.springboard").map(|s| {
        s.elements.iter().filter(|e| e.bounds.area() > 0 && (!e.id.is_empty() || !e.label().is_empty())).take(40).map(|e| format!("{}#{}[{}]", e.label(), e.id, e.class)).collect::<Vec<_>>().join(", ")
    });
    let what = format!("{what}; SpringBoard: {sb:?}");
    d.press_key(virtuoso::device::Key::Home).unwrap();
    assert!(springboard && covered, "on screen: {what}");
}

#[test]
fn the_app_element_is_not_named_like_text_on_screen() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, SETTINGS);
    wait_for(&mut d, "General", 10).expect("Settings");
    let s = d.screen().unwrap();
    // A sub-screen's back button reads "Settings": the app itself must not, or a tap on that
    // text would take the whole screen.
    let app = s.elements.iter().find(|e| &*e.class == "Application").expect("the app element");
    assert_eq!(app.label(), "", "the app element carries no text");
}

#[test]
fn a_screen_read_carries_the_agents_hash_of_it() {
    // A wait asks for the screen only when it is no longer this one.
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let s = d.fresh_screen().unwrap();
    let seq = s.seq.expect("the read's hash");
    assert_eq!(d.screen_unless(seq).unwrap().map(|t| t.elements.len()), None, "a still screen is unchanged");
    assert!(d.screen_unless(seq ^ 1).unwrap().is_some(), "another read's hash: the screen comes");
}

#[test]
fn a_tap_on_an_unchanged_screen_is_checked_and_made_in_one_request() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    wait_for(&mut d, "Form", 10).expect("the fixture's list");
    settled(&mut d);
    let s = d.fresh_screen().unwrap();
    let (x, y) = find(&s, "Form").unwrap();
    // Not the screen the lookup read: not tapped, and the screen as it is now comes back.
    let mut other = s.clone();
    other.seq = s.seq.map(|q| q ^ 1);
    let now = d.tap_if_unchanged(&other, x, y).unwrap().expect("the screen, untapped");
    assert!(find(&now, "Form").is_some());
    let s = d.fresh_screen().unwrap();
    assert!(d.tap_if_unchanged(&s, x, y).unwrap().is_none(), "tapped");
    settled(&mut d);
    assert!(wait_for(&mut d, "Subscribe", 10).is_some(), "the tap opened the form: {}", seen(&mut d, "tap-if-unchanged"));
}

#[test]
fn a_tap_returns_without_waiting_for_testmanagerds_answer() {
    // testmanagerd confirms a touch ~250 ms after playing it: the agent returns after the touch
    // and collects the answer before the next one.
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let (x, y) = wait_for(&mut d, "Name length: 0", 10).expect("the form");
    let mut took = Vec::new();
    for _ in 0..5 {
        settled(&mut d);
        let start = std::time::Instant::now();
        d.tap(x, y).unwrap();
        took.push(start.elapsed().as_millis());
    }
    took.sort();
    assert!(took[2] < 180, "taps took {took:?} ms");
}

#[test]
fn a_drag_returns_soon_after_it_lifts() {
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Long list");
    wait_for(&mut d, "Row 1", 10).expect("the list");
    let (w, h) = (d.screen().unwrap().width, d.screen().unwrap().height);
    let mut took = Vec::new();
    for _ in 0..3 {
        settled(&mut d);
        let start = std::time::Instant::now();
        // scrollUntilVisible's drag: 390 ms of moves, then the lift.
        d.swipe((w / 2, h * 75 / 100), (w / 2, h * 20 / 100), 50, Lift::Cancel).unwrap();
        took.push(start.elapsed().as_millis());
    }
    took.sort();
    assert!(took[0] < 460, "drags took {took:?} ms");
}

#[test]
fn a_read_fetches_only_what_a_screen_uses_and_reads_the_same() {
    // XCTest's snapshot fetches every attribute of every element; the agent asks for the few a
    // screen uses (WebDriverAgent's attribute snapshot), and the screen is the same.
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    wait_for(&mut d, "Subscribe", 10).expect("the form");
    settled(&mut d);
    let trimmed = d.fresh_screen().unwrap();
    assert!(d.trimmed(), "the read fetched every attribute");
    let full = d.full_screen().unwrap();
    assert_eq!(trimmed.elements, full.elements);
}

#[test]
fn a_long_press_returns_soon_after_it_lifts() {
    // As a tap does: back 100 ms after the lift, testmanagerd's answer collected before the
    // next touch (XCTest's own press waited for the app to idle first, and for the answer).
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let (x, y) = wait_for(&mut d, "Name length: 0", 10).expect("the form");
    let mut took = Vec::new();
    for _ in 0..3 {
        settled(&mut d);
        let start = std::time::Instant::now();
        d.long_press(x, y, 500).unwrap();
        took.push(start.elapsed().as_millis());
    }
    took.sort();
    assert!(took[1] < 680, "500 ms presses took {took:?} ms");
}

#[test]
fn the_clipboard_is_set_in_the_simulator_quickly() {
    // `simctl pbcopy` took ~5 s on a CI runner: the agent sets the pasteboard itself.
    let Some(mut d) = device() else { return };
    let text = format!("copied {}", std::process::id());
    let start = std::time::Instant::now();
    d.set_clipboard(&text).unwrap();
    let took = start.elapsed().as_millis();
    let udid = std::env::var("VIRTUOSO_IOS_DEVICE").unwrap();
    let out = std::process::Command::new("xcrun").args(["simctl", "pbpaste", &udid]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), text);
    assert!(took < 500, "setting the clipboard took {took} ms");
}

#[test]
fn typing_returns_once_the_keys_are_sent() {
    // testmanagerd confirms keys ~250 ms after the last: the settle after typing collects its
    // answer (and the next touch or keys wait for it), typing does not.
    let Some(mut d) = device() else { return };
    fresh(&mut d, FIXTURE);
    tap_on(&mut d, "Form");
    let s = d.screen().unwrap();
    let field = s.elements.iter().find(|e| e.id == "name-field").expect("the name field");
    let (x, y) = field.bounds.center();
    d.tap(x, y).unwrap();
    settled(&mut d);
    let start = std::time::Instant::now();
    d.input_text("Grace").unwrap();
    let took = start.elapsed().as_millis();
    settled(&mut d);
    assert!(wait_for(&mut d, "Name length: 5", 5).is_some(), "every key landed");
    // What is left is the first character landing before the rest go (iOS drops keys sent
    // while the keyboard reacts to it): ~0.6 s, against ~1.2 s waiting for the last key too.
    assert!(took < 800, "typing 5 characters took {took} ms");
}
