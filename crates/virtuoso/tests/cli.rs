//! The `virtuoso` command line, run as a user would run it.

use std::path::Path;
use std::process::{Command, Output};

fn virtuoso(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn version_prints_the_version() {
    let o = virtuoso(&["--version"]);
    assert!(o.status.success());
    assert!(text(&o).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_names_the_embedded_agent() {
    // Two builds with different agents on one device push over each other's: the bench
    // checks the tools it compares embed the same one.
    let o = virtuoso(&["--version"]);
    let t = text(&o);
    let hash = t.split("agent ").nth(1).map(|h| h.trim().trim_end_matches(')')).unwrap_or("");
    assert!(hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()), "{t}");
}

#[test]
fn ios_is_a_platform_and_web_is_not() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.yaml"), "appId: a
---
- launchApp
").unwrap();
    let flows = d.path().to_str().unwrap();
    let o = virtuoso(&["--platform", "ios", "lint", flows]);
    assert!(o.status.success(), "{}", text(&o));
    let o = virtuoso(&["--platform", "web", "lint", flows]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("android") && text(&o).contains("ios"), "{}", text(&o));
}

#[test]
fn lint_accepts_valid_flows() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.yaml"), "appId: a\n---\n- launchApp\n").unwrap();
    std::fs::write(d.path().join("b.yaml"), "appId: a\n---\n- tapOn: X\n").unwrap();
    let o = virtuoso(&["lint", d.path().to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("2 flows"), "{}", text(&o));
}

#[test]
fn lint_rejects_an_invalid_flow_naming_file_and_problem() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("bad.yaml"), "appId: a\n---\n- tapOn:\n    txt: X\n").unwrap();
    let o = virtuoso(&["lint", d.path().to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    let t = text(&o);
    assert!(t.contains("bad.yaml") && t.contains("txt"), "{t}");
}

#[test]
fn lint_follows_run_flow_files_and_reports_missing_ones() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("main.yaml"), "appId: a\n---\n- runFlow: missing.yaml\n").unwrap();
    let o = virtuoso(&["lint", d.path().join("main.yaml").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("missing.yaml"), "{}", text(&o));
}

#[test]
fn lint_names_a_broken_sub_flow_for_each_flow_that_runs_it() {
    // Sub-flows are parsed once for the whole lint; each reference still reports the problem.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("setup.yaml"), "appId: a\n---\n- tapOn:\n    txt: X\n").unwrap();
    for f in ["a.yaml", "b.yaml"] {
        std::fs::write(d.path().join(f), "appId: a\n---\n- runFlow: setup.yaml\n").unwrap();
    }
    let o = virtuoso(&["lint", d.path().join("a.yaml").to_str().unwrap(), d.path().join("b.yaml").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let t = text(&o);
    assert_eq!(t.matches("txt").count(), 2, "{t}");
    assert!(t.contains("2 problems in 2 flows"), "{t}");
}

#[test]
fn test_with_a_missing_file_is_a_usage_error() {
    let o = virtuoso(&["test", "no/such/flow.yaml"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no/such/flow.yaml"));
}

#[test]
fn test_on_a_device_that_is_not_connected_says_so() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.yaml");
    std::fs::write(&f, "appId: a\n---\n- back\n").unwrap();
    let o = virtuoso(&["--device", "nope-123", "test", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("nope-123"), "{}", text(&o));
}

#[test]
fn maestro_flags_that_change_nothing_locally_are_accepted() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.yaml"), "appId: a\n---\n- back\n").unwrap();
    let o = virtuoso(&["lint", "--headless", d.path().to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
}

/// A whole run on a device, with a JUnit report (needs VIRTUOSO_DEVICE).
#[test]
fn test_runs_a_flow_on_a_device_and_writes_junit() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("smoke.yaml");
    std::fs::write(&f, "appId: com.android.settings\n---\n- launchApp\n- assertVisible: Network & internet\n").unwrap();
    let out = d.path().join("report.xml");
    let o = virtuoso(&["--device", &serial, "test", f.to_str().unwrap(), "--format", "junit", "--output", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let xml = std::fs::read_to_string(&out).unwrap();
    assert!(xml.contains("status=\"SUCCESS\""), "{xml}");
    assert!(Path::new(&out).exists());
}

#[test]
fn a_failing_flow_exits_one_on_a_device() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("fail.yaml");
    std::fs::write(&f, "appId: com.android.settings\n---\n- assertVisible:\n    text: Definitely not here\n    optional: false\n").unwrap();
    let o = virtuoso(&["--device", &serial, "test", f.to_str().unwrap(), "--find-timeout", "1000"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("Definitely not here"));
}

#[test]
fn a_missing_env_file_is_a_usage_error_naming_it() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.yaml");
    std::fs::write(&f, "appId: a\n---\n- back\n").unwrap();
    let o = virtuoso(&["test", "--env-file", "nope.env", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("nope.env"), "{}", text(&o));
}

#[test]
fn screenshot_saves_a_png_of_the_device_screen() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("shot.png");
    let o = virtuoso(&["--device", &serial, "screenshot", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(&std::fs::read(&out).unwrap()[1..4], b"PNG");
}

#[test]
fn retries_run_a_failing_flow_again_and_report_each_attempt() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("flaky.yaml");
    std::fs::write(&f, "appId: com.android.settings\n---\n- assertVisible: Never shown\n").unwrap();
    let o = virtuoso(&["--device", &serial, "test", f.to_str().unwrap(), "--find-timeout", "300", "--retries", "2"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert_eq!(text(&o).matches("> Flow flaky").count(), 3, "{}", text(&o));
}

#[test]
fn mcp_answers_initialize_and_tools_list_over_stdio() {
    use std::io::{BufRead, BufReader, Write as _};
    let mut child = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .arg("mcp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
    drop(stdin);
    let lines: Vec<String> = BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok).collect();
    child.wait().unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains("\"serverInfo\""));
    assert!(lines[1].contains("\"look\""));
}

#[test]
fn mcp_answers_a_line_that_is_not_json_with_a_parse_error() {
    use std::io::{BufRead, BufReader, Write as _};
    let mut child = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .arg("mcp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "not json").unwrap();
    drop(stdin);
    let lines: Vec<String> = BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok).collect();
    child.wait().unwrap();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with(r#"{"error":{"code":-32700,"message":"expected ident"#), "{lines:?}");
    assert!(lines[0].ends_with(r#""},"id":null,"jsonrpc":"2.0"}"#), "{lines:?}");
}

#[test]
fn skills_installs_an_agent_skill_or_prints_it() {
    let d = tempfile::tempdir().unwrap();
    let o = virtuoso(&["skills", "--dir", d.path().to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let skill = std::fs::read_to_string(d.path().join("SKILL.md")).unwrap();
    assert!(skill.starts_with("---\nname: virtuoso\n"), "{skill}");
    assert!(skill.contains("virtuoso test") && skill.contains("tapOn"), "{skill}");
    let o = virtuoso(&["skills", "--print"]);
    assert!(o.status.success());
    assert_eq!(String::from_utf8_lossy(&o.stdout), skill);
}

#[test]
fn doctor_checks_the_toolchain() {
    let o = virtuoso(&["doctor"]);
    let t = text(&o);
    assert!(t.contains("adb") && t.contains("devices") && t.contains("curl"), "{t}");
}

#[test]
fn doctor_on_ios_builds_the_agent_instead_of_checking_adb() {
    // The iOS agent is built once per sources and Xcode (about a minute): `doctor` builds it, so
    // a CI job can before the jobs that run flows. adb has no part on iOS. (No Xcode here: the
    // build is tried, and fails.)
    let home = tempfile::tempdir().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .args(["--platform", "ios", "doctor"])
        .env("HOME", home.path())
        .env_remove("VIRTUOSO_IOS_AGENT")
        .output()
        .unwrap();
    let t = text(&o);
    assert!(t.contains("iOS agent"), "{t}");
    assert!(!t.contains("adb"), "{t}");
    if !cfg!(target_os = "macos") {
        assert_eq!(o.status.code(), Some(1), "{t}");
    }
}

#[test]
fn a_missing_app_file_is_a_usage_error_naming_it() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.yaml");
    std::fs::write(&f, "appId: a\n---\n- launchApp\n").unwrap();
    let o = virtuoso(&["test", f.to_str().unwrap(), "--app-file", "nope.apk"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("nope.apk"), "{}", text(&o));
}

#[test]
fn app_file_installs_the_app_before_the_run() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let apk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../maestro-runner/drivers/android/devicelab-android-driver.apk");
    if !apk.exists() {
        return;
    }
    let updated = || {
        let o = Command::new("adb").args(["-s", &serial, "shell", "dumpsys package dev.devicelab.driver.android | grep lastUpdateTime"]).output().unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let before = updated();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("smoke.yaml");
    std::fs::write(&f, "appId: com.android.settings\n---\n- pressKey: Home\n").unwrap();
    let o = virtuoso(&["--device", &serial, "test", f.to_str().unwrap(), "--app-file", apk.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert_ne!(updated(), before, "the app was reinstalled");
}

#[test]
fn latency_measures_agent_round_trips_on_a_device() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let o = virtuoso(&["--device", &serial, "latency", "--count", "5"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    for op in ["ping", "screen", "screenshot"] {
        assert!(t.lines().any(|l| l.starts_with(op) && l.contains("p50") && l.contains("p95")), "{op}: {t}");
    }
}

#[test]
fn continuous_runs_the_flows_again_when_one_changes() {
    use std::io::{BufRead, BufReader};
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("watch.yaml");
    std::fs::write(&f, "appId: com.android.settings\n---\n- pressKey: Home\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .args(["--device", &serial, "test", "--continuous", "--no-ansi", f.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut watching = 0;
    let mut edited = false;
    while let Some(Ok(line)) = lines.next() {
        if line.starts_with("watching") {
            watching += 1;
            if watching == 2 {
                break;
            }
            std::fs::write(&f, "appId: com.android.settings\n---\n- pressKey: Home\n- pressKey: Home\n").unwrap();
            edited = true;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(edited && watching == 2, "a second run after the edit");
}

#[test]
fn a_session_runs_commands_one_at_a_time_and_saves_them_as_a_flow() {
    let Ok(serial) = std::env::var("VIRTUOSO_DEVICE") else { return };
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("session.json");
    let s = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(args).env("VIRTUOSO_SESSION_FILE", &state).output().unwrap();
    let o = s(&["--device", &serial, "session", "start", "--app", "com.android.settings"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = s(&["do", "launchApp", "tapOn: Network & internet"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = s(&["look"]);
    assert!(text(&o).contains("Internet"), "{}", text(&o));
    let o = s(&["do", "tapOn: Definitely not here"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let flow = d.path().join("explored.yaml");
    let o = s(&["session", "save", flow.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(&flow).unwrap(), "appId: com.android.settings\n---\n- launchApp\n- tapOn: Network & internet\n");
    let o = s(&["session", "stop"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = s(&["look"]);
    assert_eq!(o.status.code(), Some(2), "no session any more: {}", text(&o));
}

#[cfg(windows)]
#[test]
fn winmm_is_not_loaded_for_commands_that_need_no_device() {
    // winmm (for the 1 ms timer that device polls want) costs ~250 KB of working set at
    // start-up: it is loaded by name when a device connects, not imported.
    let exe = std::fs::read(env!("CARGO_BIN_EXE_virtuoso")).unwrap();
    let lower: Vec<u8> = exe.iter().map(u8::to_ascii_lowercase).collect();
    assert!(!lower.windows(9).any(|w| w == b"winmm.dll"), "virtuoso.exe imports winmm.dll");
}
