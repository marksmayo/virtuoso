//! Running flows against a scripted fake device.

use virtuoso::flow::parse_flow;
use virtuoso::runner::{FlowStatus, RunOptions, Runner};
use virtuoso::testing::{node, FakeDevice, FakeScreen};
use std::time::Duration;

fn opts() -> RunOptions {
    RunOptions {
        lookup_timeout: Duration::from_millis(300),
        optional_lookup_timeout: Duration::from_millis(150),
        condition_timeout: Duration::from_millis(150),
        ..Default::default()
    }
}

fn run(dev: &mut FakeDevice, yaml: &str) -> virtuoso::runner::FlowResult {
    let flow = parse_flow(yaml, None).unwrap();
    Runner::new(dev, opts()).run_flow(&flow)
}

/// Home has a "Sign in" button that leads to a "Welcome" screen.
fn app() -> FakeDevice {
    let mut d = FakeDevice::new();
    d.add_screen("home", FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160]).goes_to("welcome")]));
    d.add_screen("welcome", FakeScreen::new(vec![node("Welcome", [100, 100, 300, 160])]));
    d.show("home");
    d
}

#[test]
fn a_flow_launches_taps_and_asserts() {
    let mut d = app();
    let r = run(&mut d, "appId: com.app\n---\n- launchApp\n- tapOn: Sign in\n- assertVisible: Welcome\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["launch com.app", "tap 200,130"]);
    assert_eq!(r.steps.len(), 3);
}

#[test]
fn a_missing_element_fails_the_flow_with_a_clear_error() {
    let mut d = app();
    let r = run(&mut d, "appId: com.app\n---\n- tapOn: Register\n- back\n");
    assert_eq!(r.status, FlowStatus::Failed);
    let err = r.error.unwrap();
    assert!(err.contains("Register"), "{err}");
    assert!(d.log.is_empty(), "nothing after the failure runs: {:?}", d.log);
}

#[test]
fn a_missing_element_error_says_what_was_on_screen() {
    let mut d = app();
    let r = run(&mut d, "appId: com.app\n---\n- tapOn: Register\n");
    let err = r.error.unwrap();
    assert!(err.contains("on screen: \"Sign in\""), "{err}");
}

#[test]
fn an_optional_step_that_fails_does_not_fail_the_flow() {
    let mut d = app();
    let r = run(&mut d, "appId: com.app\n---\n- tapOn:\n    text: Allow\n    optional: true\n- tapOn: Sign in\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 200,130"]);
}

#[test]
fn an_element_that_appears_late_is_waited_for() {
    let mut d = app();
    d.show_after_reads("welcome", 3);
    let r = run(&mut d, "appId: com.app\n---\n- assertVisible: Welcome\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
}

fn passes(d: &mut FakeDevice, cmds: &str) {
    let r = run(d, &format!("appId: com.app\n---\n{cmds}"));
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
}

fn fails(d: &mut FakeDevice, cmds: &str) -> String {
    let r = run(d, &format!("appId: com.app\n---\n{cmds}"));
    assert_eq!(r.status, FlowStatus::Failed);
    r.error.unwrap()
}

#[test]
fn input_text_types_with_variables_expanded() {
    let mut d = app();
    passes(&mut d, "- evalScript: ${output.user = 'ann'}\n- inputText: hi ${output.user}\n");
    assert_eq!(d.log, vec!["text hi ann"]);
}

#[test]
fn erase_text_defaults_to_fifty_characters() {
    let mut d = app();
    passes(&mut d, "- eraseText\n- eraseText: 3\n");
    assert_eq!(d.log, vec!["erase 50", "erase 3"]);
}

#[test]
fn random_input_has_the_requested_shape() {
    let mut d = app();
    passes(&mut d, "- inputRandomEmail\n- inputRandomNumber:\n    length: 6\n- inputRandomText:\n    length: 5\n- inputRandomPersonName\n");
    let typed: Vec<&str> = d.log.iter().map(|l| l.strip_prefix("text ").unwrap()).collect();
    assert!(typed[0].contains('@'), "{}", typed[0]);
    assert!(typed[1].len() == 6 && typed[1].chars().all(|c| c.is_ascii_digit()), "{}", typed[1]);
    assert_eq!(typed[2].chars().count(), 5);
    assert!(typed[3].contains(' '));
}

#[test]
fn copy_text_from_then_paste_types_the_copied_text() {
    let mut d = app();
    passes(&mut d, "- copyTextFrom: Sign in\n- pasteText\n- assertTrue: ${maestro.copiedText == 'Sign in'}\n");
    assert_eq!(d.log, vec!["text Sign in"]);
}

#[test]
fn set_clipboard_feeds_paste() {
    let mut d = app();
    passes(&mut d, "- setClipboard: code-42\n- pasteText\n");
    assert_eq!(d.log, vec!["clipboard code-42", "text code-42"]);
}

#[test]
fn assert_not_visible_passes_once_the_element_is_gone() {
    let mut d = app();
    d.show_after_reads("welcome", 3);
    passes(&mut d, "- assertNotVisible: Sign in\n");
    let err = fails(&mut app(), "- assertNotVisible: Sign in\n");
    assert!(err.contains("Sign in"), "{err}");
}

#[test]
fn extended_wait_until_uses_its_own_timeout() {
    let mut d = app();
    d.show_after_reads("welcome", 2);
    passes(&mut d, "- extendedWaitUntil:\n    visible: Welcome\n    timeout: 2000\n");
    let mut d = app();
    let start = std::time::Instant::now();
    fails(&mut d, "- extendedWaitUntil:\n    visible: Never\n    timeout: 100\n");
    assert!(start.elapsed() < Duration::from_millis(280), "used the lookup timeout instead");
}

#[test]
fn assert_true_checks_a_javascript_condition() {
    passes(&mut app(), "- assertTrue: ${1 + 1 == 2}\n");
    let err = fails(&mut app(), "- assertTrue: ${1 + 1 == 3}\n");
    assert!(err.contains("1 + 1 == 3"), "{err}");
}

#[test]
fn wait_for_animation_to_end_settles_from_a_mark() {
    passes(&mut app(), "- waitForAnimationToEnd\n- waitForAnimationToEnd:\n    timeout: 500\n");
}

#[test]
fn tap_on_a_point_takes_pixels_or_percent_of_the_screen() {
    let mut d = app();
    passes(&mut d, "- tapOn:\n    point: 50%,90%\n- tapOn:\n    point: 10, 20\n");
    assert_eq!(d.log, vec!["tap 540,2160", "tap 10,20"]);
}

#[test]
fn a_point_with_a_selector_is_relative_to_the_element() {
    let mut d = FakeDevice::new();
    d.add_screen("s", FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160])]));
    d.show("s");
    passes(&mut d, "- tapOn:\n    text: Sign in\n    point: 0%,0%\n- tapOn:\n    text: Sign in\n    point: 100%,50%\n");
    assert_eq!(d.log, vec!["tap 100,100", "tap 300,130"]);
}

#[test]
fn repeat_taps_several_times_as_one_gesture() {
    // One gesture to the device, its taps `delay` apart: iOS plays them as one (an app's
    // double-tap handler sees them evenly spaced), Android taps one after another.
    let mut d = FakeDevice::new();
    d.add_screen("s", FakeScreen::new(vec![node("Add", [0, 0, 100, 100])]));
    d.show("s");
    passes(&mut d, "- tapOn:
    text: Add
    repeat: 3
    delay: 1
- doubleTapOn: Add
");
    assert_eq!(d.log, vec!["taps 50,50 3x 1ms", "taps 50,50 2x 100ms"]);
}

#[test]
fn long_press_holds_the_element() {
    let mut d = app();
    passes(&mut d, "- longPressOn: Sign in\n");
    assert_eq!(d.log, vec!["long press 200,130 1000ms"]);
}

#[test]
fn index_chooses_among_matches() {
    let mut d = FakeDevice::new();
    d.add_screen("s", FakeScreen::new(vec![node("Row", [0, 0, 100, 100]), node("Row", [0, 200, 100, 300])]));
    d.show("s");
    passes(&mut d, "- tapOn:\n    text: Row\n    index: 1\n");
    assert_eq!(d.log, vec!["tap 50,250"]);
}

#[test]
fn retry_tap_if_no_change_taps_again_when_nothing_happened() {
    let mut d = FakeDevice::new();
    d.add_screen("s", FakeScreen::new(vec![node("Dead", [0, 0, 100, 100])]));
    d.show("s");
    passes(&mut d, "- tapOn:\n    text: Dead\n    retryTapIfNoChange: true\n");
    assert_eq!(d.log, vec!["tap 50,50", "tap 50,50"]);
    let mut d = app();
    passes(&mut d, "- tapOn:\n    text: Sign in\n    retryTapIfNoChange: true\n");
    assert_eq!(d.log.len(), 1, "the screen changed: no second tap");
}

#[test]
fn swipe_directions_cross_the_screen() {
    let mut d = app();
    passes(&mut d, "- swipe:\n    direction: LEFT\n- swipe:\n    direction: UP\n    duration: 200\n");
    assert_eq!(d.log, vec!["swipe 972,1200 -> 108,1200 400ms fling", "swipe 540,1200 -> 540,240 200ms fling"]);
}

#[test]
fn swipe_start_and_end_take_percent_or_pixels() {
    let mut d = app();
    passes(&mut d, "- swipe:\n    start: 90%, 50%\n    end: 10%, 50%\n- swipe:\n    start: 100, 200\n    end: 300, 400\n");
    assert_eq!(d.log, vec!["swipe 972,1200 -> 108,1200 400ms fling", "swipe 100,200 -> 300,400 400ms fling"]);
}

#[test]
fn swipe_from_an_element_starts_at_its_center() {
    let mut d = app();
    passes(&mut d, "- swipe:\n    from: Sign in\n    direction: RIGHT\n");
    assert_eq!(d.log, vec!["swipe 200,130 -> 1080,130 400ms fling"]);
}

#[test]
fn scroll_until_visible_scrolls_until_the_element_shows() {
    let mut d = FakeDevice::new();
    d.add_screen("top", FakeScreen::new(vec![node("Header", [0, 0, 100, 100])]).scrolls_to("mid"));
    d.add_screen("mid", FakeScreen::new(vec![node("Body", [0, 0, 100, 100])]).scrolls_to("end"));
    d.add_screen("end", FakeScreen::new(vec![node("Footer", [0, 1000, 100, 1100])]));
    d.show("top");
    passes(&mut d, "- scrollUntilVisible:\n    element: Footer\n");
    assert_eq!(d.log.iter().filter(|l| l.starts_with("swipe")).count(), 2, "{:?}", d.log);
    assert!(!d.log[0].ends_with("fling"), "scrolling to an element drags without flinging");
}

/// The Tap button, pushed down by a banner that came late, as the tap lands where it was.
fn shifted(platform: &'static str) -> FakeDevice {
    let mut d = FakeDevice::new();
    d.platform = platform;
    d.add_screen("before", FakeScreen::new(vec![node("Intro", [0, 0, 1080, 90]), node("Tap", [0, 100, 100, 200]).goes_to("after")]));
    d.add_screen("after", FakeScreen::new(vec![node("Intro", [0, 0, 1080, 90]), node("Banner", [0, 100, 1080, 290]), node("Tap", [0, 300, 100, 400])]));
    d.show("before");
    d
}

#[test]
fn on_ios_a_tap_the_element_moved_away_from_is_tapped_again_where_it_is() {
    let mut d = shifted("ios");
    passes(&mut d, "- tapOn: Tap\n");
    assert_eq!(d.log, vec!["tap 50,150", "tap 50,350"]);
    // Android is left as it was: one tap.
    let mut d = shifted("android");
    passes(&mut d, "- tapOn: Tap\n");
    assert_eq!(d.log, vec!["tap 50,150"]);
}

/// A popup's button, over a screen with an endless spinner: the app still animating the popup
/// in takes no touch.
fn popup(busy_taps: u32) -> FakeDevice {
    let mut d = FakeDevice::new();
    d.platform = "ios";
    d.add_screen("popup", FakeScreen::new(vec![node("Badge unlocked", [0, 0, 1080, 90]), node("Nice", [0, 100, 100, 200]).goes_to("closed")]));
    d.add_screen("closed", FakeScreen::new(vec![node("Shop", [0, 0, 1080, 90])]));
    d.show("popup");
    d.busy_taps = busy_taps;
    d
}

#[test]
fn on_ios_a_tap_the_app_took_while_animating_and_ignored_is_made_again() {
    let mut d = popup(1);
    passes(&mut d, "- tapOn: Nice\n- assertVisible: Shop\n");
    assert_eq!(d.log, vec!["tap 50,150", "tap 50,150"]);
}

#[test]
fn a_tap_that_changed_nothing_on_an_idle_app_is_not_made_again() {
    let mut d = FakeDevice::new();
    d.platform = "ios";
    d.add_screen("s", FakeScreen::new(vec![node("Label", [0, 100, 100, 200])]));
    d.show("s");
    passes(&mut d, "- tapOn: Label\n");
    assert_eq!(d.log, vec!["tap 50,150"]);
}

#[test]
fn center_element_drags_its_list_not_what_covers_the_element() {
    // The element sits under a header (iOS reports a clipped element's whole frame): the drag
    // that centres it starts in its list, and goes as far as the element has to.
    let mut d = FakeDevice::new();
    d.add_screen(
        "s",
        FakeScreen::new(vec![
            node("Header", [0, 0, 1080, 400]),
            node("", [0, 400, 1080, 2400]).flags(virtuoso::screen::VISIBLE | virtuoso::screen::ENABLED | virtuoso::screen::SCROLLABLE),
            node("Product", [0, 150, 500, 450]).depth(2),
        ]),
    );
    d.show("s");
    passes(&mut d, "- scrollUntilVisible:\n    element: Product\n    centerElement: true\n");
    assert_eq!(d.log, vec!["swipe 540,1400 -> 540,2300 500ms cancel"]);
}

#[test]
fn scroll_until_visible_fails_at_its_timeout() {
    let mut d = app();
    let start = std::time::Instant::now();
    let err = fails(&mut d, "- scrollUntilVisible:\n    element: Nowhere\n    timeout: 200\n");
    assert!(err.contains("Nowhere"), "{err}");
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn scroll_until_visible_up_swipes_down() {
    let mut d = app();
    let _ = run(&mut d, "appId: a\n---\n- scrollUntilVisible:\n    element: Nowhere\n    direction: UP\n    timeout: 50\n");
    let (from, to) = d.log[0].strip_prefix("swipe ").unwrap().split_once(" -> ").unwrap();
    let y = |p: &str| p.split(',').nth(1).unwrap().split(' ').next().unwrap().parse::<i32>().unwrap();
    assert!(y(to) > y(from), "{}", d.log[0]);
}

#[test]
fn scroll_swipes_up_from_the_middle() {
    let mut d = app();
    passes(&mut d, "- scroll\n");
    assert_eq!(d.log, vec!["swipe 540,1200 -> 540,240 400ms fling"]);
}

#[test]
fn app_control_commands_use_the_flow_app_or_their_own() {
    let mut d = app();
    passes(&mut d, "- stopApp\n- killApp: com.other\n- clearState\n- launchApp:\n    appId: com.b\n    clearState: true\n    stopApp: false\n    arguments:\n      mode: test\n");
    assert_eq!(d.log, vec!["stop com.app", "kill com.other", "clear com.app", "launch com.b clearState noStop mode=test"]);
}

#[test]
fn launch_app_permissions_are_applied() {
    let mut d = app();
    passes(&mut d, "- launchApp:\n    permissions:\n      all: deny\n");
    assert_eq!(d.log, vec!["permissions com.app all=deny", "launch com.app"]);
}

#[test]
fn a_launchs_own_permissions_go_with_the_launch_on_ios() {
    // The iOS launch sets them itself, alongside clearState and the keychain reset (each
    // `simctl privacy` ~0.3 s), instead of in a step of their own before it.
    let mut d = app();
    d.platform = "ios";
    passes(&mut d, "- launchApp:\n    clearState: true\n    permissions:\n      all: deny\n");
    assert_eq!(d.log, vec!["launch com.app clearState permissions all=deny"]);
}

#[test]
fn launch_app_clears_the_keychain_when_asked() {
    let mut d = app();
    passes(&mut d, "- launchApp:\n    clearState: true\n    clearKeychain: true\n");
    // With the launch: iOS resets it while it clears the app's data and sets its permissions.
    assert_eq!(d.log, vec!["launch com.app clearState clearKeychain"]);
}

#[test]
fn an_app_id_may_come_from_a_variable() {
    let mut d = app();
    let r = Runner::new(&mut d, RunOptions { env: vec![("APP".into(), "com.var".into())], ..opts() })
        .run_flow(&parse_flow("appId: ${APP}\n---\n- launchApp\n", None).unwrap());
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["launch com.var"]);
}

#[test]
fn backspace_taps_the_keyboards_delete_key_when_one_shows() {
    use virtuoso::screen::{CLICKABLE, ENABLED, KEYBOARD, VISIBLE};
    let mut d = FakeDevice::new();
    d.add_screen(
        "typing",
        FakeScreen::new(vec![
            node("hello", [0, 100, 1080, 200]),
            node("", [914, 1963, 1075, 2118]).desc("Delete").flags(VISIBLE | ENABLED | CLICKABLE | KEYBOARD),
        ]),
    );
    d.show("typing");
    passes(&mut d, "- pressKey: Backspace\n");
    assert_eq!(d.log, vec!["tap 994,2040"]);
    // A field just tapped has focus before its keyboard is back up: wait for the keyboard.
    use virtuoso::screen::{EDITABLE, FOCUSED};
    let mut d = FakeDevice::new();
    d.add_screen("focused", FakeScreen::new(vec![node("hello", [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED)]));
    d.add_screen(
        "keyboard up",
        FakeScreen::new(vec![
            node("hello", [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED),
            node("", [914, 1963, 1075, 2118]).desc("Delete").flags(VISIBLE | ENABLED | CLICKABLE | KEYBOARD),
        ]),
    );
    d.show("focused");
    d.show_after_reads("keyboard up", 3);
    passes(&mut d, "- pressKey: Backspace\n");
    assert_eq!(d.log, vec!["tap 994,2040"]);
    // With no keyboard up there is no key to tap: the key event.
    let mut d = app();
    passes(&mut d, "- pressKey: Backspace\n");
    assert_eq!(d.log, vec!["key Backspace"]);
}

#[test]
fn a_key_the_keyboard_shown_lacks_is_pressed_without_waiting() {
    // iOS labels its keys "delete" and "return": with its keyboard up and a field focused, the
    // wait for a keyboard to come up waited 1.5 s for one that was already there.
    use virtuoso::screen::{CLICKABLE, EDITABLE, ENABLED, FOCUSED, KEYBOARD, VISIBLE};
    let key = |label: &str, b| node(label, b).flags(VISIBLE | ENABLED | CLICKABLE | KEYBOARD);
    let mut d = FakeDevice::new();
    d.add_screen(
        "typing",
        FakeScreen::new(vec![
            node("hello", [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED),
            key("delete", [914, 1963, 1075, 2118]),
            key("return", [914, 2118, 1075, 2274]),
        ]),
    );
    d.show("typing");
    let start = std::time::Instant::now();
    passes(&mut d, "- pressKey: Backspace\n- pressKey: Enter\n");
    assert!(start.elapsed() < std::time::Duration::from_millis(500), "{:?}", start.elapsed());
    assert_eq!(d.log, vec!["key Backspace", "key Enter"]);
    assert_eq!(d.fresh_reads, 2, "one look a key");
}

#[test]
fn enter_taps_the_keyboards_action_key_whatever_it_is_labelled() {
    // A keyboard composes what is typed and corrects it on an Enter key event ("hello" became
    // "Hello"); its own action key submits the text as it is.
    use virtuoso::screen::{CLICKABLE, ENABLED, KEYBOARD, VISIBLE};
    let mut d = FakeDevice::new();
    let key = |label: &str, id: &str, b| node("", b).desc(label).id(id).flags(VISIBLE | ENABLED | CLICKABLE | KEYBOARD);
    d.add_screen(
        "typing",
        FakeScreen::new(vec![
            node("hello", [0, 100, 1080, 200]),
            key("Emoji button", "com.google.android.inputmethod.latin:id/key_pos_switch_to_next_language", [272, 2118, 379, 2274]),
            key("Done", "com.google.android.inputmethod.latin:id/key_pos_ime_action", [914, 2118, 1075, 2274]),
        ]),
    );
    d.show("typing");
    passes(&mut d, "- pressKey: Enter\n");
    assert_eq!(d.log, vec!["tap 994,2196"]);
    // A keyboard that has just come up may not be in the agent's cached read yet.
    d.log.clear();
    d.add_screen("no keyboard yet", FakeScreen::new(vec![node("hello", [0, 100, 1080, 200])]));
    d.cache_stale("no keyboard yet");
    passes(&mut d, "- pressKey: Enter\n");
    assert_eq!(d.log, vec!["tap 994,2196"]);
}

#[test]
fn an_android_key_the_cached_read_shows_is_tapped_without_a_fresh_read() {
    // Each Backspace or Enter read the screen past the cache (on Android the agent walks the
    // whole keyboard again) though the keyboard that was up was in the cached read already.
    use virtuoso::screen::{CLICKABLE, EDITABLE, ENABLED, FOCUSED, KEYBOARD, VISIBLE};
    let mut d = FakeDevice::new();
    let key = |label: &str, b| node("", b).desc(label).flags(VISIBLE | ENABLED | CLICKABLE | KEYBOARD);
    d.add_screen(
        "typing",
        FakeScreen::new(vec![
            node("hello", [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED),
            key("Delete", [914, 1963, 1075, 2118]),
        ]),
    );
    d.show("typing");
    passes(&mut d, "- pressKey: Backspace\n- pressKey: Backspace\n");
    assert_eq!(d.log, vec!["tap 994,2040", "tap 994,2040"]);
    assert_eq!(d.fresh_reads, 0);
}

#[test]
fn open_link_passes_the_app_unless_a_browser_is_asked_for() {
    let mut d = app();
    passes(&mut d, "- openLink: https://x.test/a\n- openLink:\n    link: https://x.test/b\n    browser: true\n");
    assert_eq!(d.log, vec!["open https://x.test/a in com.app", "open https://x.test/b browser"]);
}

#[test]
fn press_key_maps_maestro_key_names() {
    let mut d = app();
    passes(&mut d, "- pressKey: Enter\n- pressKey: volume up\n- pressKey: Remote Dpad Down\n");
    assert_eq!(d.log, vec!["key Enter", "key VolumeUp", "key DpadDown"]);
    let err = fails(&mut app(), "- pressKey: Teleport\n");
    assert!(err.contains("Teleport"), "{err}");
}

#[test]
fn hide_keyboard_asks_the_device() {
    let mut d = app();
    d.keyboard = true;
    passes(&mut d, "- hideKeyboard\n");
    assert_eq!(d.log, vec!["hide keyboard"]);
}

#[test]
fn hide_keyboard_waits_for_a_focused_fields_keyboard_still_coming_up() {
    // A field just tapped has focus before its keyboard is up: a hideKeyboard right after found
    // no keyboard, did nothing, and the keyboard came up over the next step's list.
    use virtuoso::screen::{EDITABLE, ENABLED, FOCUSED, KEYBOARD, VISIBLE};
    let field = || node("Name", [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED);
    let mut d = FakeDevice::new();
    d.add_screen("typing", FakeScreen::new(vec![field()]));
    d.add_screen("keyboard", FakeScreen::new(vec![field(), node("Q", [0, 1800, 100, 1900]).flags(VISIBLE | ENABLED | KEYBOARD)]).back_to("typed"));
    d.add_screen("typed", FakeScreen::new(vec![field()]));
    d.show("typing");
    d.show_after_reads("keyboard", 2);
    passes(&mut d, "- hideKeyboard\n");
    assert_eq!(d.current, "typed", "the keyboard that came up is hidden");
}

#[test]
fn clear_keychain_is_a_no_op_on_android() {
    let mut d = app();
    passes(&mut d, "- clearKeychain\n");
}

fn run_file(d: &mut FakeDevice, dir: &std::path::Path, main: &str) -> virtuoso::runner::FlowResult {
    let path = dir.join("main.yaml");
    std::fs::write(&path, main).unwrap();
    let flow = parse_flow(main, Some(&path)).unwrap();
    Runner::new(d, opts()).run_flow(&flow)
}

#[test]
fn run_flow_runs_a_file_next_to_the_flow_with_its_env() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/type.yaml"), "appId: com.app\n---\n- inputText: ${WORD}\n").unwrap();
    let mut d = app();
    let r = run_file(&mut d, dir.path(), "appId: com.app\n---\n- runFlow:\n    file: sub/type.yaml\n    env:\n      WORD: nested\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["text nested"]);
    assert!(r.steps.len() >= 2, "nested steps are reported too");
    assert_eq!(r.steps[1].depth, 2);
}

#[test]
fn run_flow_when_skips_unless_the_condition_holds() {
    let mut d = app();
    passes(
        &mut d,
        "- runFlow:\n    when:\n      visible: Sign in\n    commands:\n      - inputText: a\n- runFlow:\n    when:\n      visible: Cookies\n    commands:\n      - inputText: b\n- runFlow:\n    when:\n      notVisible: Cookies\n    commands:\n      - inputText: c\n- runFlow:\n    when:\n      platform: iOS\n    commands:\n      - inputText: d\n- runFlow:\n    when:\n      true: ${1 == 1}\n    commands:\n      - inputText: e\n",
    );
    assert_eq!(d.log, vec!["text a", "text c", "text e"]);
}

#[test]
fn a_skipped_run_flow_is_reported_as_skipped() {
    let mut d = app();
    let r = run(&mut d, "appId: a\n---\n- runFlow:\n    when:\n      platform: iOS\n    commands:\n      - back\n");
    assert_eq!(r.steps[0].status, virtuoso::runner::StepStatus::Skipped);
}

#[test]
fn a_failure_inside_run_flow_fails_the_flow() {
    let err = fails(&mut app(), "- runFlow:\n    commands:\n      - tapOn: Missing\n");
    assert!(err.contains("Missing"), "{err}");
}

#[test]
fn repeat_runs_commands_a_number_of_times_or_while_a_condition_holds() {
    let mut d = app();
    passes(&mut d, "- repeat:\n    times: 3\n    commands:\n      - inputText: x\n");
    assert_eq!(d.log, vec!["text x"; 3]);
    let mut d = FakeDevice::new();
    d.add_screen("a", FakeScreen::new(vec![node("Next", [0, 0, 100, 100]).goes_to("b")]));
    d.add_screen("b", FakeScreen::new(vec![node("Next", [0, 0, 100, 100]).goes_to("c")]));
    d.add_screen("c", FakeScreen::new(vec![node("Done", [0, 0, 100, 100])]));
    d.show("a");
    passes(&mut d, "- repeat:\n    while:\n      visible: Next\n    commands:\n      - tapOn: Next\n");
    assert_eq!(d.log.len(), 2);
}

#[test]
fn repeat_times_and_while_together_stop_at_whichever_ends_first() {
    let mut d = app();
    passes(&mut d, "- repeat:\n    times: 2\n    while:\n      visible: Sign in\n    commands:\n      - inputText: x\n");
    assert_eq!(d.log, vec!["text x"; 2]);
}

#[test]
fn retry_runs_commands_again_until_they_pass() {
    let mut d = app();
    d.show_after_reads("welcome", 12);
    passes(&mut d, "- retry:\n    maxRetries: 3\n    commands:\n      - assertVisible:\n          text: Welcome\n          optional: false\n");
    let err = fails(&mut app(), "- retry:\n    maxRetries: 1\n    commands:\n      - tapOn: Never\n");
    assert!(err.contains("Never"), "{err}");
}

#[test]
fn run_script_runs_a_file_and_its_output_is_visible_to_later_steps() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("setup.js"), "output.user = PREFIX + '-1'").unwrap();
    let mut d = app();
    let r = run_file(&mut d, dir.path(), "appId: a\n---\n- runScript:\n    file: setup.js\n    env:\n      PREFIX: u\n- inputText: ${output.user}\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["text u-1"]);
}

#[test]
fn hooks_run_around_the_flow_and_on_flow_complete_runs_after_a_failure() {
    let mut d = app();
    let r = run(&mut d, "appId: a\nonFlowStart:\n  - inputText: start\nonFlowComplete:\n  - inputText: end\n---\n- tapOn: Missing\n");
    assert_eq!(r.status, FlowStatus::Failed);
    assert_eq!(d.log, vec!["text start", "text end"]);
}

#[test]
fn flow_env_wins_over_command_line_env_and_can_use_it() {
    let mut d = app();
    let r = Runner::new(&mut d, RunOptions { env: vec![("A".into(), "cli".into()), ("B".into(), "cli".into())], ..opts() })
        .run_flow(&parse_flow("appId: a\nenv:\n  A: flow\n  C: ${B}-x\n---\n- inputText: ${A} ${B} ${C}\n", None).unwrap());
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["text flow cli cli-x"]);
}

#[test]
fn maestro_variables_are_set_for_every_flow() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = app();
    let r = run_file(&mut d, dir.path(), "appId: a\n---\n- inputText: ${MAESTRO_FILENAME} ${MAESTRO_DEVICE_UDID}\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["text main fake-1"]);
}

#[test]
fn orientation_location_and_modes_reach_the_device() {
    let mut d = app();
    passes(
        &mut d,
        "- setOrientation: LANDSCAPE_LEFT\n- setLocation:\n    latitude: 52.37\n    longitude: 4.89\n- setAirplaneMode: enabled\n- toggleAirplaneMode\n- setDarkMode: true\n- assertDarkMode\n- toggleDarkMode\n- assertLightMode\n",
    );
    assert_eq!(
        d.log,
        vec!["orientation LandscapeLeft", "location 52.37,4.89", "airplane true", "airplane false", "dark true", "dark false"]
    );
    let err = fails(&mut app(), "- assertDarkMode\n");
    assert!(err.to_lowercase().contains("dark"), "{err}");
    assert!(fails(&mut app(), "- setOrientation: SIDEWAYS\n").contains("SIDEWAYS"));
}

#[test]
fn travel_moves_the_location_along_its_points() {
    let mut d = app();
    passes(&mut d, "- travel:\n    points:\n      - 0.0,0.0\n      - 0.0,0.001\n    speed: 100000\n");
    assert_eq!(d.log.first().unwrap(), "location 0,0");
    assert_eq!(d.log.last().unwrap(), "location 0,0.001");
}

#[test]
fn travel_keeps_to_its_speed_however_long_an_update_takes() {
    // Each update is a simctl call on iOS (~0.1-0.2 s): it is taken out of the pause before
    // the next, not added to it. ~111 m at 55.66 m/s: updates at 1 s and 2 s.
    let mut d = app();
    d.location_takes = Duration::from_millis(400);
    let start = std::time::Instant::now();
    passes(&mut d, "- travel:\n    points:\n      - 0.0,0.0\n      - 0.0,0.001\n    speed: 55.66\n");
    let took = start.elapsed();
    assert_eq!(d.log.iter().filter(|l| l.starts_with("location")).count(), 3, "{:?}", d.log);
    // The last update starts at 2 s and takes 0.4 s; the old pauses made it 3.2 s.
    assert!(took >= Duration::from_secs(2) && took < Duration::from_millis(2800), "took {took:?}");
}

#[test]
fn set_permissions_uses_the_flow_app() {
    let mut d = app();
    passes(&mut d, "- setPermissions:\n    permissions:\n      camera: allow\n      location: deny\n");
    assert_eq!(d.log, vec!["permissions com.app camera=allow,location=deny"]);
}

#[test]
fn add_media_sends_files_next_to_the_flow() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cat.png"), b"png").unwrap();
    let mut d = app();
    let r = run_file(&mut d, dir.path(), "appId: a\n---\n- addMedia:\n    - cat.png\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["media cat.png"]);
    let r = run_file(&mut app(), dir.path(), "appId: a\n---\n- addMedia:\n    - missing.png\n");
    assert!(r.error.unwrap().contains("missing.png"));
}

fn run_with_output(d: &mut FakeDevice, flow_dir: &std::path::Path, out: &std::path::Path, yaml: &str) -> virtuoso::runner::FlowResult {
    let path = flow_dir.join("main.yaml");
    let flow = parse_flow(yaml, Some(&path)).unwrap();
    Runner::new(d, RunOptions { output_dir: Some(out.to_path_buf()), ..opts() }).run_flow(&flow)
}

#[test]
fn take_screenshot_writes_a_png_into_the_output_folder() {
    let flows = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let mut d = app();
    let r = run_with_output(&mut d, flows.path(), out.path(), "appId: a\n---\n- takeScreenshot: home\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    let png = std::fs::read(out.path().join("home.png")).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[test]
fn assert_screenshot_compares_with_a_reference_image() {
    let flows = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::fs::write(flows.path().join("same.png"), virtuoso::image_util::solid_png(4, 8, [200, 200, 200])).unwrap();
    std::fs::write(flows.path().join("other.png"), virtuoso::image_util::solid_png(4, 8, [10, 10, 10])).unwrap();
    let r = run_with_output(&mut app(), flows.path(), out.path(), "appId: a\n---\n- assertScreenshot: same.png\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    let r = run_with_output(&mut app(), flows.path(), out.path(), "appId: a\n---\n- assertScreenshot: other.png\n");
    assert!(r.error.unwrap().contains('%'));
}

#[test]
fn assert_screenshot_without_a_reference_saves_one() {
    let flows = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let r = run_with_output(&mut app(), flows.path(), out.path(), "appId: a\n---\n- assertScreenshot: new.png\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert!(flows.path().join("new.png").exists());
}

#[test]
fn recordings_start_and_stop() {
    let flows = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let mut d = app();
    let r = run_with_output(&mut d, flows.path(), out.path(), "appId: a\n---\n- startRecording: demo\n- stopRecording\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["record demo.mp4", "stop recording"]);
}

#[test]
fn hide_keyboard_does_not_settle_again_once_the_keyboard_is_gone() {
    let mut d = app();
    d.keyboard = true;
    passes(&mut d, "- hideKeyboard\n");
    assert_eq!(d.log, vec!["hide keyboard"]);
    assert_eq!(d.settles, 0, "the device already waited for the keyboard to go");
}

#[test]
fn scroll_until_visible_drags_over_half_the_screen_per_step() {
    let mut d = app();
    let _ = run(&mut d, "appId: a\n---\n- scrollUntilVisible:\n    element: Nowhere\n    timeout: 50\n");
    let (from, to) = d.log[0].strip_prefix("swipe ").unwrap().split_once(" -> ").unwrap();
    let y = |p: &str| p.split(',').nth(1).unwrap().split(' ').next().unwrap().parse::<i32>().unwrap();
    let dragged = y(from) - y(to);
    assert!(dragged * 100 >= 2400 * 55, "dragged {dragged} of 2400: {}", d.log[0]);
    assert!(y(to) >= 2400 * 20 / 100, "stays clear of the status bar: {}", d.log[0]);
}

#[test]
fn scrolling_up_and_down_ends_each_drag_with_a_cancel_so_no_list_flings() {
    // A React Native list flung now and then on a lift, however still the finger had been.
    let mut d = app();
    let _ = run(&mut d, "appId: a
---
- scrollUntilVisible:
    element: Nowhere
    timeout: 50
");
    let _ = run(&mut d, "appId: a
---
- scrollUntilVisible:
    element: Nowhere
    direction: UP
    timeout: 50
");
    let swipes: Vec<&String> = d.log.iter().filter(|l| l.starts_with("swipe")).collect();
    assert!(swipes.len() >= 2 && swipes.iter().all(|l| l.ends_with("50ms cancel")), "{swipes:?}");
}

#[test]
fn commands_without_their_own_wording_are_described_by_name() {
    let flow = parse_flow("appId: a\n---\n- back\n- hideKeyboard\n- waitForAnimationToEnd\n- dismissAlert\n- scrollUntilVisible:\n    element: X\n", None).unwrap();
    let names: Vec<String> = flow.commands.iter().map(|s| virtuoso::runner::describe(&s.cmd)).collect();
    assert_eq!(names, ["Back", "HideKeyboard", "WaitForAnimationToEnd", "Alert", "ScrollUntilVisible"]);
}

#[test]
fn a_png_written_by_virtuoso_reads_back_pixel_for_pixel() {
    use virtuoso::image_util::{crop, decode, encode, Rgba};
    // 300x300 RGBA is 360 KB of pixels: several deflate blocks.
    let pixels: Vec<u8> = (0..300 * 300).flat_map(|i: u32| [(i % 251) as u8, (i / 300) as u8, (i * 7 % 256) as u8, 255]).collect();
    let img = Rgba { width: 300, height: 300, pixels };
    assert_eq!(decode(&encode(&img)).unwrap(), img);
    let part = crop(&img, 10, 20, 110, 70);
    assert_eq!(decode(&encode(&part)).unwrap(), part);
}

/// A `w`x`h` PNG of `color` at `depth` bits, its samples from `f(x, y, channel)`.
fn png_of(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, f: impl Fn(u32, u32, usize) -> u8) -> Vec<u8> {
    let channels = color.samples();
    let bytes = if depth == png::BitDepth::Sixteen { 2 } else { 1 };
    let data: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| (0..channels).map(move |c| (x, y, c)))).flat_map(|(x, y, c)| vec![f(x, y, c); bytes]).collect();
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(color);
    e.set_depth(depth);
    if color == png::ColorType::Indexed {
        e.set_palette((0..=255u8).flat_map(|i| [i, 255 - i, i / 2]).collect::<Vec<u8>>());
    }
    e.write_header().unwrap().write_image_data(&data).unwrap();
    out
}

#[test]
fn screenshots_compare_and_crop_row_by_row_as_they_did_whole() {
    // Decoding a screenshot whole is ~10 MB of pixels (two for assertScreenshot): the largest
    // memory any flow used. Read a row at a time, the answers are the same.
    use png::{BitDepth, ColorType};
    use virtuoso::image_util::{crop, crop_png, decode, similarity, similarity_png};
    let (w, h) = (37, 23);
    let images = [
        png_of(w, h, ColorType::Rgba, BitDepth::Eight, |x, y, c| (x * 7 + y * 3 + c as u32 * 40) as u8),
        png_of(w, h, ColorType::Rgba, BitDepth::Eight, |x, y, c| (x * 7 + y * 3 + c as u32 * 40 + (x % 3) * 9) as u8),
        png_of(w, h, ColorType::GrayscaleAlpha, BitDepth::Eight, |x, y, c| if c == 1 { 200 } else { (x * 5 + y) as u8 }),
        png_of(w, h, ColorType::Grayscale, BitDepth::Eight, |x, y, _| (x * 5 + y) as u8),
        png_of(w, h, ColorType::Rgb, BitDepth::Eight, |x, y, c| (x * 7 + y * 3 + c as u32 * 40) as u8),
        png_of(w, h, ColorType::Rgba, BitDepth::Sixteen, |x, y, c| (x * 7 + y * 3 + c as u32 * 40) as u8),
        png_of(w, h, ColorType::Indexed, BitDepth::Eight, |x, y, _| (x * 6 + y) as u8),
        png_of(w + 1, h, ColorType::Rgba, BitDepth::Eight, |_, _, _| 9),
        virtuoso::image_util::solid_png(w, h, [9, 9, 9]),
    ];
    for (i, a) in images.iter().enumerate() {
        for (j, b) in images.iter().enumerate() {
            let whole = similarity(&decode(a).unwrap(), &decode(b).unwrap());
            assert_eq!(similarity_png(a, b).unwrap(), whole, "images {i} and {j}");
        }
        for (l, t, r, b) in [(3, 4, 20, 15), (-5, -5, 100, 100), (10, 10, 10, 20), (30, 20, 5, 2), (0, 22, 37, 23)] {
            assert_eq!(crop_png(a, l, t, r, b).unwrap(), crop(&decode(a).unwrap(), l, t, r, b), "image {i} cropped to {l},{t},{r},{b}");
        }
    }
}

#[test]
fn a_when_true_condition_on_the_flows_env_is_false_when_it_does_not_hold() {
    let mut d = app();
    let r = run(&mut d, "appId: a\nenv:\n  GREETING: Hello\n---\n- runFlow:\n    when:\n      true: ${GREETING == 'Hello'}\n    commands:\n      - inputText: yes\n- runFlow:\n    when:\n      true: ${GREETING == 'Bye'}\n    commands:\n      - inputText: no\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["text yes"]);
}


fn with_condition_timeout(ms: u64) -> RunOptions {
    RunOptions { condition_timeout: std::time::Duration::from_millis(ms), ..opts() }
}

#[test]
fn checks_for_popups_that_never_show_share_one_wait_from_the_last_change() {
    // As in speedrun: a `when: visible:` waits until the condition timeout has passed since
    // the last command that may have changed the app, not the whole timeout each.
    let mut d = app();
    let flow = parse_flow(
        "appId: a\n---\n- evalScript: ${1}\n- runFlow:\n    when:\n      visible: Cookies\n    commands:\n      - back\n- runFlow:\n    when:\n      visible: Promo\n    commands:\n      - back\n- runFlow:\n    when:\n      visible: Survey\n    commands:\n      - back\n",
        None,
    )
    .unwrap();
    let start = std::time::Instant::now();
    let r = Runner::new(&mut d, with_condition_timeout(400)).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert!(start.elapsed() < std::time::Duration::from_millis(800), "three checks took {:?}", start.elapsed());
}

#[test]
fn a_check_after_a_tap_waits_the_whole_condition_timeout_again() {
    let mut d = app();
    let flow = parse_flow(
        "appId: a\n---\n- runFlow:\n    when:\n      visible: Cookies\n    commands:\n      - back\n- tapOn: Sign in\n- runFlow:\n    when:\n      visible: Promo\n    commands:\n      - back\n",
        None,
    )
    .unwrap();
    let start = std::time::Instant::now();
    let r = Runner::new(&mut d, with_condition_timeout(300)).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert!(start.elapsed() >= std::time::Duration::from_millis(550), "only {:?}", start.elapsed());
}

#[test]
fn a_tap_under_a_loading_overlay_waits_for_it_to_go() {
    let key = || node("1", [60, 650, 360, 780]).goes_to("next digit");
    let mut d = FakeDevice::new();
    let mut spinner = node("", [500, 1100, 580, 1180]).depth(2);
    spinner.el.class = "android.widget.ProgressBar".into();
    d.add_screen("loading", FakeScreen::new(vec![key(), node("", [0, 136, 1080, 2400]).covers(), spinner]));
    d.add_screen("ready", FakeScreen::new(vec![key()]));
    d.add_screen("next digit", FakeScreen::new(vec![node("Confirm your PIN", [0, 300, 1080, 400])]));
    d.show("loading");
    d.show_after_reads("ready", 4);
    passes(&mut d, "- tapOn: \"1\"\n- assertVisible: Confirm your PIN\n");
}

#[test]
fn a_tap_waits_for_its_element_to_stop_moving() {
    // A banner growing in above it pushes the button down: the tap goes where it comes to rest.
    let mut d = FakeDevice::new();
    for (name, top) in [("shifting", 1000), ("pushed", 1200)] {
        d.add_screen(name, FakeScreen::new(vec![node("Like", [80, top, 220, top + 50])]));
    }
    d.show("shifting");
    d.show_after_reads("pushed", 2);
    passes(&mut d, "- tapOn: Like\n");
    assert_eq!(d.log, vec!["tap 150,1225"], "tapped where it rests, not where it was first seen");
}

#[test]
fn a_tap_checks_where_its_element_is_with_one_fresh_read() {
    // A cached read can hold bounds a layout change made stale without an event (a Reanimated
    // height animation): the check before the tap reads past the cache, once on a still screen.
    let mut d = app();
    passes(&mut d, "- tapOn: Sign in\n");
    assert_eq!(d.log, vec!["tap 200,130"]);
    assert_eq!(d.fresh_reads, 1);
}

#[test]
fn scrolling_to_an_element_not_yet_on_the_screen_keeps_to_cached_reads() {
    // A read past the cache costs a whole tree from the app: a scroll down a long list to a
    // row reads it only when the row shows in a state the selector does not want.
    let mut d = FakeDevice::new();
    d.add_screen("top", FakeScreen::new(vec![node("Row 1", [0, 400, 1080, 525])]).scrolls_to("middle"));
    d.add_screen("middle", FakeScreen::new(vec![node("Row 60", [0, 400, 1080, 525])]).scrolls_to("end"));
    d.add_screen("end", FakeScreen::new(vec![node("Row 120", [0, 400, 1080, 525])]));
    d.show("top");
    passes(&mut d, "- scrollUntilVisible:\n    element: Row 120\n");
    assert_eq!(d.fresh_reads, 0);
}

#[test]
fn a_tap_drags_a_row_out_from_under_a_sticky_header_first() {
    // Scrolled up to it, the row sits under its section's sticky header, which would take the
    // tap: as a user would, the list is dragged down until the row shows, and the row tapped.
    let mut d = FakeDevice::new();
    let list = || node("", [0, 387, 1080, 2400]).flags(virtuoso::screen::VISIBLE | virtuoso::screen::ENABLED | virtuoso::screen::SCROLLABLE);
    let header = || node("Section A", [42, 413, 1038, 464]).depth(2).covers();
    d.add_screen("under", FakeScreen::new(vec![list(), node("Row 3", [0, 387, 1080, 455]).depth(2).goes_to("picked"), header()]).scrolls_back("clear"));
    d.add_screen("clear", FakeScreen::new(vec![list(), header(), node("Row 3", [0, 700, 1080, 825]).depth(2).goes_to("picked")]));
    d.add_screen("picked", FakeScreen::new(vec![node("Picked row 3", [0, 272, 1080, 386])]));
    d.show("under");
    passes(&mut d, "- tapOn: Row 3\n- assertVisible: Picked row 3\n");
    assert!(d.log.first().is_some_and(|l| l.starts_with("swipe")), "dragged clear first: {:?}", d.log);
}

#[test]
fn a_rows_own_views_listed_after_its_label_do_not_count_as_a_bar_over_it() {
    // iOS 26's Settings lists a cell's backgrounds after its label, at the cell's bounds: they
    // are the row itself, not a header over it, and nothing is dragged.
    let mut d = FakeDevice::new();
    let list = || node("", [0, 102, 414, 862]).flags(virtuoso::screen::VISIBLE | virtuoso::screen::ENABLED | virtuoso::screen::SCROLLABLE);
    d.add_screen(
        "list",
        FakeScreen::new(vec![
            list(),
            node("", [20, 451, 394, 504]).depth(2),
            node("Display & Text Size", [38, 464, 224, 492]).depth(3).goes_to("display"),
            node("", [20, 451, 394, 504]).depth(3),
            node("", [20, 451, 394, 504]).depth(3),
        ]),
    );
    d.add_screen("display", FakeScreen::new(vec![node("Bold Text", [0, 300, 414, 350])]));
    d.show("list");
    passes(&mut d, "- tapOn: Display & Text Size
- assertVisible: Bold Text
");
    assert!(!d.log.iter().any(|l| l.starts_with("swipe")), "no drag: {:?}", d.log);
}

#[test]
fn a_view_behind_a_row_does_not_count_as_a_bar_over_it() {
    // iOS lists a section's background before its cells: it overlaps the row, but it is drawn
    // behind it, so the tap lands on the row and nothing is dragged.
    let mut d = FakeDevice::new();
    let list = || node("", [0, 100, 1080, 2400]).flags(virtuoso::screen::VISIBLE | virtuoso::screen::ENABLED | virtuoso::screen::SCROLLABLE);
    let background = || node("", [40, 280, 1040, 900]).depth(2);
    d.add_screen("list", FakeScreen::new(vec![list(), background(), node("General", [40, 300, 1040, 400]).depth(2).goes_to("general")]));
    d.add_screen("general", FakeScreen::new(vec![node("About", [0, 300, 1080, 400])]));
    d.show("list");
    passes(&mut d, "- tapOn: General
- assertVisible: About
");
    assert!(!d.log.iter().any(|l| l.starts_with("swipe")), "no drag: {:?}", d.log);
}

#[test]
fn scrolling_until_an_element_is_in_a_state_reads_past_a_stale_cache_before_scrolling_on() {
    // A double tap likes the post, but the cached read still shows its button unselected (the
    // state changed without an event): scrolling on would carry the button off the screen.
    let mut d = FakeDevice::new();
    let liked = |selected: bool| {
        let n = node("Like", [80, 1800, 220, 1850]).id("like-2");
        if selected { n.flags(virtuoso::screen::VISIBLE | virtuoso::screen::ENABLED | virtuoso::screen::SELECTED) } else { n }
    };
    d.add_screen("unliked", FakeScreen::new(vec![liked(false)]));
    d.add_screen("liked", FakeScreen::new(vec![liked(true)]));
    d.show("liked");
    d.cache_stale("unliked");
    passes(&mut d, "- scrollUntilVisible:\n    element:\n      id: like-2\n      selected: true\n");
    assert!(d.log.is_empty(), "no scroll past the liked button: {:?}", d.log);
}

#[test]
fn scrolling_sideways_drags_across_the_list_that_scrolls_sideways() {
    let mut d = FakeDevice::new();
    let mut cards = node("", [0, 300, 1080, 600]).flags(virtuoso::screen::VISIBLE | virtuoso::screen::ENABLED | virtuoso::screen::SCROLLABLE);
    cards.el.class = "android.widget.HorizontalScrollView".into();
    d.add_screen("cards", FakeScreen::new(vec![cards, node("Card 1", [0, 300, 500, 600]).depth(2), node("Card 2", [520, 300, 1020, 600]).depth(2)]));
    d.show("cards");
    let r = run(&mut d, "appId: a\n---\n- scrollUntilVisible:\n    element: Card 25\n    direction: RIGHT\n    timeout: 1\n");
    assert_eq!(r.status, FlowStatus::Failed);
    let swipe = d.log.iter().find(|l| l.starts_with("swipe")).expect("a swipe");
    assert!(swipe.contains(",450 -> ") && swipe.ends_with(",450 50ms"), "{swipe}");
}

#[test]
fn a_swipe_or_scroll_takes_the_screen_size_without_reading_the_screen() {
    // A whole tree costs ~130 ms on an iOS simulator: where a swipe goes needs only the size.
    let mut d = app();
    passes(&mut d, "- swipe:\n    direction: UP\n- scroll\n- swipe:\n    start: 50%, 80%\n    end: 50%, 20%\n- tapOn:\n    point: 50%, 50%\n");
    assert_eq!(d.reads + d.fresh_reads, 0, "{:?}", d.log);
    assert_eq!(d.log[0], "swipe 540,1200 -> 540,240 400ms fling");
}

#[test]
fn waiting_for_an_alert_reads_afresh_after_the_first_look() {
    // A device keeps its last read for the next command's first look (iOS): a wait polls past it.
    let mut d = FakeDevice::new();
    d.add_screen("plain", FakeScreen::new(vec![node("Hello", [100, 100, 300, 160])]));
    d.add_screen("alert", FakeScreen::new(vec![node("Allow", [100, 500, 300, 560])]));
    d.show("alert");
    d.cache_stale("plain");
    passes(&mut d, "- acceptAlert\n");
    assert_eq!(d.log, vec!["tap 200,530"]);
}

#[test]
fn erasing_on_ios_sends_no_more_deletes_than_the_focused_field_holds() {
    // iOS types each delete as a key event, 10 ms apart: 50 for a 5-letter field is 450 ms lost.
    use virtuoso::screen::{EDITABLE, ENABLED, FOCUSED, PASSWORD, VISIBLE};
    let field = |text: &str, extra: u32| node(text, [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED | extra);
    let mut d = FakeDevice::new();
    d.platform = "ios";
    d.add_screen("form", FakeScreen::new(vec![field("hello", 0)]));
    d.add_screen("secret", FakeScreen::new(vec![field("•••", PASSWORD)]));
    d.add_screen("empty", FakeScreen::new(vec![field("", 0)]));
    d.show("form");
    passes(&mut d, "- eraseText\n- eraseText: 3\n");
    assert_eq!(d.log, vec!["erase 5", "erase 3"]);
    // A secure field hides its length, and an empty one may only not show its value: as asked.
    for s in ["secret", "empty"] {
        d.log.clear();
        d.show(s);
        passes(&mut d, "- eraseText\n");
        assert_eq!(d.log, vec!["erase 50"], "{s}");
    }
    // Android erases in one call: as asked.
    let mut a = FakeDevice::new();
    a.add_screen("form", FakeScreen::new(vec![field("hello", 0)]));
    a.show("form");
    passes(&mut a, "- eraseText\n");
    assert_eq!(a.log, vec!["erase 50"]);
}

#[test]
fn a_tap_on_a_still_screen_checks_and_taps_in_one_request() {
    // On iOS, the check that the element has not moved and the tap are one agent request.
    let mut d = app();
    passes(&mut d, "- tapOn: Sign in\n");
    assert_eq!(d.log, vec!["tap 200,130"]);
    assert_eq!(d.taps_if_unchanged, 1);
}

#[test]
fn a_tap_looks_for_its_element_on_a_screen_to_act_on() {
    // Not a read a check just made (a modal fading in can be in it, and takes no touch yet).
    let mut d = app();
    passes(&mut d, "- assertVisible: Sign in\n- tapOn: Sign in\n");
    assert_eq!(d.looks_to_act_on, 1);
}

/// The reads one command makes: (screen reads, of them fresh ones, looks to act on, screen
/// size asks). Each is an agent request on a device.
fn reads_of(d: &mut FakeDevice, cmds: &str) -> (usize, usize, usize, usize) {
    (d.reads, d.fresh_reads, d.looks_to_act_on, d.rect_reads) = (0, 0, 0, 0);
    passes(d, cmds);
    (d.reads, d.fresh_reads, d.looks_to_act_on, d.rect_reads)
}

#[test]
fn each_command_makes_the_reads_it_makes_today() {
    // A benchmark guard: a read brought back into a command fails no flow, it only makes the
    // benchmark slower. These are the counts as they stand; a change that removes a read
    // lowers its number here, and one that adds a read has to say why.
    use virtuoso::screen::{CLICKABLE, EDITABLE, ENABLED, FOCUSED, KEYBOARD, VISIBLE};
    let mut d = app();
    let check = |d: &mut FakeDevice, cmds: &str, want: (usize, usize, usize, usize)| {
        assert_eq!(reads_of(d, cmds), want, "(reads, fresh, to act on, size) for {cmds:?}");
    };
    check(&mut d, "- assertVisible: Sign in\n", (1, 0, 0, 0));
    // The lookup, then the fresh read the tap goes with (tap_if_unchanged: one request).
    check(&mut d, "- tapOn: Sign in\n", (2, 1, 1, 0));
    check(&mut d, "- assertNotVisible: Sign in\n", (1, 0, 0, 0));
    check(&mut d, "- back\n", (0, 0, 0, 0));
    // A swipe reads no screen, only its size.
    check(&mut d, "- swipe:\n    direction: LEFT\n", (0, 0, 0, 1));
    check(&mut d, "- scroll\n", (0, 0, 0, 1));
    check(&mut d, "- inputText: hello\n", (0, 0, 0, 0));
    check(&mut d, "- tapOn:\n    point: 50%,50%\n", (0, 0, 0, 1));

    // A field with focus and its keyboard up.
    let key = |label: &str, b| node(label, b).flags(VISIBLE | ENABLED | CLICKABLE | KEYBOARD);
    d.add_screen(
        "typing",
        FakeScreen::new(vec![
            node("hello", [0, 100, 1080, 200]).flags(VISIBLE | ENABLED | EDITABLE | FOCUSED),
            key("q", [0, 1963, 100, 2118]),
        ]),
    );
    d.show("typing");
    // hideKeyboard looks afresh once, for a keyboard still coming up.
    check(&mut d, "- inputText: hello\n- hideKeyboard\n", (1, 1, 0, 0));
    d.show("typing");
    // The keyboard shown has no Delete key: the cached look (Android's, a miss here), one
    // fresh look, then the key event.
    check(&mut d, "- pressKey: Backspace\n", (2, 1, 1, 0));
    check(&mut d, "- pressKey: Back\n", (0, 0, 0, 0));
}
