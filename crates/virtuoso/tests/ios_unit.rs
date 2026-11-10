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
    assert!(c.get(t0).is_none());
    c.put(tree.clone(), t0);
    assert!(c.get(t0 + Duration::from_millis(900)).is_some());
    assert!(c.get(t0 + Duration::from_millis(900)).is_some(), "every command's first look until then: nothing has acted");
    assert!(c.get(t0 + Duration::from_millis(1100)).is_none(), "a second old: read again");
    c.put(tree, t0);
    c.clear();
    assert!(c.get(t0).is_none(), "an action since: read again");
}

#[test]
fn a_plain_read_stands_for_the_next_look_briefly() {
    // Read-only commands in a row (assertVisible after assertVisible) share one read, as long
    // as nothing acted since and it is fresh: 300 ms, a settle's tree holds still for a second.
    use std::time::{Duration, Instant};
    use virtuoso::ios::SettledScreen;
    let t0 = Instant::now();
    let mut c = SettledScreen::default();
    c.put_read(virtuoso::screen::ScreenTree::default(), t0);
    assert!(c.get(t0 + Duration::from_millis(250)).is_some());
    assert!(c.get(t0 + Duration::from_millis(350)).is_none());
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

#[test]
fn a_settle_reply_is_read_in_one_pass_with_its_outcome_and_screen() {
    use virtuoso::screen::ScreenReply;
    let body = br#"{"ok":true,"changed":true,"timedOut":false,"idleMs":120,"snaps":2,"snapMs":90,
        "windows":[{"pkg":"com.app","type":1}],"nodes":[{"d":0,"w":0,"f":34,"t":"Hi","k":"StaticText","b":[0,0,10,10]}]}"#;
    let r = ScreenReply::parse(body, 390, 844).unwrap();
    assert!(r.changed && !r.timed_out);
    assert_eq!(r.tree.elements[0].text, "Hi");
    assert!(r.has_screen);
    let r = ScreenReply::parse(br#"{"ok":true,"changed":false,"timedOut":true}"#, 390, 844).unwrap();
    assert!(r.timed_out && !r.has_screen);
}

#[test]
fn simctl_is_found_in_the_developer_folder_without_xcrun() {
    // `xcrun simctl` is a process more and an SDK lookup on every call: simctl is run directly
    // from the developer folder (DEVELOPER_DIR, else xcode-select's), when it is there.
    use virtuoso::ios::simctl_in;
    let dev = std::env::temp_dir().join(format!("virtuoso-devdir-{}", std::process::id()));
    assert_eq!(simctl_in(&dev), None);
    std::fs::create_dir_all(dev.join("usr/bin")).unwrap();
    std::fs::write(dev.join("usr/bin/simctl"), b"").unwrap();
    assert_eq!(simctl_in(&dev), Some(dev.join("usr/bin/simctl")));
    std::fs::remove_dir_all(&dev).unwrap();
}

#[test]
fn an_apps_data_folder_is_remembered_while_it_is_there() {
    use virtuoso::ios::{remember_container, remembered_container};
    let udid = format!("test-udid-{}", std::process::id());
    let data = std::env::temp_dir().join(format!("virtuoso-container-{}", std::process::id()));
    assert_eq!(remembered_container(&udid, "com.app"), None);
    std::fs::create_dir_all(&data).unwrap();
    remember_container(&udid, "com.app", &data);
    assert_eq!(remembered_container(&udid, "com.app"), Some(data.clone()));
    assert_eq!(remembered_container(&udid, "com.other"), None);
    // Uninstalled since: asked for again.
    std::fs::remove_dir_all(&data).unwrap();
    assert_eq!(remembered_container(&udid, "com.app"), None);
}

#[test]
fn a_simulator_whose_agent_answers_is_booted_without_asking_simctl() {
    // `simctl list` takes a process and a CoreSimulator round trip on every run: a simulator
    // with a live agent (the state file the last run left) is booted.
    use std::io::{Read, Write};
    use virtuoso::ios::agent_answers;
    let udid = format!("test-agent-{}", std::process::id());
    assert!(!agent_answers(&udid), "no agent recorded");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut c, _) = listener.accept().unwrap();
        let mut frame = || {
            let mut len = [0u8; 4];
            c.read_exact(&mut len).ok()?;
            let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
            c.read_exact(&mut body).ok()?;
            Some(())
        };
        frame(); // the token
        frame(); // ping
        let reply = br#"{"ok":true}"#;
        c.write_all(&(reply.len() as u32).to_be_bytes()).unwrap();
        c.write_all(reply).unwrap();
    });
    let state = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"));
    std::fs::write(&state, format!(r#"{{"port":{port},"token":"t","build":"b"}}"#)).unwrap();
    assert!(agent_answers(&udid));
    std::fs::remove_file(&state).unwrap();
}

#[test]
fn the_agent_is_built_optimised() {
    // Its screen walk, digest and JSON run for every read and settle step: unoptimised Swift
    // is many times slower at string and dictionary work.
    let (_, scheme) = virtuoso::ios::agent_sources().iter().find(|(p, _)| p.ends_with("VirtuosoAgent.xcscheme")).expect("the agent's scheme");
    let scheme = String::from_utf8_lossy(scheme);
    let test = &scheme[scheme.find("<TestAction").unwrap()..];
    assert!(test[..test.find('>').unwrap()].contains(r#"buildConfiguration = "Release""#), "the TestAction builds Debug");
    let (_, spec) = virtuoso::ios::agent_sources().iter().find(|(p, _)| p.ends_with("project.yml")).expect("project.yml");
    assert!(String::from_utf8_lossy(spec).contains("config: Release"), "project.yml's test action");
}

#[test]
fn an_action_looks_only_at_a_screen_that_held_still() {
    // A plain read can catch an element just appearing (a modal fading in takes no touch yet):
    // a tap's first look takes a settle's screen, never a plain read's.
    use std::time::Instant;
    use virtuoso::ios::SettledScreen;
    let t0 = Instant::now();
    let mut c = SettledScreen::default();
    c.put_read(virtuoso::screen::ScreenTree::default(), t0);
    assert!(c.get(t0).is_some() && c.get_settled(t0).is_none());
    c.put(virtuoso::screen::ScreenTree::default(), t0);
    assert!(c.get_settled(t0).is_some());
}
