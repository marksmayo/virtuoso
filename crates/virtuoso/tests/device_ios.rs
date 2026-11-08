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
    d.launch_app(app, &LaunchOptions { stop_app: true, ..Default::default() }).unwrap();
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
    let labels: Vec<String> = s.elements.iter().filter(|e| !e.label().is_empty()).map(|e| format!("{} ({})", e.label(), e.package)).collect();
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
    assert!(wait_for(&mut d, "About", 10).is_some(), "General lists About");
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
    let Some(mut d) = device() else { return };
    fresh(&mut d, SETTINGS);
    wait_for(&mut d, "General", 10).expect("Settings");
    settled(&mut d);
    let still = d.settle(Duration::from_millis(300), Duration::from_secs(5)).unwrap();
    assert!(!still.changed && !still.timed_out);
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
        s.elements.iter().filter(|e| e.label().contains("Open") || e.class == "Alert" || e.class == "Sheet").map(|e| format!("{} [{}]", e.label(), e.class)).collect::<Vec<_>>().join(", ")
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
    std::thread::sleep(Duration::from_secs(1));
    assert!(keyboard(&mut d), "the keyboard is up");
    d.hide_keyboard().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    assert!(!keyboard(&mut d), "the keyboard is away");
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
    let row_h = s.elements.iter().find(|e| e.text == "Row 2").map(|e| e.bounds.height()).unwrap_or(44);
    let before = first_row(&s).unwrap();
    // scrollUntilVisible's drag: 55% of the screen, cancelled at the end so nothing coasts.
    d.swipe((w / 2, h * 75 / 100), (w / 2, h * 20 / 100), 50, Lift::Cancel).unwrap();
    settled(&mut d);
    let after = first_row(&d.screen().unwrap()).unwrap();
    let rows = (after - before) as i32;
    let finger = h * 55 / 100 / row_h;
    assert!(rows >= finger / 2 && rows <= finger * 3 / 2, "moved {rows} rows for a {finger}-row drag");
}
