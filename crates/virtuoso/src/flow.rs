//! Maestro flow files: a header document (appId, name, tags, env, hooks), `---`, then a list of
//! commands. Values keep their `${...}` expressions; the runner expands them when a command runs.
//!
//! Parsing is strict, like Maestro's: an unknown command or field is an error naming it, so a
//! typo never turns into a step that silently does nothing.

use serde_yaml::{Mapping, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
#[error("{}{msg}", path.as_ref().map(|p| format!("{}: ", p.display())).unwrap_or_default())]
pub struct FlowError {
    pub path: Option<PathBuf>,
    pub msg: String,
}

type R<T> = Result<T, String>;

#[derive(Debug, Clone, Default)]
pub struct Flow {
    pub path: Option<PathBuf>,
    pub config: FlowConfig,
    pub commands: Vec<Step>,
}

#[derive(Debug, Clone, Default)]
pub struct FlowConfig {
    pub app_id: Option<String>,
    pub name: Option<String>,
    pub tags: Vec<String>,
    /// In file order: later values may refer to earlier ones.
    pub env: Vec<(String, String)>,
    pub on_flow_start: Vec<Step>,
    pub on_flow_complete: Vec<Step>,
    pub properties: BTreeMap<String, String>,
}

/// One command of a flow with the fields every command takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub cmd: Cmd,
    pub label: Option<String>,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selector {
    pub text: Option<String>,
    pub id: Option<String>,
    pub index: Option<String>,
    pub enabled: Option<bool>,
    pub checked: Option<bool>,
    pub focused: Option<bool>,
    pub selected: Option<bool>,
    pub below: Option<Box<Selector>>,
    pub above: Option<Box<Selector>>,
    pub left_of: Option<Box<Selector>>,
    pub right_of: Option<Box<Selector>>,
    pub child_of: Option<Box<Selector>>,
    pub contains_child: Option<Box<Selector>>,
    pub contains_descendants: Vec<Selector>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub tolerance: Option<i32>,
    pub traits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TapOn {
    pub selector: Selector,
    /// "x,y" in pixels or "x%,y%"; with a selector, relative to the element.
    pub point: Option<String>,
    pub repeat: Option<String>,
    pub delay_ms: Option<String>,
    pub retry_if_no_change: Option<bool>,
    pub wait_until_visible: Option<bool>,
    pub wait_to_settle_ms: Option<String>,
    pub long_press: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaunchApp {
    pub app_id: Option<String>,
    pub clear_state: bool,
    pub clear_keychain: bool,
    pub stop_app: Option<bool>,
    pub permissions: Vec<(String, String)>,
    pub arguments: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Swipe {
    pub direction: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub from: Option<Selector>,
    pub duration_ms: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScrollUntilVisible {
    pub element: Selector,
    pub direction: Option<String>,
    pub timeout_ms: Option<String>,
    pub visibility_percentage: Option<i32>,
    pub center_element: bool,
    pub speed: Option<i32>,
    pub wait_to_settle_ms: Option<String>,
}

/// `when:` of runFlow / runScript, and `while:` of repeat. Every given part must hold.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Condition {
    pub visible: Option<Selector>,
    pub not_visible: Option<Selector>,
    pub true_: Option<String>,
    pub platform: Option<String>,
}

/// One end of a drag: an element, or a point ("x,y" or "x%,y%" of the screen).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Anchor {
    pub selector: Option<Selector>,
    pub point: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunFlow {
    pub file: Option<String>,
    pub commands: Vec<Step>,
    pub env: Vec<(String, String)>,
    pub when: Option<Condition>,
    /// What runs when `when` is false (maestro-runner's `else` / `elseCommands`).
    pub else_file: Option<String>,
    pub else_commands: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Retry {
    pub max_retries: Option<String>,
    pub file: Option<String>,
    pub commands: Vec<Step>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomKind {
    Email,
    Number,
    PersonName,
    Text,
    CityName,
    CountryName,
    ColorName,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    LaunchApp(LaunchApp),
    TapOn(TapOn),
    Back,
    HideKeyboard,
    WaitForAnimationToEnd { timeout_ms: Option<String> },
    StopApp { app_id: Option<String> },
    KillApp { app_id: Option<String> },
    ClearState { app_id: Option<String> },
    ClearKeychain,
    PasteText,
    Scroll,
    InputText { text: String },
    EraseText { chars: Option<String> },
    InputRandom { kind: RandomKind, length: Option<i32> },
    AssertVisible(Selector),
    AssertNotVisible(Selector),
    AssertTrue { condition: String },
    ExtendedWaitUntil { visible: Option<Selector>, not_visible: Option<Selector>, timeout_ms: Option<String> },
    Swipe(Swipe),
    ScrollUntilVisible(ScrollUntilVisible),
    PressKey { key: String },
    CopyTextFrom(Selector),
    SetClipboard { text: String },
    OpenLink { link: String, browser: bool, auto_verify: bool },
    TakeScreenshot { path: String, crop_on: Option<Selector> },
    AssertScreenshot { path: String, threshold: Option<f64>, crop_on: Option<Selector> },
    StartRecording { path: String },
    StopRecording,
    SetOrientation { orientation: String },
    SetLocation { latitude: String, longitude: String },
    Travel { points: Vec<String>, speed: Option<f64> },
    SetAirplaneMode { enabled: bool },
    ToggleAirplaneMode,
    SetPermissions { app_id: Option<String>, permissions: Vec<(String, String)> },
    AddMedia { files: Vec<String> },
    SetDarkMode { enabled: bool },
    ToggleDarkMode,
    AssertDarkMode { dark: bool },
    RunFlow(RunFlow),
    Repeat { times: Option<String>, while_: Option<Condition>, commands: Vec<Step> },
    Retry(Retry),
    RunScript { file: String, env: Vec<(String, String)>, when: Option<Condition> },
    EvalScript { script: String },
    /// maestro-runner's fixed pause.
    Wait { ms: String },
    /// maestro-runner's host command: `sh -c` (`cmd /C` on Windows), stdout kept in `output`.
    RunShell { command: String, output: Option<String>, env: Vec<(String, String)>, timeout_ms: Option<String> },
    OpenNotifications,
    /// maestro-runner's drag: press and hold `from`, move to `to`, release.
    DragAndDrop { from: Anchor, to: Anchor, hold_ms: Option<String>, move_ms: Option<String> },
    /// Taps a system dialog's accepting (`accept`) or refusing button.
    Alert { accept: bool },
}

/// Reads a flow file's text. `path` locates files the flow refers to and names it in errors.
pub fn parse_flow(text: &str, path: Option<&Path>) -> Result<Flow, FlowError> {
    let err = |msg: String| FlowError { path: path.map(Path::to_path_buf), msg };
    let text = text.replace("\r\n", "\n");
    let mut docs = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(&text) {
        let v: Value = serde::Deserialize::deserialize(doc).map_err(|e| err(format!("invalid YAML: {e}")))?;
        docs.push(v);
    }
    let (header, body) = match docs.len() {
        2 => (docs.remove(0), docs.remove(0)),
        1 if docs[0].is_sequence() => {
            return Err(err("the flow needs a header (`appId: ...`) and `---` before its commands".into()))
        }
        1 => (docs.remove(0), Value::Sequence(vec![])),
        0 => return Err(err("the flow file is empty".into())),
        n => return Err(err(format!("a flow has a header and a command list; this file has {n} documents"))),
    };
    let config = parse_config(&header).map_err(err)?;
    let commands = parse_steps(&body).map_err(err)?;
    Ok(Flow { path: path.map(Path::to_path_buf), config, commands })
}

fn parse_config(v: &Value) -> R<FlowConfig> {
    let map = v.as_mapping().ok_or("the flow header must be a map (`appId: ...`)")?;
    let f = Fields::new("the flow header", map);
    let mut c = FlowConfig {
        app_id: f.str("appId")?.or(f.str("url")?),
        name: f.str("name")?,
        tags: f.str_list("tags")?,
        ..Default::default()
    };
    if let Some(env) = f.get("env") {
        let env = env.as_mapping().ok_or("`env` must be a map")?;
        for (k, v) in env {
            c.env.push((scalar(k).ok_or("an env name must be text")?, scalar(v).unwrap_or_default()));
        }
    }
    if let Some(v) = f.get("onFlowStart") {
        c.on_flow_start = parse_steps(v)?;
    }
    if let Some(v) = f.get("onFlowComplete") {
        c.on_flow_complete = parse_steps(v)?;
    }
    if let Some(p) = f.get("properties") {
        for (k, v) in p.as_mapping().ok_or("`properties` must be a map")? {
            c.properties.insert(scalar(k).unwrap_or_default(), scalar(v).unwrap_or_default());
        }
    }
    // Maestro accepts these and so do we; they change nothing on a local run.
    for ignored in ["jsEngine", "androidWebViewHierarchy", "extendedWaitUntil"] {
        f.get(ignored);
    }
    f.finish()?;
    Ok(c)
}

pub fn parse_steps(v: &Value) -> R<Vec<Step>> {
    match v {
        Value::Null => Ok(vec![]),
        Value::Sequence(items) => items.iter().map(parse_step).collect(),
        _ => Err("the commands must be a list (`- launchApp`)".into()),
    }
}

/// A command is a bare name (`- back`) or a one-key map (`- tapOn: ...`).
fn parse_step(v: &Value) -> R<Step> {
    let (name, arg) = match v {
        Value::String(s) => (s.as_str(), &Value::Null),
        Value::Mapping(m) if m.len() == 1 => {
            let (k, v) = m.iter().next().unwrap();
            (k.as_str().ok_or("a command name must be text")?, v)
        }
        Value::Mapping(m) => {
            let names: Vec<String> = m.keys().filter_map(scalar).collect();
            return Err(format!("one command per list item; found {}", names.join(", ")));
        }
        _ => return Err(format!("not a command: {}", show(v))),
    };
    let mut label = None;
    let mut optional = false;
    let cmd = command(name, arg, &mut |f: &Fields| {
        label = f.str("label")?;
        optional = f.bool("optional")?.unwrap_or(false);
        Ok(())
    })
    .map_err(|e| format!("{name}: {e}"))?;
    Ok(Step { cmd, label, optional })
}

/// `common` reads the fields every command accepts from a command's map.
fn command(name: &str, arg: &Value, common: &mut dyn FnMut(&Fields) -> R<()>) -> R<Cmd> {
    let map = arg.as_mapping();
    let empty = Mapping::new();
    let f = Fields::new(name, map.unwrap_or(&empty));
    if map.is_some() {
        common(&f)?;
    }
    let cmd = match name {
        "launchApp" => Cmd::LaunchApp(launch_app(arg, &f)?),
        "tapOn" => Cmd::TapOn(tap_on(arg, &f, false)?),
        "doubleTapOn" => {
            let mut t = tap_on(arg, &f, false)?;
            t.repeat.get_or_insert_with(|| "2".into());
            Cmd::TapOn(t)
        }
        "longPressOn" => Cmd::TapOn(tap_on(arg, &f, true)?),
        "tapOnPoint" => Cmd::TapOn(TapOn { point: Some(text_arg(arg, &f, "point")?), ..Default::default() }),
        "action" => {
            let inner = scalar(arg).ok_or("`action` takes a command name")?;
            return command(&inner, &Value::Null, common);
        }
        "back" => Cmd::Back,
        "hideKeyboard" => Cmd::HideKeyboard,
        "clearKeychain" => Cmd::ClearKeychain,
        "pasteText" => Cmd::PasteText,
        "scroll" => Cmd::Scroll,
        "waitForAnimationToEnd" => Cmd::WaitForAnimationToEnd { timeout_ms: f.str("timeout")? },
        "stopApp" => Cmd::StopApp { app_id: opt_text_arg(arg, &f, "appId")? },
        "killApp" => Cmd::KillApp { app_id: opt_text_arg(arg, &f, "appId")? },
        "clearState" => Cmd::ClearState { app_id: opt_text_arg(arg, &f, "appId")? },
        "inputText" => Cmd::InputText { text: text_arg(arg, &f, "text")? },
        "eraseText" => Cmd::EraseText { chars: opt_text_arg(arg, &f, "charactersToErase")? },
        "inputRandomEmail" | "inputRandomNumber" | "inputRandomPersonName" | "inputRandomText"
        | "inputRandomCityName" | "inputRandomCountryName" | "inputRandomColorName" => {
            let kind = match name {
                "inputRandomEmail" => RandomKind::Email,
                "inputRandomNumber" => RandomKind::Number,
                "inputRandomPersonName" => RandomKind::PersonName,
                "inputRandomText" => RandomKind::Text,
                "inputRandomCityName" => RandomKind::CityName,
                "inputRandomCountryName" => RandomKind::CountryName,
                _ => RandomKind::ColorName,
            };
            Cmd::InputRandom { kind, length: f.int("length")? }
        }
        "assertVisible" => Cmd::AssertVisible(selector_arg(arg, &f)?),
        "assertNotVisible" => Cmd::AssertNotVisible(selector_arg(arg, &f)?),
        "assertTrue" => Cmd::AssertTrue { condition: text_arg(arg, &f, "condition")? },
        "swipe" => Cmd::Swipe(Swipe {
            direction: f.str("direction")?,
            start: f.str("start")?,
            end: f.str("end")?,
            from: f.get("from").map(parse_selector).transpose()?,
            duration_ms: f.str("duration")?,
        }),
        "scrollUntilVisible" => Cmd::ScrollUntilVisible(ScrollUntilVisible {
            element: parse_selector(f.get("element").ok_or("needs `element:`")?)?,
            direction: f.str("direction")?,
            timeout_ms: f.str("timeout")?,
            visibility_percentage: f.int("visibilityPercentage")?,
            center_element: f.bool("centerElement")?.unwrap_or(false),
            speed: f.int("speed")?,
            wait_to_settle_ms: f.str("waitToSettleTimeoutMs")?,
        }),
        "pressKey" => Cmd::PressKey { key: text_arg(arg, &f, "key")? },
        "copyTextFrom" => Cmd::CopyTextFrom(selector_arg(arg, &f)?),
        "setClipboard" => Cmd::SetClipboard { text: text_arg(arg, &f, "text")? },
        "openLink" => Cmd::OpenLink {
            link: text_arg(arg, &f, "link")?,
            browser: f.bool("browser")?.unwrap_or(false),
            auto_verify: f.bool("autoVerify")?.unwrap_or(false),
        },
        "takeScreenshot" => Cmd::TakeScreenshot {
            path: text_arg(arg, &f, "path")?,
            crop_on: f.get("cropOn").map(parse_selector).transpose()?,
        },
        "assertScreenshot" => Cmd::AssertScreenshot {
            path: text_arg(arg, &f, "path")?,
            threshold: f.str("thresholdPercentage")?.map(|t| t.parse().map_err(|_| "`thresholdPercentage` must be a number")).transpose()?,
            crop_on: f.get("cropOn").map(parse_selector).transpose()?,
        },
        "startRecording" => Cmd::StartRecording { path: text_arg(arg, &f, "path")? },
        "stopRecording" => Cmd::StopRecording,
        "setOrientation" => Cmd::SetOrientation { orientation: text_arg(arg, &f, "orientation")? },
        "setLocation" => Cmd::SetLocation {
            latitude: f.str("latitude")?.ok_or("needs `latitude`")?,
            longitude: f.str("longitude")?.ok_or("needs `longitude`")?,
        },
        "travel" => Cmd::Travel {
            points: f.str_list("points")?,
            speed: f.str("speed")?.map(|t| t.parse().map_err(|_| "`speed` must be a number")).transpose()?,
        },
        "setAirplaneMode" => Cmd::SetAirplaneMode { enabled: on_off(&text_arg(arg, &f, "value")?)? },
        "toggleAirplaneMode" => Cmd::ToggleAirplaneMode,
        "setPermissions" => {
            let mut permissions = Vec::new();
            if let Some(p) = f.get("permissions") {
                for (k, v) in p.as_mapping().ok_or("`permissions` must be a map")? {
                    permissions.push((scalar(k).unwrap_or_default(), scalar(v).unwrap_or_default()));
                }
            }
            Cmd::SetPermissions { app_id: f.str("appId")?, permissions }
        }
        "addMedia" => Cmd::AddMedia {
            files: match arg {
                Value::Sequence(s) => s.iter().filter_map(scalar).collect(),
                Value::Mapping(_) => f.str_list("files")?,
                v => scalar(v).into_iter().collect(),
            },
        },
        "setDarkMode" => Cmd::SetDarkMode { enabled: on_off(&text_arg(arg, &f, "value")?)? },
        "toggleDarkMode" => Cmd::ToggleDarkMode,
        "assertDarkMode" => Cmd::AssertDarkMode { dark: true },
        "assertLightMode" => Cmd::AssertDarkMode { dark: false },
        "runFlow" => Cmd::RunFlow(match scalar(arg) {
            Some(file) => RunFlow { file: Some(file), ..Default::default() },
            None => RunFlow {
                file: f.str("file")?,
                commands: f.get("commands").map(parse_steps).transpose()?.unwrap_or_default(),
                env: env_map(f.get("env"))?,
                when: f.get("when").map(parse_condition).transpose()?,
                else_file: f.get("else").and_then(scalar),
                else_commands: match (f.get("else"), f.get("elseCommands")) {
                    (Some(v @ Value::Sequence(_)), _) | (_, Some(v)) => parse_steps(v)?,
                    (Some(v), None) if scalar(v).is_none() => return Err("`else` must be a flow file or a list of commands".into()),
                    _ => vec![],
                },
            },
        }),
        "repeat" => Cmd::Repeat {
            times: f.str("times")?,
            while_: f.get("while").map(parse_condition).transpose()?,
            commands: parse_steps(f.get("commands").ok_or("needs `commands:`")?)?,
        },
        "retry" => Cmd::Retry(Retry {
            max_retries: f.str("maxRetries")?,
            file: f.str("file")?,
            commands: f.get("commands").map(parse_steps).transpose()?.unwrap_or_default(),
            env: env_map(f.get("env"))?,
        }),
        "runScript" => Cmd::RunScript {
            file: text_arg(arg, &f, "file")?,
            env: env_map(f.get("env"))?,
            when: f.get("when").map(parse_condition).transpose()?,
        },
        "evalScript" => Cmd::EvalScript { script: text_arg(arg, &f, "script")? },
        "openNotifications" => Cmd::OpenNotifications,
        "dragAndDrop" => Cmd::DragAndDrop {
            from: parse_anchor(f.get("from").ok_or("needs `from:` (a selector or a point)")?)?,
            to: parse_anchor(f.get("to").ok_or("needs `to:` (a selector or a point)")?)?,
            hold_ms: f.str("holdDuration")?,
            move_ms: f.str("duration")?,
        },
        "acceptAlert" => Cmd::Alert { accept: true },
        "dismissAlert" => Cmd::Alert { accept: false },
        "wait" => Cmd::Wait { ms: text_arg(arg, &f, "duration")? },
        "runShell" => Cmd::RunShell {
            command: text_arg(arg, &f, "command")?,
            output: f.str("output")?,
            env: env_map(f.get("env"))?,
            timeout_ms: f.str("timeout")?,
        },
        "extendedWaitUntil" => Cmd::ExtendedWaitUntil {
            visible: f.get("visible").map(parse_selector).transpose()?,
            not_visible: f.get("notVisible").map(parse_selector).transpose()?,
            timeout_ms: f.str("timeout")?,
        },
        _ => return Err(format!("`{name}` is not a command virtuoso knows")),
    };
    f.finish()?;
    Ok(cmd)
}

fn env_map(v: Option<&Value>) -> R<Vec<(String, String)>> {
    let Some(v) = v else { return Ok(vec![]) };
    let m = v.as_mapping().ok_or("`env` must be a map")?;
    Ok(m.iter().map(|(k, v)| (scalar(k).unwrap_or_default(), scalar(v).unwrap_or_default())).collect())
}

pub fn parse_condition(v: &Value) -> R<Condition> {
    let map = v.as_mapping().ok_or("a condition is a map (`visible:`, `notVisible:`, `true:`, `platform:`)")?;
    let f = Fields::new("the condition", map);
    let c = Condition {
        visible: f.get("visible").map(parse_selector).transpose()?,
        not_visible: f.get("notVisible").map(parse_selector).transpose()?,
        true_: f.str("true")?,
        platform: f.str("platform")?,
    };
    f.finish()?;
    Ok(c)
}

fn on_off(v: &str) -> R<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "enabled" | "enable" | "on" => Ok(true),
        "false" | "disabled" | "disable" | "off" => Ok(false),
        other => Err(format!("`{other}` is not enabled or disabled")),
    }
}

/// The command's value as text, or its `key` field when given as a map.
fn text_arg(arg: &Value, f: &Fields, key: &str) -> R<String> {
    opt_text_arg(arg, f, key)?.ok_or_else(|| format!("needs a value (or `{key}:`)"))
}

fn opt_text_arg(arg: &Value, f: &Fields, key: &str) -> R<Option<String>> {
    match scalar(arg) {
        Some(s) => Ok(Some(s)),
        None => f.str(key),
    }
}

/// A selector as the command's value: text, or the fields of the command's own map.
fn selector_arg(arg: &Value, f: &Fields) -> R<Selector> {
    match scalar(arg) {
        Some(text) => Ok(Selector { text: Some(text), ..Default::default() }),
        None => selector_fields(f),
    }
}

fn launch_app(arg: &Value, f: &Fields) -> R<LaunchApp> {
    if let Some(app) = scalar(arg) {
        return Ok(LaunchApp { app_id: Some(app), ..Default::default() });
    }
    let mut l = LaunchApp {
        app_id: f.str("appId")?,
        clear_state: f.bool("clearState")?.unwrap_or(false),
        clear_keychain: f.bool("clearKeychain")?.unwrap_or(false),
        stop_app: f.bool("stopApp")?,
        ..Default::default()
    };
    if let Some(p) = f.get("permissions") {
        for (k, v) in p.as_mapping().ok_or("`permissions` must be a map")? {
            l.permissions.push((scalar(k).unwrap_or_default(), scalar(v).unwrap_or_default()));
        }
    }
    if let Some(a) = f.get("arguments") {
        for (k, v) in a.as_mapping().ok_or("`arguments` must be a map")? {
            l.arguments.push((scalar(k).unwrap_or_default(), v.clone()));
        }
    }
    Ok(l)
}

fn tap_on(arg: &Value, f: &Fields, long_press: bool) -> R<TapOn> {
    if let Some(text) = scalar(arg) {
        return Ok(TapOn { selector: Selector { text: Some(text), ..Default::default() }, long_press, ..Default::default() });
    }
    Ok(TapOn {
        point: f.str("point")?,
        repeat: f.str("repeat")?,
        delay_ms: f.str("delay")?,
        retry_if_no_change: f.bool("retryTapIfNoChange")?,
        wait_until_visible: f.bool("waitUntilVisible")?,
        wait_to_settle_ms: f.str("waitToSettleTimeoutMs")?,
        long_press: long_press || f.bool("longPress")?.unwrap_or(false),
        selector: selector_fields(f)?,
    })
}

/// A selector given as text (`"Sign in"`) or a map of fields.
fn parse_anchor(v: &Value) -> R<Anchor> {
    if let Some(Value::Mapping(m)) = Some(v) {
        if m.len() == 1 {
            if let Some(p) = m.get("point") {
                return Ok(Anchor { point: Some(scalar(p).ok_or("`point` must be x,y")?), selector: None });
            }
        }
    }
    Ok(Anchor { selector: Some(parse_selector(v)?), point: None })
}

pub fn parse_selector(v: &Value) -> R<Selector> {
    if let Some(text) = scalar(v) {
        return Ok(Selector { text: Some(text), ..Default::default() });
    }
    let map = v.as_mapping().ok_or_else(|| format!("not a selector: {}", show(v)))?;
    let f = Fields::new("selector", map);
    let s = selector_fields(&f)?;
    f.finish()?;
    Ok(s)
}

fn selector_fields(f: &Fields) -> R<Selector> {
    let sub = |k: &str| -> R<Option<Box<Selector>>> { f.get(k).map(|v| parse_selector(v).map(Box::new)).transpose() };
    let mut s = Selector {
        text: f.str("text")?,
        id: f.str("id")?,
        index: f.str("index")?,
        enabled: f.bool("enabled")?,
        checked: f.bool("checked")?,
        focused: f.bool("focused")?,
        selected: f.bool("selected")?,
        below: sub("below")?,
        above: sub("above")?,
        left_of: sub("leftOf")?,
        right_of: sub("rightOf")?,
        child_of: sub("childOf")?,
        contains_child: sub("containsChild")?,
        width: f.int("width")?,
        height: f.int("height")?,
        tolerance: f.int("tolerance")?,
        traits: f.str_list("traits")?,
        contains_descendants: vec![],
    };
    if let Some(v) = f.get("containsDescendants") {
        let items = v.as_sequence().ok_or("`containsDescendants` must be a list of selectors")?;
        s.contains_descendants = items.iter().map(parse_selector).collect::<R<_>>()?;
    }
    Ok(s)
}

/// A command's map, remembering which fields were read so the rest can be reported as unknown.
pub(crate) struct Fields<'a> {
    what: &'a str,
    map: &'a Mapping,
    used: RefCell<HashSet<String>>,
}

impl<'a> Fields<'a> {
    pub(crate) fn new(what: &'a str, map: &'a Mapping) -> Self {
        Fields { what, map, used: RefCell::new(HashSet::new()) }
    }

    pub(crate) fn get(&self, k: &str) -> Option<&'a Value> {
        self.used.borrow_mut().insert(k.to_string());
        self.map.get(k).filter(|v| !v.is_null())
    }

    pub(crate) fn str(&self, k: &str) -> R<Option<String>> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => scalar(v).map(Some).ok_or_else(|| format!("`{k}` must be a single value, not {}", show(v))),
        }
    }

    pub(crate) fn bool(&self, k: &str) -> R<Option<bool>> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(Value::String(s)) if s == "true" || s == "false" => Ok(Some(s == "true")),
            Some(v) => Err(format!("`{k}` must be true or false, not {}", show(v))),
        }
    }

    pub(crate) fn int(&self, k: &str) -> R<Option<i32>> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => scalar(v)
                .and_then(|s| s.trim().parse().ok())
                .map(Some)
                .ok_or_else(|| format!("`{k}` must be a whole number, not {}", show(v))),
        }
    }

    pub(crate) fn str_list(&self, k: &str) -> R<Vec<String>> {
        match self.get(k) {
            None => Ok(vec![]),
            Some(Value::Sequence(s)) => Ok(s.iter().filter_map(scalar).collect()),
            Some(v) => scalar(v).map(|s| vec![s]).ok_or_else(|| format!("`{k}` must be a list")),
        }
    }

    pub(crate) fn finish(&self) -> R<()> {
        let used = self.used.borrow();
        let unknown: Vec<String> = self.map.keys().filter_map(scalar).filter(|k| !used.contains(k)).collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!("unknown field{} {} in {}", if unknown.len() > 1 { "s" } else { "" }, unknown.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", "), self.what))
        }
    }
}

/// Text, numbers and booleans read as text: `inputText: 1234` types "1234".
pub(crate) fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn show(v: &Value) -> String {
    serde_yaml::to_string(v).unwrap_or_default().trim().to_string()
}
