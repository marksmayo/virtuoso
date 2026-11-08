//! iOS helpers that need no simulator.

use virtuoso::ios::booted_simulators;

#[test]
fn booted_simulators_are_read_from_simctl_json() {
    let json = r#"{"devices": {
        "com.apple.CoreSimulator.SimRuntime.iOS-18-2": [
            {"udid": "AAAA", "name": "iPhone 16", "state": "Shutdown", "isAvailable": true},
            {"udid": "BBBB", "name": "iPhone 16 Pro", "state": "Booted", "isAvailable": true}
        ],
        "com.apple.CoreSimulator.SimRuntime.watchOS-11-2": [
            {"udid": "CCCC", "name": "Apple Watch", "state": "Booted", "isAvailable": true}
        ]
    }}"#;
    assert_eq!(booted_simulators(json).unwrap(), vec!["BBBB"]);
}

use virtuoso::device::Key;
use virtuoso::ios::ios_key;

#[test]
fn keys_map_to_what_the_ios_agent_presses() {
    assert_eq!(ios_key(Key::Home), Some("home"));
    assert_eq!(ios_key(Key::Enter), Some("enter"));
    assert_eq!(ios_key(Key::Backspace), Some("backspace"));
    assert_eq!(ios_key(Key::Lock), Some("lock"));
    // iOS has no Back key, and the simulator no volume buttons.
    assert_eq!(ios_key(Key::Back), None);
    assert_eq!(ios_key(Key::VolumeUp), None);
}

use virtuoso::ios::privacy;

#[test]
fn maestro_permissions_map_to_simctl_privacy() {
    assert_eq!(privacy("location", "allow"), Some(("grant", "location")));
    assert_eq!(privacy("photos", "deny"), Some(("revoke", "photos")));
    assert_eq!(privacy("contacts", "unset"), Some(("reset", "contacts")));
    assert_eq!(privacy("all", "deny"), Some(("revoke", "all")));
    assert_eq!(privacy("medialibrary", "allow"), Some(("grant", "media-library")));
    // simctl cannot set these: the camera and notifications ask the user.
    assert_eq!(privacy("camera", "allow"), None);
    assert_eq!(privacy("notifications", "allow"), None);
    assert_eq!(privacy("location", "sometimes"), None);
}
