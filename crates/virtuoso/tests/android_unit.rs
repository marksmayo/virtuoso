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
