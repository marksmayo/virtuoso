//! Reading Maestro flow files.

use virtuoso::flow::{parse_flow, Cmd, RandomKind, Selector};

fn cmds(yaml: &str) -> Vec<Cmd> {
    parse_flow(yaml, None).unwrap().commands.into_iter().map(|c| c.cmd).collect()
}

#[test]
fn a_flow_has_a_header_and_commands() {
    let flow = parse_flow("appId: com.example\n---\n- launchApp\n- tapOn: \"Sign in\"\n", None).unwrap();
    assert_eq!(flow.config.app_id.as_deref(), Some("com.example"));
    assert_eq!(flow.commands.len(), 2);
    assert!(matches!(flow.commands[0].cmd, Cmd::LaunchApp(_)));
    match &flow.commands[1].cmd {
        Cmd::TapOn(t) => assert_eq!(t.selector.text.as_deref(), Some("Sign in")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_selector_map_reads_every_field() {
    let c = cmds(
        "appId: a\n---\n- tapOn:\n    id: \"login\"\n    index: 2\n    enabled: true\n    checked: false\n    below: \"Email\"\n    childOf:\n      id: form\n    containsDescendants:\n      - text: A\n      - text: B\n    width: 100\n    height: 40\n    tolerance: 5\n",
    );
    let Cmd::TapOn(t) = &c[0] else { panic!() };
    let s = &t.selector;
    assert_eq!(s.id.as_deref(), Some("login"));
    assert_eq!(s.index.as_deref(), Some("2"));
    assert_eq!(s.enabled, Some(true));
    assert_eq!(s.checked, Some(false));
    assert_eq!(s.below.as_ref().unwrap().text.as_deref(), Some("Email"));
    assert_eq!(s.child_of.as_ref().unwrap().id.as_deref(), Some("form"));
    assert_eq!(s.contains_descendants.len(), 2);
    assert_eq!((s.width, s.height, s.tolerance), (Some(100), Some(40), Some(5)));
}

#[test]
fn an_unknown_command_is_an_error_naming_it() {
    let err = parse_flow("appId: a\n---\n- flyAway: true\n", None).unwrap_err();
    assert!(err.to_string().contains("flyAway"), "{err}");
}

#[test]
fn an_unknown_selector_field_is_an_error() {
    let err = parse_flow("appId: a\n---\n- tapOn:\n    colour: red\n", None).unwrap_err();
    assert!(err.to_string().contains("colour"), "{err}");
}

#[test]
fn a_flow_without_header_is_an_error() {
    assert!(parse_flow("- launchApp\n", None).is_err());
}

#[test]
fn label_and_optional_apply_to_any_command() {
    let flow = parse_flow("appId: a\n---\n- tapOn:\n    text: Skip\n    label: Skip onboarding\n    optional: true\n", None).unwrap();
    assert_eq!(flow.commands[0].label.as_deref(), Some("Skip onboarding"));
    assert!(flow.commands[0].optional);
    let Cmd::TapOn(t) = &flow.commands[0].cmd else { panic!() };
    assert_eq!(t.selector, Selector { text: Some("Skip".into()), ..Default::default() });
}

fn one(yaml_cmd: &str) -> Cmd {
    cmds(&format!("appId: a\n---\n{yaml_cmd}\n")).remove(0)
}

#[test]
fn bare_commands_parse() {
    assert_eq!(one("- back"), Cmd::Back);
    assert_eq!(one("- hideKeyboard"), Cmd::HideKeyboard);
    assert_eq!(one("- waitForAnimationToEnd"), Cmd::WaitForAnimationToEnd { timeout_ms: None });
    assert_eq!(one("- stopApp"), Cmd::StopApp { app_id: None });
    assert_eq!(one("- stopApp: com.other"), Cmd::StopApp { app_id: Some("com.other".into()) });
    assert_eq!(one("- killApp"), Cmd::KillApp { app_id: None });
    assert_eq!(one("- clearState"), Cmd::ClearState { app_id: None });
    assert_eq!(one("- clearKeychain"), Cmd::ClearKeychain);
    assert_eq!(one("- pasteText"), Cmd::PasteText);
    assert_eq!(one("- scroll"), Cmd::Scroll);
}

#[test]
fn the_legacy_action_form_maps_to_commands() {
    assert_eq!(one("- action: back"), Cmd::Back);
    assert_eq!(one("- action: hideKeyboard"), Cmd::HideKeyboard);
}

#[test]
fn text_input_commands_parse() {
    assert_eq!(one("- inputText: hello"), Cmd::InputText { text: "hello".into() });
    assert_eq!(one("- inputText: 1234"), Cmd::InputText { text: "1234".into() });
    assert_eq!(one("- inputText:\n    text: hi\n    label: greet"), Cmd::InputText { text: "hi".into() });
    assert_eq!(one("- eraseText"), Cmd::EraseText { chars: None });
    assert_eq!(one("- eraseText: 5"), Cmd::EraseText { chars: Some("5".into()) });
    assert_eq!(one("- eraseText:\n    charactersToErase: 7"), Cmd::EraseText { chars: Some("7".into()) });
    assert_eq!(one("- inputRandomEmail"), Cmd::InputRandom { kind: RandomKind::Email, length: None });
    assert_eq!(one("- inputRandomNumber:\n    length: 4"), Cmd::InputRandom { kind: RandomKind::Number, length: Some(4) });
    assert_eq!(one("- inputRandomPersonName"), Cmd::InputRandom { kind: RandomKind::PersonName, length: None });
    assert_eq!(one("- inputRandomText"), Cmd::InputRandom { kind: RandomKind::Text, length: None });
}

#[test]
fn assertions_parse() {
    let Cmd::AssertVisible(s) = one("- assertVisible: Welcome") else { panic!() };
    assert_eq!(s.text.as_deref(), Some("Welcome"));
    let Cmd::AssertNotVisible(s) = one("- assertNotVisible:\n    id: spinner") else { panic!() };
    assert_eq!(s.id.as_deref(), Some("spinner"));
    assert_eq!(one("- assertTrue: ${output.ok}"), Cmd::AssertTrue { condition: "${output.ok}".into() });
    let Cmd::ExtendedWaitUntil { visible, not_visible, timeout_ms } =
        one("- extendedWaitUntil:\n    visible: Done\n    timeout: 5000")
    else {
        panic!()
    };
    assert_eq!(visible.unwrap().text.as_deref(), Some("Done"));
    assert!(not_visible.is_none());
    assert_eq!(timeout_ms.as_deref(), Some("5000"));
}

#[test]
fn taps_of_every_kind_parse() {
    let Cmd::TapOn(t) = one("- doubleTapOn: Like") else { panic!() };
    assert_eq!(t.repeat.as_deref(), Some("2"));
    let Cmd::TapOn(t) = one("- longPressOn:\n    id: item") else { panic!() };
    assert!(t.long_press);
    let Cmd::TapOn(t) = one("- tapOn:\n    point: 50%,90%") else { panic!() };
    assert_eq!(t.point.as_deref(), Some("50%,90%"));
    assert_eq!(t.selector, Selector::default());
    let Cmd::TapOn(t) = one("- tapOnPoint:\n    point: 10,20") else { panic!() };
    assert_eq!(t.point.as_deref(), Some("10,20"));
}

#[test]
fn swipes_parse_in_each_form() {
    let Cmd::Swipe(s) = one("- swipe:\n    direction: LEFT\n    duration: 300") else { panic!() };
    assert_eq!(s.direction.as_deref(), Some("LEFT"));
    assert_eq!(s.duration_ms.as_deref(), Some("300"));
    let Cmd::Swipe(s) = one("- swipe:\n    start: 90%, 50%\n    end: 10%, 50%") else { panic!() };
    assert_eq!((s.start.as_deref(), s.end.as_deref()), (Some("90%, 50%"), Some("10%, 50%")));
    let Cmd::Swipe(s) = one("- swipe:\n    from:\n      id: card\n    direction: UP") else { panic!() };
    assert_eq!(s.from.unwrap().id.as_deref(), Some("card"));
}

#[test]
fn scroll_until_visible_parses() {
    let Cmd::ScrollUntilVisible(s) =
        one("- scrollUntilVisible:\n    element:\n      text: Footer\n    direction: DOWN\n    timeout: 9000\n    visibilityPercentage: 80\n    centerElement: true\n    speed: 40")
    else {
        panic!()
    };
    assert_eq!(s.element.text.as_deref(), Some("Footer"));
    assert_eq!(s.direction.as_deref(), Some("DOWN"));
    assert_eq!(s.timeout_ms.as_deref(), Some("9000"));
    assert_eq!(s.visibility_percentage, Some(80));
    assert!(s.center_element);
    let Cmd::ScrollUntilVisible(s) = one("- scrollUntilVisible:\n    element: Footer") else { panic!() };
    assert_eq!(s.element.text.as_deref(), Some("Footer"));
}

#[test]
fn keys_clipboard_and_links_parse() {
    assert_eq!(one("- pressKey: Enter"), Cmd::PressKey { key: "Enter".into() });
    let Cmd::CopyTextFrom(s) = one("- copyTextFrom:\n    id: code") else { panic!() };
    assert_eq!(s.id.as_deref(), Some("code"));
    assert_eq!(one("- setClipboard: abc"), Cmd::SetClipboard { text: "abc".into() });
    assert_eq!(
        one("- openLink: https://example.com"),
        Cmd::OpenLink { link: "https://example.com".into(), browser: false, auto_verify: false }
    );
    assert_eq!(
        one("- openLink:\n    link: app://x\n    browser: true"),
        Cmd::OpenLink { link: "app://x".into(), browser: true, auto_verify: false }
    );
}

#[test]
fn screenshots_and_recordings_parse() {
    assert_eq!(one("- takeScreenshot: home"), Cmd::TakeScreenshot { path: "home".into(), crop_on: None });
    let Cmd::AssertScreenshot { path, threshold, crop_on } =
        one("- assertScreenshot:\n    path: home.png\n    thresholdPercentage: 90\n    cropOn:\n      id: header")
    else {
        panic!()
    };
    assert_eq!((path.as_str(), threshold), ("home.png", Some(90.0)));
    assert_eq!(crop_on.unwrap().id.as_deref(), Some("header"));
    assert_eq!(one("- startRecording: demo"), Cmd::StartRecording { path: "demo".into() });
    assert_eq!(one("- stopRecording"), Cmd::StopRecording);
}

#[test]
fn device_commands_parse() {
    assert_eq!(one("- setOrientation: LANDSCAPE_LEFT"), Cmd::SetOrientation { orientation: "LANDSCAPE_LEFT".into() });
    assert_eq!(
        one("- setLocation:\n    latitude: 52.37\n    longitude: 4.89"),
        Cmd::SetLocation { latitude: "52.37".into(), longitude: "4.89".into() }
    );
    let Cmd::Travel { points, speed } = one("- travel:\n    points:\n      - 0.0,0.0\n      - 0.1,0.1\n    speed: 7") else { panic!() };
    assert_eq!(points, vec!["0.0,0.0".to_string(), "0.1,0.1".to_string()]);
    assert_eq!(speed, Some(7.0));
    assert_eq!(one("- setAirplaneMode: enabled"), Cmd::SetAirplaneMode { enabled: true });
    assert_eq!(one("- setAirplaneMode: disabled"), Cmd::SetAirplaneMode { enabled: false });
    assert_eq!(one("- toggleAirplaneMode"), Cmd::ToggleAirplaneMode);
    let Cmd::SetPermissions { app_id, permissions } = one("- setPermissions:\n    permissions:\n      camera: allow") else { panic!() };
    assert!(app_id.is_none());
    assert_eq!(permissions, vec![("camera".to_string(), "allow".to_string())]);
    assert_eq!(one("- addMedia:\n    - a.png\n    - b.mp4"), Cmd::AddMedia { files: vec!["a.png".into(), "b.mp4".into()] });
    assert_eq!(one("- setDarkMode: true"), Cmd::SetDarkMode { enabled: true });
    assert_eq!(one("- toggleDarkMode"), Cmd::ToggleDarkMode);
    assert_eq!(one("- assertDarkMode"), Cmd::AssertDarkMode { dark: true });
    assert_eq!(one("- assertLightMode"), Cmd::AssertDarkMode { dark: false });
}

#[test]
fn run_flow_takes_a_file_or_inline_commands_with_env_and_when() {
    let Cmd::RunFlow(r) = one("- runFlow: login.yaml") else { panic!() };
    assert_eq!(r.file.as_deref(), Some("login.yaml"));
    let Cmd::RunFlow(r) = one(
        "- runFlow:\n    when:\n      visible: Accept cookies\n      platform: Android\n    env:\n      USER: bob\n    commands:\n      - tapOn: Accept",
    ) else {
        panic!()
    };
    assert!(r.file.is_none());
    assert_eq!(r.commands.len(), 1);
    assert_eq!(r.env, vec![("USER".to_string(), "bob".to_string())]);
    let when = r.when.unwrap();
    assert_eq!(when.visible.unwrap().text.as_deref(), Some("Accept cookies"));
    assert_eq!(when.platform.as_deref(), Some("Android"));
}

#[test]
fn repeat_and_retry_parse() {
    let Cmd::Repeat { times, while_, commands } = one("- repeat:\n    times: 3\n    commands:\n      - back") else { panic!() };
    assert_eq!(times.as_deref(), Some("3"));
    assert!(while_.is_none());
    assert_eq!(commands.len(), 1);
    let Cmd::Repeat { while_, .. } = one("- repeat:\n    while:\n      notVisible: End\n    commands:\n      - scroll") else { panic!() };
    assert_eq!(while_.unwrap().not_visible.unwrap().text.as_deref(), Some("End"));
    let Cmd::Retry(r) = one("- retry:\n    maxRetries: 2\n    commands:\n      - tapOn: Go") else { panic!() };
    assert_eq!(r.max_retries.as_deref(), Some("2"));
    assert_eq!(r.commands.len(), 1);
}

#[test]
fn scripts_parse() {
    assert_eq!(one("- evalScript: ${output.n = 1}"), Cmd::EvalScript { script: "${output.n = 1}".into() });
    let Cmd::RunScript { file, env, when } = one("- runScript:\n    file: setup.js\n    env:\n      A: 1") else { panic!() };
    assert_eq!(file, "setup.js");
    assert_eq!(env, vec![("A".to_string(), "1".to_string())]);
    assert!(when.is_none());
    let Cmd::RunScript { file, .. } = one("- runScript: setup.js") else { panic!() };
    assert_eq!(file, "setup.js");
}

#[test]
fn the_header_reads_name_tags_env_hooks_and_properties() {
    let flow = parse_flow(
        "appId: com.x\nname: Login\ntags:\n  - smoke\n  - auth\nenv:\n  USER: alice\n  PASS: ${SECRET}\nonFlowStart:\n  - runScript: before.js\nonFlowComplete:\n  - stopApp\nproperties:\n  junitId: T-1\n---\n- launchApp\n",
        None,
    )
    .unwrap();
    let c = flow.config;
    assert_eq!(c.name.as_deref(), Some("Login"));
    assert_eq!(c.tags, vec!["smoke", "auth"]);
    assert_eq!(c.env, vec![("USER".into(), "alice".into()), ("PASS".into(), "${SECRET}".into())]);
    assert_eq!(c.on_flow_start.len(), 1);
    assert_eq!(c.on_flow_complete.len(), 1);
    assert_eq!(c.properties.get("junitId").map(String::as_str), Some("T-1"));
}

#[test]
fn errors_name_the_file_and_the_command() {
    let err = parse_flow("appId: a\n---\n- tapOn:\n    txt: x\n", Some(std::path::Path::new("flows/a.yaml"))).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("a.yaml") && msg.contains("tapOn") && msg.contains("txt"), "{msg}");
}

#[test]
fn windows_line_endings_parse() {
    assert_eq!(cmds("appId: a\r\n---\r\n- back\r\n"), vec![Cmd::Back]);
}

#[test]
fn errors_show_the_offending_value_on_one_line() {
    let err = |yaml: &str| parse_flow(yaml, None).unwrap_err().to_string();
    let e = err("appId: a\n---\n- 42\n");
    assert!(e.contains("not a command: 42"), "{e}");
    let e = err("appId: a\n---\n- tapOn:\n    text: [a, b]\n");
    assert!(e.contains("must be a single value, not [a, b]"), "{e}");
    let e = err("appId: a\n---\n- tapOn:\n    enabled: {x: 1}\n");
    assert!(e.contains("must be true or false, not {x: 1}"), "{e}");
}
