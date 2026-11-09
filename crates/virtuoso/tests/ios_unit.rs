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

#[test]
fn what_simctl_cannot_set_goes_into_the_simulators_tcc_database() {
    use virtuoso::ios::tcc_sql;
    let p = |k: &str, v: &str| (k.to_string(), v.to_string());
    let sql = tcc_sql("dev.app", &[p("camera", "allow")]).unwrap();
    assert!(sql.contains("DELETE FROM access WHERE service = 'kTCCServiceCamera' AND client = 'dev.app' AND client_type = 0;"), "{sql}");
    assert!(sql.contains("VALUES ('kTCCServiceCamera', 'dev.app', 0, 2, 2, 1, 0);"), "{sql}");
    let sql = tcc_sql("dev.app", &[p("camera", "deny")]).unwrap();
    assert!(sql.contains("VALUES ('kTCCServiceCamera', 'dev.app', 0, 0, 2, 1, 0);"), "{sql}");
    // Unset: the row goes, nothing comes in its place.
    let sql = tcc_sql("dev.app", &[p("camera", "unset")]).unwrap();
    assert!(sql.contains("DELETE") && !sql.contains("INSERT"), "{sql}");
    // `all` reaches the camera too; what simctl sets is left to it.
    assert!(tcc_sql("dev.app", &[p("all", "allow")]).unwrap().contains("kTCCServiceCamera"));
    assert_eq!(tcc_sql("dev.app", &[p("location", "allow"), p("photos", "deny")]), None);
}

#[test]
fn the_screen_a_settle_ended_on_stands_until_an_action_or_a_second() {
    use std::time::{Duration, Instant};
    use virtuoso::ios::SettledScreen;
    let tree = virtuoso::screen::ScreenTree::default();
    let t0 = Instant::now();
    let mut c = SettledScreen::default();
    assert!(c.take(t0).is_none());
    c.put(tree.clone(), t0);
    assert!(c.take(t0 + Duration::from_millis(900)).is_some());
    assert!(c.take(t0 + Duration::from_millis(900)).is_none(), "the first read only: a wait reads afresh");
    c.put(tree.clone(), t0);
    assert!(c.take(t0 + Duration::from_millis(1100)).is_none(), "a second old: read again");
    c.put(tree, t0);
    c.clear();
    assert!(c.take(t0).is_none(), "an action since: read again");
}

#[test]
fn simctl_launch_says_which_process_it_started() {
    use virtuoso::ios::launched_pid;
    assert_eq!(launched_pid("com.apple.Preferences: 17604\n"), Some(17604));
    assert_eq!(launched_pid("dev.speedrun.bench: 412"), Some(412));
    assert_eq!(launched_pid("something else"), None);
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
