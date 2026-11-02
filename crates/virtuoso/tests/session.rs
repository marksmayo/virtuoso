//! Interactive sessions: commands run one at a time, the passing ones saved as a flow.

use virtuoso::runner::RunOptions;
use virtuoso::session::Session;
use virtuoso::testing::{node, FakeDevice, FakeScreen};
use std::time::Duration;

fn app() -> FakeDevice {
    let mut d = FakeDevice::new();
    d.add_screen("home", FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160]).goes_to("welcome")]));
    d.add_screen("welcome", FakeScreen::new(vec![node("Welcome", [100, 100, 300, 160])]));
    d.show("home");
    d
}

fn session() -> Session {
    Session::new("com.app", RunOptions { lookup_timeout: Duration::from_millis(200), optional_lookup_timeout: Duration::from_millis(100), ..Default::default() })
}

#[test]
fn passing_commands_are_kept_and_saved_as_a_flow() {
    let mut d = app();
    let mut s = session();
    let out = s.run(&mut d, &["tapOn: Sign in".into(), "assertVisible: Welcome".into()]);
    assert!(out.iter().all(|r| r.passed), "{out:?}");
    assert_eq!(d.log, vec!["tap 200,130"]);
    assert_eq!(s.flow_yaml(), "appId: com.app\n---\n- tapOn: Sign in\n- assertVisible: Welcome\n");
    let flow = virtuoso::flow::parse_flow(&s.flow_yaml(), None).unwrap();
    assert_eq!(flow.commands.len(), 2);
}

#[test]
fn a_failing_command_is_reported_not_kept_and_stops_the_rest() {
    let mut d = app();
    let mut s = session();
    let out = s.run(&mut d, &["tapOn: Register".into(), "tapOn: Sign in".into()]);
    assert_eq!(out.len(), 1, "the rest do not run");
    assert!(!out[0].passed && out[0].error.as_deref().unwrap_or("").contains("Register"), "{out:?}");
    assert_eq!(s.log(), &[] as &[String]);
}

#[test]
fn a_command_that_is_not_valid_yaml_or_maestro_is_an_error_not_a_crash() {
    let mut d = app();
    let mut s = session();
    let out = s.run(&mut d, &["tapOnTheMoon: true".into()]);
    assert!(!out[0].passed && out[0].error.as_deref().unwrap_or("").contains("tapOnTheMoon"), "{out:?}");
}

#[test]
fn clear_starts_the_recording_over() {
    let mut d = app();
    let mut s = session();
    s.run(&mut d, &["tapOn: Sign in".into()]);
    s.clear();
    assert_eq!(s.flow_yaml(), "appId: com.app\n---\n");
}

#[test]
fn a_multi_line_command_is_kept_whole() {
    let mut d = app();
    let mut s = session();
    let out = s.run(&mut d, &["tapOn:\n  text: Sign in".into()]);
    assert!(out[0].passed, "{out:?}");
    assert_eq!(s.flow_yaml(), "appId: com.app\n---\n- tapOn:\n    text: Sign in\n");
}
