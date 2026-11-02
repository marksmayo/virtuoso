//! The commands maestro-runner adds to Maestro, and the flow-control extras.

use virtuoso::flow::parse_flow;
use virtuoso::runner::{FlowStatus, RunOptions, Runner};
use virtuoso::testing::{node, FakeDevice, FakeScreen};
use std::time::{Duration, Instant};

fn opts() -> RunOptions {
    RunOptions {
        lookup_timeout: Duration::from_millis(300),
        optional_lookup_timeout: Duration::from_millis(150),
        condition_timeout: Duration::from_millis(150),
        ..Default::default()
    }
}

fn run(dev: &mut FakeDevice, cmds: &str) -> virtuoso::runner::FlowResult {
    let flow = parse_flow(&format!("appId: com.app\n---\n{cmds}"), None).unwrap();
    Runner::new(dev, opts()).run_flow(&flow)
}

fn app() -> FakeDevice {
    let mut d = FakeDevice::new();
    d.add_screen("home", FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160]).goes_to("welcome")]));
    d.add_screen("welcome", FakeScreen::new(vec![node("Welcome", [100, 100, 300, 160])]));
    d.show("home");
    d
}

#[test]
fn wait_pauses_for_the_given_milliseconds_in_either_form() {
    let mut d = app();
    let start = Instant::now();
    let r = run(&mut d, "- wait: 150\n- wait:\n    duration: 100\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert!(start.elapsed() >= Duration::from_millis(250), "{:?}", start.elapsed());
    assert!(d.log.is_empty(), "{:?}", d.log);
}

#[test]
fn run_shell_runs_a_host_command_and_can_keep_its_output() {
    let mut d = app();
    let r = run(&mut d, "- runShell:\n    command: echo hello\n    output: GREETING\n- assertTrue: ${GREETING == 'hello'}\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
}

#[test]
fn run_shell_fails_the_step_when_the_command_fails() {
    let mut d = app();
    let r = run(&mut d, "- runShell: exit 3\n");
    assert_eq!(r.status, FlowStatus::Failed);
    assert!(r.error.as_deref().unwrap_or("").contains("exit"), "{:?}", r.error);
}

#[test]
fn run_shell_passes_its_env_to_the_command() {
    let mut d = app();
    let echo = if cfg!(windows) { "echo %WHO%" } else { "echo $WHO" };
    let r = run(&mut d, &format!("- runShell:\n    command: {echo}\n    env:\n      WHO: virtuoso\n    output: OUT\n- assertTrue: ${{OUT == 'virtuoso'}}\n"));
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
}

#[test]
fn run_flow_runs_its_else_branch_when_the_condition_is_false() {
    let mut d = app();
    let r = run(&mut d, "- runFlow:\n    when:\n      visible: Register\n    commands:\n      - back\n    else:\n      - tapOn: Sign in\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 200,130"]);
}

#[test]
fn else_commands_is_an_alias_and_a_true_condition_skips_the_else_branch() {
    let mut d = app();
    let r = run(&mut d, "- runFlow:\n    when:\n      visible: Sign in\n    commands:\n      - tapOn: Sign in\n    elseCommands:\n      - back\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 200,130"]);
}

/// Android's runtime permission dialog over the app.
fn permission_dialog() -> FakeDevice {
    let mut d = FakeDevice::new();
    d.add_screen(
        "dialog",
        FakeScreen::new(vec![
            node("Allow Camera to take pictures?", [50, 400, 650, 500]),
            node("While using the app", [50, 520, 650, 600]).goes_to("home"),
            node("Only this time", [50, 620, 650, 700]).goes_to("home"),
            node("Don't allow", [50, 720, 650, 800]).goes_to("home"),
        ]),
    );
    d.add_screen("home", FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160])]));
    d.show("dialog");
    d
}

#[test]
fn accept_alert_taps_the_dialogs_accepting_button() {
    let mut d = permission_dialog();
    let r = run(&mut d, "- acceptAlert\n- assertVisible: Sign in\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 350,560"], "While using the app");
}

#[test]
fn dismiss_alert_taps_the_dialogs_refusing_button() {
    let mut d = permission_dialog();
    let r = run(&mut d, "- dismissAlert\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 350,760"], "Don't allow");
}

#[test]
fn an_alert_command_with_no_alert_showing_is_skipped() {
    let mut d = app();
    let r = run(&mut d, "- acceptAlert\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(r.steps[0].status, virtuoso::runner::StepStatus::Skipped);
    assert!(d.log.is_empty());
}

#[test]
fn open_notifications_pulls_down_the_shade() {
    let mut d = app();
    let r = run(&mut d, "- openNotifications\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["notifications"]);
}

fn list() -> FakeDevice {
    let mut d = FakeDevice::new();
    d.add_screen("list", FakeScreen::new(vec![node("Item A", [0, 100, 700, 200]), node("Item B", [0, 300, 700, 400])]));
    d.show("list");
    d
}

#[test]
fn drag_and_drop_holds_one_element_and_drops_it_on_another() {
    let mut d = list();
    let r = run(&mut d, "- dragAndDrop:\n    from: Item A\n    to:\n      text: Item B\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["drag 350,150 -> 350,350 hold 1000ms move 1000ms"]);
}

#[test]
fn drag_and_drop_takes_points_and_its_own_timings() {
    let mut d = list();
    let r = run(&mut d, "- dragAndDrop:\n    from:\n      point: 10,20\n    to:\n      point: 30,40\n    holdDuration: 500\n    duration: 250\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["drag 10,20 -> 30,40 hold 500ms move 250ms"]);
}

#[test]
fn drag_and_drop_needs_both_ends() {
    let e = parse_flow("appId: a\n---\n- dragAndDrop:\n    from: Item A\n", None).unwrap_err();
    assert!(e.msg.contains("to"), "{}", e.msg);
}

/// "Sign in" under a snackbar that goes away after a few screen reads.
fn covered() -> FakeDevice {
    let mut d = app();
    d.add_screen(
        "covered",
        FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160]).goes_to("welcome"), node("Saved", [0, 80, 700, 180]).covers()]),
    );
    d.show("covered");
    d.show_after_reads("home", 4);
    d
}

#[test]
fn tap_with_wait_until_visible_taps_again_once_the_element_is_uncovered() {
    let mut d = covered();
    let r = run(&mut d, "- tapOn:\n    text: Sign in\n    waitUntilVisible: true\n- assertVisible: Welcome\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 200,130", "tap 200,130"]);
}

#[test]
fn tap_without_wait_until_visible_taps_once() {
    let mut d = covered();
    let r = run(&mut d, "- tapOn: Sign in\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["tap 200,130"]);
}

#[test]
fn the_element_at_a_point_is_the_topmost_one_there() {
    use virtuoso::screen::{Element, Rect, ScreenTree, VISIBLE};
    let el = |text: &str, depth: u16, window: u16, b: [i32; 4]| Element {
        text: text.into(),
        depth,
        window,
        flags: VISIBLE,
        bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
        ..Default::default()
    };
    let tree = ScreenTree::new(
        vec![
            el("dialog", 0, 0, [0, 500, 700, 1000]),
            el("app", 0, 1, [0, 0, 700, 1000]),
            el("button", 1, 1, [100, 100, 300, 160]),
            el("snackbar", 1, 1, [0, 80, 700, 180]),
        ],
        700,
        1000,
    );
    assert_eq!(tree.element_at(200, 130), Some(3), "later in the same window draws on top");
    assert_eq!(tree.element_at(200, 600), Some(0), "the first window is the top one");
    assert_eq!(tree.element_at(200, 300), Some(1));
    assert_eq!(tree.element_at(800, 300), None);
}

#[test]
fn set_clipboard_sets_the_device_clipboard_too() {
    let mut d = app();
    let r = run(&mut d, "- setClipboard: hello\n");
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["clipboard hello"]);
}

fn run_with_env(dev: &mut FakeDevice, env: &[(&str, &str)], cmds: &str) -> virtuoso::runner::FlowResult {
    let flow = parse_flow(&format!("appId: com.app\n---\n{cmds}"), None).unwrap();
    let opts = RunOptions { env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), ..opts() };
    Runner::new(dev, opts).run_flow(&flow)
}

#[test]
fn variable_values_are_masked_in_errors_and_step_results() {
    let mut d = app();
    let r = run_with_env(&mut d, &[("TOKEN", "hunter2")], "- tapOn: ${TOKEN}\n");
    assert_eq!(r.status, FlowStatus::Failed);
    let err = r.error.unwrap();
    assert!(!err.contains("hunter2") && err.contains("${TOKEN}"), "{err}");
    for s in &r.steps {
        assert!(!s.name.contains("hunter2") && !s.error.as_deref().unwrap_or("").contains("hunter2"), "{s:?}");
    }
}

#[test]
fn a_failing_shell_command_does_not_print_the_secrets_it_was_given() {
    let mut d = app();
    let r = run_with_env(&mut d, &[("TOKEN", "hunter2")], "- runShell: exit 3 ${TOKEN}\n");
    let err = r.error.unwrap();
    assert!(!err.contains("hunter2"), "{err}");
}

#[test]
fn scripts_can_require_modules_beside_them_and_modules_can_require_theirs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts/lib")).unwrap();
    std::fs::write(dir.path().join("scripts/lib/base.js"), "module.exports = { base: 20 };").unwrap();
    std::fs::write(dir.path().join("scripts/helpers.js"), "var b = require('./lib/base.js'); exports.twice = function (x) { return 2 * x + b.base - 20; };").unwrap();
    std::fs::write(dir.path().join("scripts/main.js"), "output.n = require('./helpers').twice(21);").unwrap();
    std::fs::write(dir.path().join("shared.js"), "module.exports = 'from the flow folder';").unwrap();
    let yaml = "appId: com.app\n---\n- runScript: scripts/main.js\n- assertTrue: ${output.n == 42}\n- evalScript: ${output.s = require('./shared.js')}\n- assertTrue: ${output.s == 'from the flow folder'}\n";
    let path = dir.path().join("flow.yaml");
    let flow = parse_flow(yaml, Some(&path)).unwrap();
    let mut d = app();
    let r = Runner::new(&mut d, opts()).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
}

#[test]
fn requiring_a_missing_module_fails_with_its_name() {
    let mut d = app();
    let r = run(&mut d, "- evalScript: ${require('./nope.js')}\n");
    assert_eq!(r.status, FlowStatus::Failed);
    assert!(r.error.as_deref().unwrap_or("").contains("nope.js"), "{:?}", r.error);
}

#[test]
fn video_records_the_whole_flow_into_the_output_folder() {
    let out = tempfile::tempdir().unwrap();
    let mut d = app();
    let flow = parse_flow("appId: com.app\nname: Sign in flow\n---\n- tapOn: Sign in\n", None).unwrap();
    let r = Runner::new(&mut d, RunOptions { video: true, output_dir: Some(out.path().into()), ..opts() }).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.log, vec!["record Sign in flow.mp4", "tap 200,130", "stop recording"]);
}

#[test]
fn video_still_stops_recording_when_the_flow_fails() {
    let out = tempfile::tempdir().unwrap();
    let mut d = app();
    let flow = parse_flow("appId: com.app\n---\n- tapOn: Register\n", None).unwrap();
    let r = Runner::new(&mut d, RunOptions { video: true, output_dir: Some(out.path().into()), ..opts() }).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Failed);
    assert_eq!(d.log.last().map(String::as_str), Some("stop recording"));
}
