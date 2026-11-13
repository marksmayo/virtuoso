//! Android helpers that need no device.

use virtuoso::android::{android_permissions, parse_devices, runtime_permissions};

#[test]
fn maestro_permission_names_map_to_android_permissions() {
    assert_eq!(android_permissions("camera"), vec!["android.permission.CAMERA"]);
    assert!(android_permissions("location").contains(&"android.permission.ACCESS_FINE_LOCATION".to_string()));
    assert_eq!(android_permissions("android.permission.READ_SMS"), vec!["android.permission.READ_SMS"]);
    assert!(android_permissions("teleport").is_empty());
}

#[test]
fn runtime_permissions_are_read_from_dumpsys() {
    let dump = "Packages:\n  Package [com.app]\n    install permissions:\n      android.permission.INTERNET: granted=true\n    User 0: ceDataInode=1\n      runtime permissions:\n        android.permission.CAMERA: granted=false, flags=[ USER_SENSITIVE ]\n        android.permission.POST_NOTIFICATIONS: granted=true\n      enabledComponents:\n";
    assert_eq!(runtime_permissions(dump), vec!["android.permission.CAMERA", "android.permission.POST_NOTIFICATIONS"]);
}

#[test]
fn adb_devices_lists_only_ready_devices() {
    let out = "* daemon started\nList of devices attached\nemulator-5554\tdevice\nR58M\tunauthorized\n10.0.0.2:5555\tdevice\n\n";
    assert_eq!(parse_devices(out), vec!["emulator-5554", "10.0.0.2:5555"]);
}

use virtuoso::android_device::error_dialog_button;
use virtuoso::screen::{Element, Rect, ScreenTree, ENABLED, VISIBLE};
use std::time::{Duration, Instant};

fn el(depth: u16, text: &str, pkg: &str, b: [i32; 4]) -> Element {
    Element {
        depth,
        text: text.into(),
        package: pkg.into(),
        bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
        flags: VISIBLE | ENABLED,
        ..Default::default()
    }
}

#[test]
fn an_isnt_responding_dialog_is_answered_with_wait() {
    let s = ScreenTree::new(
        vec![
            el(0, "", "android", [28, 979, 1052, 1493]),
            el(1, "System UI isn't responding", "android", [60, 1000, 1000, 1100]),
            el(1, "Close app", "android", [60, 1200, 1000, 1300]),
            el(1, "Wait", "android", [60, 1350, 1000, 1450]),
        ],
        1080,
        2400,
    );
    assert_eq!(error_dialog_button(&s), Some((530, 1400)));
}

#[test]
fn a_keeps_stopping_dialog_is_closed() {
    let s = ScreenTree::new(
        vec![el(0, "", "android", [0, 0, 1080, 800]), el(1, "Phone keeps stopping", "android", [0, 0, 1080, 100]), el(1, "Close app", "android", [0, 200, 1080, 300])],
        1080,
        2400,
    );
    assert_eq!(error_dialog_button(&s), Some((540, 250)));
}

#[test]
fn an_app_screen_mentioning_wait_is_left_alone() {
    let s = ScreenTree::new(vec![el(0, "Wait", "com.app", [0, 0, 100, 100]), el(0, "isn't responding", "com.app", [0, 100, 100, 200])], 1080, 2400);
    assert_eq!(error_dialog_button(&s), None);
}

#[test]
fn only_permissions_not_yet_granted_need_granting() {
    use virtuoso::android::ungranted_runtime_permissions;
    let dump = "      runtime permissions:\n        android.permission.CAMERA: granted=false, flags=[ USER_SENSITIVE ]\n        android.permission.POST_NOTIFICATIONS: granted=true\n        android.permission.READ_CONTACTS: granted=false\n      enabledComponents:\n";
    assert_eq!(ungranted_runtime_permissions(dump), vec!["android.permission.CAMERA", "android.permission.READ_CONTACTS"]);
}

#[test]
fn the_emulator_to_start_is_the_one_asked_for_or_the_first_listed() {
    use virtuoso::android::pick_avd;
    let listed = "INFO    | Storing crashdata in: /tmp\nPixel_7_API_34\nvirtuoso_api36\n";
    assert_eq!(pick_avd(listed, None).as_deref(), Some("Pixel_7_API_34"));
    assert_eq!(pick_avd(listed, Some("virtuoso_api36")).as_deref(), Some("virtuoso_api36"));
    assert_eq!(pick_avd(listed, Some("nope")), None);
    assert_eq!(pick_avd("", None), None);
}

#[test]
fn each_device_gets_its_own_stable_local_port() {
    use virtuoso::android::local_port_for;
    let a = local_port_for("emulator-5554");
    assert_eq!(a, local_port_for("emulator-5554"), "the same every run");
    assert_ne!(a, local_port_for("emulator-5556"));
    assert!((20000..30000).contains(&a), "{a}");
}

/// When Back was pressed, if it was.
type BackedAt = std::rc::Rc<std::cell::Cell<Option<Instant>>>;

/// A keyboard read that takes `read` and shows the keyboard until `closes` after Back.
fn slow_keyboard(read: Duration, closes: Duration, backed: &BackedAt) -> impl FnMut(Option<u64>, Instant) -> anyhow::Result<(bool, Option<u64>)> {
    let backed = backed.clone();
    move |_, _| {
        std::thread::sleep(read);
        Ok((backed.get().is_none_or(|at| at.elapsed() < closes), Some(1)))
    }
}

/// Back, noting when it was pressed.
fn back(backed: &BackedAt) -> impl FnMut() -> anyhow::Result<()> {
    let backed = backed.clone();
    move || {
        backed.set(Some(Instant::now()));
        Ok(())
    }
}

#[test]
fn hide_keyboard_gives_a_slow_keyboard_its_whole_limit_from_the_back_press() {
    // A software-rendered emulator: the first read takes most of the limit, and the keyboard
    // goes well after Back.
    let backed = BackedAt::default();
    let shown = slow_keyboard(Duration::from_millis(150), Duration::from_millis(250), &backed);
    virtuoso::android_device::close_keyboard(shown, back(&backed), Duration::from_millis(300)).unwrap();
    assert!(backed.get().is_some());
}

#[test]
fn hide_keyboard_fails_on_a_keyboard_that_stays() {
    let backed = BackedAt::default();
    let shown = slow_keyboard(Duration::from_millis(10), Duration::from_secs(60), &backed);
    let e = virtuoso::android_device::close_keyboard(shown, back(&backed), Duration::from_millis(100)).unwrap_err();
    assert!(e.to_string().contains("still shown"), "{e}");
}

#[test]
fn hide_keyboard_without_a_keyboard_presses_nothing() {
    let pressed = std::cell::Cell::new(false);
    let none = |_: Option<u64>, _: Instant| Ok((false, Some(1)));
    let back = || {
        pressed.set(true);
        Ok(())
    };
    virtuoso::android_device::close_keyboard(none, back, Duration::from_millis(100)).unwrap();
    assert!(!pressed.get());
}

fn launch(clear_state: bool) -> String {
    let o = virtuoso::device::LaunchOptions { clear_state, stop_app: true, ..Default::default() };
    virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &o)
}

#[test]
fn a_launch_grants_the_permissions_side_by_side_and_starts_once_all_are_done() {
    // One after another, nine grants took ~270 ms on the emulator; side by side ~110 ms.
    for script in [launch(false), launch(true)] {
        let grant = script.find(" grant com.app $p 2>/dev/null & done; wait; ").unwrap_or_else(|| panic!("{script}"));
        let start = script.find(" start -W ").unwrap();
        assert!(grant < start, "{script}");
    }
}

#[test]
fn a_clear_state_launch_lists_the_permissions_while_the_data_is_cleared() {
    // pm clear (it stops the app and wipes its data) and dumpsys took ~440 ms one after the
    // other on the emulator, ~290 ms side by side. The clear's failure still stops the launch.
    let script = launch(true);
    let clear = script.find("cmd package clear com.app >/dev/null & c=$!; ").unwrap_or_else(|| panic!("{script}"));
    let dump = script.find("dumpsys package com.app").unwrap();
    let joined = script.find("wait $c || { echo 'clearing com.app failed'; exit 1; }; ").unwrap_or_else(|| panic!("{script}"));
    let grant = script.find(" grant com.app ").unwrap();
    assert!(clear < dump && dump < joined && joined < grant, "{script}");
    // Every permission listed is granted: the clear revoked them all, whatever the list said.
    assert!(!script.contains("granted=false"), "{script}");
}

#[test]
fn a_launch_calls_the_services_without_the_am_and_pm_scripts() {
    // /system/bin/am and pm are shell scripts that run `cmd activity` / `cmd package`: a shell
    // started for each clear, stop, grant and start.
    let stop = virtuoso::device::LaunchOptions { stop_app: true, permissions: vec![("camera".into(), "allow".into())], ..Default::default() };
    let given = virtuoso::device::LaunchOptions { clear_state: true, permissions: vec![("camera".into(), "allow".into())], ..Default::default() };
    let scripts = [launch(false), launch(true), virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &stop), virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &given)];
    for script in scripts {
        let words: Vec<&str> = script.split(|c: char| c.is_whitespace() || ";&|(){}$".contains(c)).collect();
        assert!(!words.contains(&"am") && !words.contains(&"pm"), "{script}");
        assert!(script.contains("cmd activity start -W -n com.app/.Main"), "{script}");
    }
}

#[test]
fn a_launch_lists_the_permissions_with_one_sed() {
    // dumpsys | grep | sed | grep | sed: four processes only to narrow text one sed narrows.
    for script in [launch(false), launch(true)] {
        let list = &script[script.find("dumpsys package com.app").unwrap()..];
        let list = &list[..list.find(')').unwrap()];
        assert_eq!(list.matches('|').count(), 1, "{list}");
        assert!(list.contains("| sed -n "), "{list}");
    }
}

#[test]
fn a_first_launch_finds_the_apps_launcher_activity_in_the_same_script() {
    // Each run is a new process with nothing cached: the lookup was an exec of its own.
    let o = virtuoso::device::LaunchOptions { clear_state: true, ..Default::default() };
    let script = virtuoso::android_device::launch_script("com.app", None, &o);
    let resolve = script.find("set -- $(cmd package resolve-activity --brief -c android.intent.category.LAUNCHER com.app); eval a=\\${$#}; ").unwrap_or_else(|| panic!("{script}"));
    let missing = script.find("case $a in */*) ;; *) echo 'com.app is not installed, or has no launcher activity'; exit 3;; esac; echo \"launcher $a\"; ").unwrap_or_else(|| panic!("{script}"));
    let clear = script.find("cmd package clear").unwrap();
    let start = script.find("cmd activity start -W -n $a ").unwrap_or_else(|| panic!("{script}"));
    assert!(resolve < missing && missing < clear && clear < start, "{script}");
    // Known already: no lookup.
    let known = virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &o);
    assert!(!known.contains("resolve-activity") && known.contains("-n com.app/.Main "), "{known}");
}

#[test]
fn the_agent_starts_without_waiting_on_the_old_jars_cleanup() {
    // touch and find (~20 ms on the emulator) only keep other builds' jars from piling up.
    let script = virtuoso::android::agent_start_script("/data/local/tmp/virtuoso-agent-1.jar", "", "tok", true);
    let missing = script.find("[ -f /data/local/tmp/virtuoso-agent-1.jar ] || { echo missing; exit 0; }; ").unwrap_or_else(|| panic!("{script}"));
    // This build's jar is touched before find can take it for an old one.
    let housekeeping = script.find("(touch /data/local/tmp/virtuoso-agent-1.jar; find /data/local/tmp -maxdepth 1 -name 'virtuoso-agent-*.jar' -mtime +7 -delete) & ").unwrap_or_else(|| panic!("{script}"));
    let start = script.find("exec app_process").unwrap();
    assert!(missing < housekeeping && housekeeping < start, "{script}");
    assert_eq!(script.matches("find ").count(), 1, "{script}");
}

#[test]
fn after_pushing_the_agent_it_starts_without_stopping_one_again() {
    // The first script stopped any agent (a pidof, ~30-50 ms on the emulator) before it said
    // the jar was missing: nothing can have started one since.
    let first = virtuoso::android::agent_start_script("/data/local/tmp/virtuoso-agent-1.jar", "", "tok", true);
    assert!(first.contains("pidof virtuoso-agent"), "{first}");
    let second = virtuoso::android::agent_start_script("/data/local/tmp/virtuoso-agent-1.jar", "", "tok", false);
    assert!(!second.contains("pidof") && !second.contains("kill"), "{second}");
    // The first exited before its housekeeping: this one does it.
    assert!(second.contains("find /data/local/tmp") && second.contains("exec app_process"), "{second}");
}
