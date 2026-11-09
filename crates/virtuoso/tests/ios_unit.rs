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

#[test]
fn an_app_is_launched_with_every_permission_unless_the_flow_says_otherwise() {
    use virtuoso::ios::launch_permissions;
    let all = vec![("all".to_string(), "allow".to_string())];
    assert_eq!(launch_permissions(&[]), all);
    let camera = vec![("camera".to_string(), "deny".to_string())];
    assert_eq!(launch_permissions(&camera), camera);
}

use virtuoso::ios::open_prompt_button;
use virtuoso::screen::{Element, Rect, ScreenTree, CLICKABLE, ENABLED, VISIBLE};

fn node(depth: u16, text: &str, class: &str, pkg: &str, b: [i32; 4]) -> Element {
    Element {
        depth,
        text: text.into(),
        class: class.into(),
        package: pkg.into(),
        bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
        flags: VISIBLE | ENABLED | if class == "Button" { CLICKABLE } else { 0 },
        ..Default::default()
    }
}

#[test]
fn the_open_in_app_prompt_is_answered_with_open() {
    let sb = "com.apple.springboard";
    let prompt = ScreenTree::new(
        vec![
            node(0, "", "Alert", sb, [40, 300, 350, 500]),
            node(1, "Open in “Fixture”?", "StaticText", sb, [60, 320, 330, 350]),
            node(1, "Cancel", "Button", sb, [40, 450, 195, 500]),
            node(1, "Open", "Button", sb, [195, 450, 350, 500]),
        ],
        390,
        844,
    );
    assert_eq!(open_prompt_button(&prompt), Some((272, 475)));
    // An app's own Open button is not the system's prompt.
    let app = ScreenTree::new(vec![node(0, "Open", "Button", "dev.app", [0, 0, 100, 50])], 390, 844);
    assert_eq!(open_prompt_button(&app), None);
}

use virtuoso::ios::{agent_sources, simulator_xctestrun};

#[test]
fn the_simulator_build_of_the_agent_is_picked_from_the_products() {
    let names = ["Debug-iphonesimulator", "VirtuosoAgent_iphoneos18.0-arm64.xctestrun", "VirtuosoAgent_iphonesimulator26.2-arm64-x86_64.xctestrun"];
    assert_eq!(simulator_xctestrun(&names), Some("VirtuosoAgent_iphonesimulator26.2-arm64-x86_64.xctestrun"));
    assert_eq!(simulator_xctestrun(&["Debug-iphonesimulator"]), None);
}

#[test]
fn the_binary_carries_the_agent_sources() {
    let files: Vec<&str> = agent_sources().iter().map(|(p, _)| *p).collect();
    for want in ["Agent/Agent.swift", "Agent/Handler.swift", "Host/HostApp.swift", "VirtuosoAgent.xcodeproj/project.pbxproj"] {
        assert!(files.contains(&want), "missing {want}: {files:?}");
    }
    assert!(!files.iter().any(|f| f.starts_with("Fixture/")), "the fixture app is the tests', not the agent's");
}
