//! iOS behaviour on a booted simulator (needs VIRTUOSO_IOS_DEVICE, its UDID, and
//! VIRTUOSO_IOS_AGENT, the agent's .xctestrun built from agent/ios). The fixture app
//! (agent/ios/Fixture) must be installed.

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

fn tap_on(d: &mut IosDevice, label: &str) {
    let (x, y) = wait_for(d, label, 10).unwrap_or_else(|| panic!("no {label} on screen"));
    d.tap(x, y).unwrap();
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
    d.open_link("virtuoso-fixture://hello", None, false).unwrap();
    assert!(wait_for(&mut d, "Opened virtuoso-fixture://hello", 10).is_some());
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
    std::thread::sleep(Duration::from_secs(1));
    assert!(find(&d.screen().unwrap(), "General").is_none(), "Settings is gone");
}
