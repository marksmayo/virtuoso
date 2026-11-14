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
fn the_build_watches_the_agent_sources_but_not_the_fixture_or_build_output() {
    // A watched folder is scanned whole: agent/ios itself would take in the fixture app's
    // DerivedData (`-derivedDataPath build`), and every fixture build would rebuild virtuoso.
    // The folders the sources are in are watched, so a file added there is still noticed.
    // What the build script watches, as it writes it out (Cargo's own record of its output can
    // be missing from a cached build).
    let output = std::fs::read_to_string(concat!(env!("OUT_DIR"), "/ios_watched.txt")).unwrap();
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let root = canon(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent/ios"));
    let watched: Vec<std::path::PathBuf> = output.lines().map(|p| canon(std::path::Path::new(p))).filter(|p| p.starts_with(&root)).collect();
    for w in &watched {
        let rel = w.strip_prefix(&root).unwrap();
        assert!(!rel.as_os_str().is_empty(), "agent/ios is watched whole");
        assert!(!rel.starts_with("Fixture") && !rel.starts_with("build"), "{} is watched", w.display());
    }
    for (path, _) in agent_sources() {
        let dir = root.join(path).parent().unwrap().to_path_buf();
        assert!(watched.contains(&root.join(path)) || watched.contains(&dir), "{path} is not watched");
        assert!(dir == root || watched.contains(&dir), "a file added beside {path} would not be noticed");
    }
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

#[test]
fn the_agent_is_built_for_the_host_simulator_only_without_ide_extras() {
    // A generic simulator destination builds every architecture; the simulator runs the
    // Mac's. dSYMs, the index store and a per-build module cache are an IDE's, not a run's.
    use std::path::Path;
    let args = virtuoso::ios::agent_build_args(Path::new("/src"), Path::new("/b"), Path::new("/cache"), "aarch64");
    let has = |a: &str| args.iter().any(|x| x == a);
    assert!(has("build-for-testing") && has("CODE_SIGNING_ALLOWED=NO"), "{args:?}");
    assert!(has("ARCHS=arm64"), "{args:?}");
    assert!(has("ONLY_ACTIVE_ARCH=YES"), "{args:?}");
    assert!(has("DEBUG_INFORMATION_FORMAT=dwarf"), "{args:?}");
    // No debug info at all: nothing reads it (no debugger, no dSYM), and the runner installed at
    // every agent start is smaller without it.
    assert!(has("GCC_GENERATE_DEBUGGING_SYMBOLS=NO"), "{args:?}");
    assert!(has("COMPILER_INDEX_STORE_ENABLE=NO"), "{args:?}");
    assert!(has("MODULE_CACHE_DIR=/cache/ModuleCache"), "{args:?}");
    let intel = virtuoso::ios::agent_build_args(Path::new("/src"), Path::new("/b"), Path::new("/cache"), "x86_64");
    assert!(intel.iter().any(|x| x == "ARCHS=x86_64"), "{intel:?}");
}

#[test]
fn an_agent_built_by_another_xcode_is_not_reused() {
    // The same sources built by another Xcode are another build: its runner and XCTest differ.
    use std::path::Path;
    let a = virtuoso::ios::agent_cache_dir(Path::new("/home"), 7, Some(Path::new("/Applications/Xcode_16.4.app/Contents/Developer")));
    let b = virtuoso::ios::agent_cache_dir(Path::new("/home"), 7, Some(Path::new("/Applications/Xcode_26.0.app/Contents/Developer")));
    assert_ne!(a, b);
    assert!(a.starts_with("/home/Library/Caches/virtuoso"), "{}", a.display());
    assert_eq!(a, virtuoso::ios::agent_cache_dir(Path::new("/home"), 7, Some(Path::new("/Applications/Xcode_16.4.app/Contents/Developer"))));
}

#[test]
fn the_agent_runs_with_its_test_settings_spelled_out() {
    // One test, never in parallel, never timed out by XCTest: not left to defaults.
    let args = virtuoso::ios::agent_test_args("/x.xctestrun", "UDID");
    let s = args.join(" ");
    assert!(s.starts_with("test-without-building -xctestrun /x.xctestrun -destination id=UDID"), "{s}");
    assert!(s.contains("-only-testing:VirtuosoAgent/VirtuosoAgent/testServe"), "{s}");
    assert!(s.contains("-parallel-testing-enabled NO"), "{s}");
    assert!(s.contains("-test-timeouts-enabled NO"), "{s}");
}

#[test]
fn the_agent_keeps_no_xctest_attachments() {
    // The agent's one test serves until it is stopped: XCTest's automatic screenshots and
    // attachments for each XCTest call (launch, openURL, a gesture) would pile up all its life.
    let (_, scheme) = virtuoso::ios::agent_sources().iter().find(|(p, _)| p.ends_with("VirtuosoAgent.xcscheme")).expect("the agent's scheme");
    let scheme = String::from_utf8_lossy(scheme);
    let test = &scheme[scheme.find("<TestAction").unwrap()..];
    assert!(test[..test.find('>').unwrap()].contains(r#"systemAttachmentLifetime = "keepNever""#), "the TestAction keeps attachments");
    let (_, spec) = virtuoso::ios::agent_sources().iter().find(|(p, _)| p.ends_with("project.yml")).expect("project.yml");
    assert!(String::from_utf8_lossy(spec).contains("captureScreenshotsAutomatically: false"), "project.yml's test action");
}

#[test]
fn the_agent_sources_hash_is_worked_out_when_virtuoso_is_built() {
    // The same FNV-1a over each source's path and bytes, in order, that names the agent's build:
    // not worked out again over ~100 KB on every run.
    let mut hash: u64 = 0xcbf29ce484222325;
    for (path, bytes) in virtuoso::ios::agent_sources() {
        for b in path.as_bytes().iter().chain(bytes.iter()) {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    assert_eq!(virtuoso::ios::agent_sources_hash(), hash);
}

#[test]
fn the_run_uses_the_connection_that_found_its_agent_answering() {
    // The device check pinged the agent: the run goes on that connection, not on another one
    // opened (and the token sent again) a moment later. This stand-in takes one connection.
    use std::io::{Read, Write};
    use virtuoso::ios::{agent_answers, IosDevice};
    let udid = format!("test-reuse-{}", std::process::id());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut c, _) = listener.accept().unwrap();
        fn frame(c: &mut std::net::TcpStream) -> Option<String> {
            let mut len = [0u8; 4];
            c.read_exact(&mut len).ok()?;
            let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
            c.read_exact(&mut body).ok()?;
            Some(String::from_utf8_lossy(&body).into_owned())
        }
        frame(&mut c); // the token: not answered
        while let Some(req) = frame(&mut c) {
            let reply: &[u8] = if req.contains("\"info\"") { br#"{"ok":true,"width":390,"height":844}"# } else { br#"{"ok":true}"# };
            if c.write_all(&(reply.len() as u32).to_be_bytes()).and_then(|_| c.write_all(reply)).is_err() {
                return;
            }
        }
    });
    let build = format!("{}/reuse-{}.xctestrun", std::env::temp_dir().display(), std::process::id());
    let state = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"));
    std::fs::write(&state, serde_json::json!({"port": port, "token": "t", "build": build}).to_string()).unwrap();
    std::env::set_var("VIRTUOSO_IOS_AGENT", &build);
    assert!(agent_answers(&udid));
    let d = IosDevice::connect(&udid);
    std::fs::remove_file(&state).unwrap();
    let mut d = d.expect("connected on the connection agent_answers made");
    use virtuoso::device::Device;
    assert_eq!(d.screen_rect().unwrap().width(), 390);
}

#[test]
fn a_tap_reply_says_whether_the_app_was_still_animating() {
    // The agent waits for the app's animations before a touch, up to a cap: a tap that went in
    // at the cap (a popup presenting, an endless spinner) may have been ignored.
    use virtuoso::screen::ScreenReply;
    let tapped_while_busy = |reply: &[u8]| ScreenReply::parse(reply, 390, 844).unwrap().busy;
    assert!(tapped_while_busy(br#"{"ok":true,"busy":true}"#));
    assert!(!tapped_while_busy(br#"{"ok":true,"busy":false}"#));
    assert!(!tapped_while_busy(br#"{"ok":true}"#));
}

#[test]
fn an_agent_launched_before_the_simulator_knew_it_is_started_again() {
    // Just installed on a simulator just booted, the test runner can be launched before
    // SpringBoard has registered it: xcodebuild gives up at once, and a second try works.
    use virtuoso::ios::agent_launch_raced;
    let log = r#"Failure Reason: The request was denied by service delegate (SBMainWorkspace) for reason: NotFound ("Application "dev.virtuoso.agent.xctrunner" is unknown to FrontBoard")."#;
    assert!(agent_launch_raced(log));
    assert!(!agent_launch_raced("** TEST FAILED **\nTesting failed: the agent crashed"));
}

/// A stand-in iOS agent on `udid` that takes one connection: it logs each request's op and
/// answers `info` with a 390x844 screen, `screen` with one node, anything else ok. The state
/// file is written for it, as a run of the agent leaves one.
fn stand_in_agent(udid: &str) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
    use std::io::{Read, Write};
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = log.clone();
    std::thread::spawn(move || {
        let (mut c, _) = listener.accept().unwrap();
        fn frame(c: &mut std::net::TcpStream) -> Option<serde_json::Value> {
            let mut len = [0u8; 4];
            c.read_exact(&mut len).ok()?;
            let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
            c.read_exact(&mut body).ok()?;
            serde_json::from_slice(&body).ok()
        }
        frame(&mut c); // the token: not answered
        while let Some(req) = frame(&mut c) {
            let op = req["op"].as_str().unwrap_or("").to_string();
            let reply = match op.as_str() {
                "info" => r#"{"ok":true,"width":390,"height":844}"#.to_string(),
                "screen" => r#"{"ok":true,"seq":1,"windows":[{"pkg":"com.app","type":1}],"nodes":[{"d":0,"k":"Application","b":[0,0,390,844]}]}"#.to_string(),
                _ => r#"{"ok":true}"#.to_string(),
            };
            seen.lock().unwrap().push(op);
            if c.write_all(&(reply.len() as u32).to_be_bytes()).and_then(|_| c.write_all(reply.as_bytes())).is_err() {
                return;
            }
        }
    });
    // Every test names the same build: VIRTUOSO_IOS_AGENT is the process's, set by tests at once.
    let build = format!("{}/reuse-{}.xctestrun", std::env::temp_dir().display(), std::process::id());
    std::env::set_var("VIRTUOSO_IOS_AGENT", &build);
    let state = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"));
    std::fs::write(&state, serde_json::json!({"port": port, "token": "t", "build": build}).to_string()).unwrap();
    log
}

#[test]
fn each_simulator_checked_keeps_the_connection_that_found_its_agent() {
    // --device A,B checks both agents before either run connects: each run goes on its own
    // simulator's connection (the stand-ins take one connection each), whichever was checked last.
    use virtuoso::device::Device;
    use virtuoso::ios::{agent_answers, IosDevice};
    let (a, b) = (format!("test-pair-a-{}", std::process::id()), format!("test-pair-b-{}", std::process::id()));
    let (log_a, log_b) = (stand_in_agent(&a), stand_in_agent(&b));
    assert!(agent_answers(&a) && agent_answers(&b));
    let (da, db) = (IosDevice::connect(&a), IosDevice::connect(&b));
    for u in [&a, &b] {
        std::fs::remove_file(std::env::temp_dir().join(format!("virtuoso-ios-agent-{u}.json"))).unwrap();
    }
    assert_eq!(da.expect("A on the connection its check made").screen_rect().unwrap().width(), 390);
    assert_eq!(db.expect("B on the connection its check made").screen_rect().unwrap().width(), 390);
    // Each agent asked once, by the check: its answer is the run's too.
    assert_eq!(log_a.lock().unwrap().len(), 1);
    assert_eq!(log_b.lock().unwrap().len(), 1);
}

#[test]
fn a_launchs_permission_grant_is_remembered_across_runs_while_the_stores_are_untouched() {
    // Every run is a new process: the grant a launch makes (`simctl privacy`, ~0.3 s) is not
    // made again while the files the simulator keeps grants in are as the last grant left them.
    use virtuoso::ios::{forget_grant, granted_already, launch_permissions, permission_fingerprint, remember_grant};
    let id = std::process::id();
    let udid = format!("test-grant-{id}");
    let data = std::env::temp_dir().join(format!("virtuoso-simdata-{id}"));
    std::fs::create_dir_all(data.join("Library/TCC")).unwrap();
    std::fs::create_dir_all(data.join("Library/Caches/locationd")).unwrap();
    std::fs::write(data.join("Library/TCC/TCC.db"), b"db").unwrap();
    std::fs::write(data.join("Library/TCC/TCC.db-wal"), b"wal").unwrap();
    std::fs::write(data.join("Library/Caches/locationd/clients.plist"), b"plist").unwrap();
    let all = launch_permissions(&[]);
    let fp = permission_fingerprint(&data, &udid, "com.app");
    assert!(!granted_already(&udid, "com.app", &all, &fp), "nothing granted yet");
    remember_grant(&udid, "com.app", &all, &fp);
    assert!(granted_already(&udid, "com.app", &all, &permission_fingerprint(&data, &udid, "com.app")));
    assert!(!granted_already(&udid, "com.other", &all, &fp), "another app's");
    let camera = vec![("camera".to_string(), "deny".to_string())];
    assert!(!granted_already(&udid, "com.app", &camera, &fp), "other permissions");
    // Any write to a store since (a revoke, Settings, another tool's reset): granted again.
    for store in ["Library/TCC/TCC.db-wal", "Library/TCC/TCC.db", "Library/Caches/locationd/clients.plist"] {
        remember_grant(&udid, "com.app", &all, &permission_fingerprint(&data, &udid, "com.app"));
        std::fs::write(data.join(store), b"changed since").unwrap();
        assert!(!granted_already(&udid, "com.app", &all, &permission_fingerprint(&data, &udid, "com.app")), "{store} changed");
    }
    // setPermissions since: granted again.
    remember_grant(&udid, "com.app", &all, &permission_fingerprint(&data, &udid, "com.app"));
    forget_grant(&udid, "com.app");
    assert!(!granted_already(&udid, "com.app", &all, &permission_fingerprint(&data, &udid, "com.app")));
    std::fs::remove_dir_all(&data).unwrap();
}

#[test]
fn permissions_for_different_services_are_set_at_once() {
    // Each `simctl privacy` takes ~0.3 s. `all` goes first and alone (it covers every service),
    // as does whatever came before it; after it, each service's settings in order, the services
    // side by side. Location and location-always are one setting in locationd: never side by side.
    use virtuoso::ios::privacy_steps;
    let p = |k: &str, v: &str| (k.to_string(), v.to_string());
    let (first, apart) = privacy_steps(&[p("location", "allow"), p("photos", "deny"), p("contacts", "allow"), p("camera", "allow")]);
    assert!(first.is_empty());
    assert_eq!(apart, vec![vec![("grant", "location")], vec![("revoke", "photos")], vec![("grant", "contacts")]]);
    let (first, apart) = privacy_steps(&[p("photos", "deny"), p("all", "allow"), p("location", "deny"), p("contacts", "deny"), p("locationAlways", "allow"), p("location", "unset")]);
    assert_eq!(first, vec![("revoke", "photos"), ("grant", "all")]);
    assert_eq!(apart, vec![vec![("revoke", "location"), ("grant", "location-always"), ("reset", "location")], vec![("revoke", "contacts")]]);
    let (first, apart) = privacy_steps(&[p("all", "allow")]);
    assert_eq!((first, apart.len()), (vec![("grant", "all")], 0));
}

#[test]
fn the_check_that_finds_the_agent_answering_asks_for_what_the_run_needs() {
    // The device check's request proves the agent alive and brings its screen size: the run
    // does not ask again on the same connection (one round trip a run).
    use virtuoso::device::Device;
    use virtuoso::ios::{agent_answers, IosDevice};
    let udid = format!("test-info-{}", std::process::id());
    let log = stand_in_agent(&udid);
    assert!(agent_answers(&udid));
    let d = IosDevice::connect(&udid);
    std::fs::remove_file(std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"))).unwrap();
    assert_eq!(d.unwrap().screen_rect().unwrap().width(), 390);
    assert_eq!(*log.lock().unwrap(), vec!["info"]);
}

#[test]
fn the_agent_build_the_last_run_saved_is_taken_without_listing_the_products() {
    // The state file names the build its agent runs: while it is still this cache's simulator
    // build, the products folder need not be listed to find it again.
    use virtuoso::ios::saved_xctestrun;
    let products = std::env::temp_dir().join(format!("virtuoso-products-{}", std::process::id()));
    std::fs::create_dir_all(&products).unwrap();
    let run = products.join("VirtuosoAgent_iphonesimulator26.2-arm64.xctestrun");
    let saved = run.to_string_lossy().into_owned();
    assert_eq!(saved_xctestrun(&products, &saved), None, "not built (any more)");
    std::fs::write(&run, b"").unwrap();
    assert_eq!(saved_xctestrun(&products, &saved), Some(run.clone()));
    // Another cache's build (other sources, another Xcode), or a device build: listed for.
    let other = std::env::temp_dir().join("VirtuosoAgent_iphonesimulator26.2-arm64.xctestrun");
    assert_eq!(saved_xctestrun(&products, &other.to_string_lossy()), None);
    let device = products.join("VirtuosoAgent_iphoneos26.2-arm64.xctestrun");
    std::fs::write(&device, b"").unwrap();
    assert_eq!(saved_xctestrun(&products, &device.to_string_lossy()), None);
    std::fs::remove_dir_all(&products).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn the_macos_binary_names_only_the_dylibs_it_uses() {
    // dyld loads every dylib the binary names at each start: libiconv, which std names and
    // nothing in virtuoso calls into, is left out by the linker (`otool -L` listed it).
    let out = std::process::Command::new("otool").args(["-L", env!("CARGO_BIN_EXE_virtuoso")]).output().expect("otool");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && text.contains("libSystem"), "{text}");
    assert!(!text.contains("libiconv"), "{text}");
}

#[test]
fn locationds_authorization_for_an_app_is_read_from_plutils_listing() {
    use virtuoso::ios::location_authorization;
    let listing = "{\n  \"icom.apple.Maps:\" => {\n    \"Authorization\" => 2\n  }\n  \"icom.app:\" => {\n    \"AuthorizationUpgradeAvailable\" => 0\n    \"Authorization\" => 4\n    \"BundleId\" => \"com.app\"\n    \"VectorClocks\" => [\n    ]\n  }\n  \"icom.app.widget:\" => {\n    \"Authorization\" => 1\n  }\n  \"MigrationVersionNumber\" => 17\n}\n";
    assert_eq!(location_authorization(listing, "com.app").as_deref(), Some("4"));
    assert_eq!(location_authorization(listing, "com.apple.Maps").as_deref(), Some("2"));
    assert_eq!(location_authorization(listing, "com.other"), None);
}

#[test]
fn a_grant_whose_stores_were_rewritten_stands_while_the_apps_own_permissions_are_as_it_left_them() {
    // locationd saves its clients ~10 s after a grant, after the run has ended: the next run
    // saw a store changed and granted everything again. What the app itself was granted (its
    // TCC rows, its location authorization) is compared with what it was right after the grant.
    use virtuoso::ios::{forget_grant, grant_stands_by_content, granted_already, launch_permissions, remember_grant, remember_grant_with};
    let udid = format!("test-grant-content-{}", std::process::id());
    let all = launch_permissions(&[]);
    remember_grant_with(&udid, "com.app", &all, "fp1", Some("kTCCServiceAll|2;loc=4"));
    assert!(granted_already(&udid, "com.app", &all, "fp1"), "the stores as the grant left them");
    assert!(!granted_already(&udid, "com.app", &all, "fp2"));
    assert!(grant_stands_by_content(&udid, "com.app", &all, Some("kTCCServiceAll|2;loc=4")));
    assert!(!grant_stands_by_content(&udid, "com.app", &all, Some("kTCCServiceAll|2;loc=2")), "location revoked");
    assert!(!grant_stands_by_content(&udid, "com.app", &all, Some("kTCCServiceAll|0;loc=4")), "a service revoked");
    assert!(!grant_stands_by_content(&udid, "com.app", &all, None), "the stores could not be read");
    let camera = vec![("camera".to_string(), "deny".to_string())];
    assert!(!grant_stands_by_content(&udid, "com.app", &camera, Some("kTCCServiceAll|2;loc=4")), "other permissions");
    // A note with no record of what was granted (an older virtuoso's): granted again.
    remember_grant(&udid, "com.app", &all, "fp1");
    assert!(!grant_stands_by_content(&udid, "com.app", &all, Some("kTCCServiceAll|2;loc=4")));
    remember_grant_with(&udid, "com.app", &all, "fp1", Some("kTCCServiceAll|2;loc=4"));
    forget_grant(&udid, "com.app");
    assert!(!grant_stands_by_content(&udid, "com.app", &all, Some("kTCCServiceAll|2;loc=4")), "setPermissions since");
}
