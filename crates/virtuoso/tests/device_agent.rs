//! Tests against a real Android device or emulator. They run when VIRTUOSO_DEVICE names one
//! (`VIRTUOSO_DEVICE=emulator-5554 cargo test --test device_agent -- --test-threads=1`) and pass
//! trivially otherwise.

use virtuoso::android::{Adb, AndroidAgent};
use serde_json::json;

fn device() -> Option<Adb> {
    let serial = std::env::var("VIRTUOSO_DEVICE").ok()?;
    Some(Adb::new(&serial))
}

#[test]
fn the_agent_starts_and_answers_a_ping() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).expect("agent start");
    agent.client().ping().unwrap();
}

#[test]
fn the_screen_lists_windows_and_visible_nodes() {
    let Some(adb) = device() else { return };
    adb.shell("input keyevent KEYCODE_HOME").unwrap();
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let reply = agent.client().call(json!({"op": "screen"})).unwrap();
    let nodes = reply["nodes"].as_array().unwrap();
    assert!(!reply["windows"].as_array().unwrap().is_empty());
    assert!(nodes.len() > 5, "only {} nodes", nodes.len());
    assert!(nodes.iter().any(|n| n["t"] != "" || n["c"] != ""), "no node has text");
}

#[test]
fn display_info_reports_a_portrait_size() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let info = agent.client().call(json!({"op": "info"})).unwrap();
    assert!(info["width"].as_i64().unwrap() > 300);
    assert!(info["height"].as_i64().unwrap() > info["width"].as_i64().unwrap());
}

#[test]
fn exec_runs_as_the_shell_user_without_a_shell() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let r = agent.client().call(json!({"op": "exec", "argv": ["id", "-un"]})).unwrap();
    assert_eq!(r["out"].as_str().unwrap().trim(), "shell");
    let r = agent.client().call(json!({"op": "exec", "argv": ["echo", "a;id"]})).unwrap();
    assert_eq!(r["out"].as_str().unwrap().trim(), "a;id");
}

#[test]
fn a_second_start_replaces_the_first_agent() {
    let Some(adb) = device() else { return };
    let _first = AndroidAgent::start(&adb).unwrap();
    let mut second = AndroidAgent::start(&adb).unwrap();
    second.client().ping().unwrap();
}

fn animation_scales(adb: &Adb) -> String {
    adb.shell("for k in window_animation_scale transition_animation_scale animator_duration_scale; do settings get global $k; done").unwrap()
}

#[test]
fn starting_the_agent_can_turn_animations_off_and_forwards_a_stable_port() {
    let Some(adb) = device() else { return };
    adb.shell("settings put global window_animation_scale 1; settings put global animator_duration_scale 1").unwrap();
    let mut agent = AndroidAgent::start_with(&adb, true).unwrap();
    agent.client().ping().unwrap();
    assert!(animation_scales(&adb).split_whitespace().all(|v| v == "0" || v == "0.0"), "{}", animation_scales(&adb));
    let forwards = adb.run(&["forward", "--list"]).unwrap();
    assert!(forwards.contains(&format!("{} tcp:{}", adb.serial, virtuoso::android::local_port_for(&adb.serial))), "{forwards}");
}

#[test]
fn the_agent_reports_the_display_when_it_starts() {
    let Some(adb) = device() else { return };
    let agent = AndroidAgent::start_with(&adb, false).unwrap();
    let d = agent.display();
    assert!(d.width > 300 && d.height > d.width && d.sdk >= 26, "{d:?}");
}

#[test]
fn dropping_the_agent_returns_at_once_and_the_agent_goes() {
    let Some(adb) = device() else { return };
    let agent = AndroidAgent::start_with(&adb, false).unwrap();
    let t = std::time::Instant::now();
    drop(agent);
    assert!(t.elapsed() < std::time::Duration::from_millis(50), "drop took {:?}", t.elapsed());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !adb.shell("pidof virtuoso-agent || true").unwrap().trim().is_empty() {
        assert!(std::time::Instant::now() < deadline, "the agent is still running");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn a_settle_says_which_packages_sent_the_events_it_waited_for() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    agent.client().call(json!({"op": "key", "code": 3})).unwrap();
    let r = agent.client().call(json!({"op": "settle", "quiet": 80, "nochange": 300, "timeout": 5000})).unwrap();
    let sources = r["sources"].as_object().unwrap_or_else(|| panic!("no sources: {r}"));
    if r["events"].as_i64().unwrap_or(0) > 0 {
        assert!(!sources.is_empty(), "{r}");
        assert!(sources.keys().all(|k| k.contains('.') || k == "?"), "packages: {r}");
    }
}

#[test]
fn the_windows_op_lists_windows_without_nodes() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let r = agent.client().call(json!({"op": "windows"})).unwrap();
    let ws = r["windows"].as_array().unwrap_or_else(|| panic!("{r}"));
    assert!(!ws.is_empty() && ws.iter().all(|w| w.get("pkg").is_some() && w.get("type").is_some() && w.get("active").is_some()), "{r}");
    assert!(r.get("nodes").is_none(), "{r}");
}

#[test]
fn exec_can_mark_the_settle_start_first() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    agent.client().call(json!({"op": "key", "code": 3})).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    agent.client().call(json!({"op": "exec", "argv": ["true"], "mark": true})).unwrap();
    let r = agent.client().call(json!({"op": "settle", "quiet": 80, "nochange": 0, "timeout": 2000})).unwrap();
    assert_eq!(r["events"], 0, "the mark reset the events: {r}");
}

#[test]
fn an_action_can_carry_the_settle_that_follows_it() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let r = agent.client().call(json!({"op": "key", "code": 3, "then": {"op": "settle", "quiet": 80, "nochange": 300, "timeout": 5000}})).unwrap();
    assert!(r.get("waited").is_some() && r.get("changed").is_some(), "the settle's answer comes back: {r}");
}

#[test]
fn another_builds_agent_stays_on_the_device_unless_it_is_stale() {
    // Two virtuoso builds sharing a device used to delete each other's agent on every start,
    // so each run pushed its own again.
    let Some(adb) = device() else { return };
    let other = "/data/local/tmp/virtuoso-agent-00000000000000aa.jar";
    let stale = "/data/local/tmp/virtuoso-agent-00000000000000bb.jar";
    adb.shell(&format!("echo x > {other}; echo x > {stale}; touch -d 2020-01-01 {stale}")).unwrap();
    let agent = AndroidAgent::start(&adb).unwrap();
    drop(agent);
    let left = adb.shell(&format!("ls {other} {stale} 2>/dev/null; true")).unwrap();
    adb.shell(&format!("rm -f {other} {stale}")).unwrap();
    assert!(left.contains(other), "the other build's agent was deleted: {left}");
    assert!(!left.contains(stale), "a stale agent was kept: {left}");
}

#[test]
fn a_missing_agent_is_pushed_through_the_adb_server_and_starts() {
    let Some(adb) = device() else { return };
    let jar = format!("/data/local/tmp/virtuoso-agent-{}.jar", virtuoso::android::AGENT_HASH);
    adb.shell(&format!("rm -f {jar}")).unwrap();
    let mut agent = AndroidAgent::start(&adb).unwrap();
    agent.client().ping().unwrap();
    let size = adb.shell(&format!("stat -c %s {jar}")).unwrap();
    assert_eq!(size.trim(), virtuoso::android::AGENT_JAR.len().to_string());
}

#[test]
fn the_adb_server_takes_a_push_from_memory() {
    let Some(adb) = device() else { return };
    let bytes: Vec<u8> = (0..100_000u32).map(|i| (i * 7) as u8).collect();
    virtuoso::adb_wire::AdbWire::local().push(&adb.serial, &bytes, "/data/local/tmp/virtuoso-push-test.bin", 0o644).unwrap();
    let out = adb.shell("stat -c %s /data/local/tmp/virtuoso-push-test.bin; rm -f /data/local/tmp/virtuoso-push-test.bin").unwrap();
    assert_eq!(out.trim(), "100000");
}

#[test]
fn the_agent_hears_only_the_events_that_change_what_it_reads() {
    // Every event type subscribed means every app builds and sends clicks, hovers and
    // announcements nobody reads. Kept: those that change the node cache (Android's
    // AccessibilityCache.CACHE_CRITICAL_EVENTS_MASK).
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let info = agent.client().call(json!({"op": "info"})).unwrap();
    let types = info["eventTypes"].as_i64().expect("eventTypes in info");
    const CLICKED: i64 = 0x1;
    const HOVER_ENTER: i64 = 0x80;
    const ANNOUNCEMENT: i64 = 0x4000;
    const CACHE_CRITICAL: i64 = 0x8000 | 0x10000 | 0x8 | 0x4 | 0x10 | 0x2000 | 0x800 | 0x400000 | 0x20 | 0x1000;
    assert_eq!(types & (CLICKED | HOVER_ENTER | ANNOUNCEMENT), 0, "{types:#x}");
    assert_eq!(types & CACHE_CRITICAL, CACHE_CRITICAL, "{types:#x}");
}

/// The agent's own memory through a busy session, for comparing builds:
/// `VIRTUOSO_DEVICE=emulator-5554 cargo test --test device_agent agent_memory -- --ignored --nocapture`
#[test]
#[ignore]
fn agent_memory() {
    let Some(adb) = device() else { return };
    adb.shell("input keyevent KEYCODE_HOME").unwrap();
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let rss = |adb: &Adb| {
        let s = adb.shell("grep -E 'VmHWM|VmRSS|RssAnon' /proc/$(pidof virtuoso-agent)/status | tr -s ' \n' ' '").unwrap();
        s.trim().to_string()
    };
    println!("started:         {}", rss(&adb));
    for _ in 0..200 {
        agent.client().call(json!({"op": "screen"})).unwrap();
    }
    println!("200 screens:     {}", rss(&adb));
    let t = std::time::Instant::now();
    agent.client().call_with_blob(json!({"op": "screenshot"})).unwrap();
    println!("screenshot:      {} ({} ms)", rss(&adb), t.elapsed().as_millis());
    for _ in 0..200 {
        agent.client().call(json!({"op": "windows"})).unwrap();
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    println!("200 windows +2s:  {}", rss(&adb));
}

#[test]
fn replies_carry_text_outside_ascii_and_the_basic_plane() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let text = "caf\u{e9} \u{20ac} \u{1f600} \"q\" \\ tab\tend";
    agent.client().call(json!({"op": "clipboard", "text": text})).unwrap();
    let r = agent.client().call(json!({"op": "clipboard"})).unwrap();
    assert_eq!(r["text"], text);
}

#[test]
fn a_screenshot_comes_back_as_a_png_of_the_display() {
    let Some(adb) = device() else { return };
    let mut agent = AndroidAgent::start(&adb).unwrap();
    let info = agent.client().call(json!({"op": "info"})).unwrap();
    let (_, png) = agent.client().call_with_blob(json!({"op": "screenshot"})).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    assert_eq!(width as i64, info["width"].as_i64().unwrap());
    // The connection is still in step after the raw frame.
    agent.client().ping().unwrap();
}
