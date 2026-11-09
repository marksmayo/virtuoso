//! Runs a flow's commands on a device.

use crate::device::{Device, Key, LaunchOptions, Lift, Orientation};
use crate::flow::{Cmd, Flow, Selector, Step, TapOn};
use crate::js::Js;
use crate::screen::{Rect, ScreenTree};
use crate::select::select;
use anyhow::{anyhow, bail, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// How long a command waits for its element (Maestro: 17 s).
    pub lookup_timeout: Duration,
    /// The same for optional commands (Maestro: 7 s).
    pub optional_lookup_timeout: Duration,
    /// How long `when: visible:` waits before it is false.
    pub condition_timeout: Duration,
    pub settle_timeout: Duration,
    /// `-e` variables; a flow's own `env:` wins over them, as in Maestro.
    pub env: Vec<(String, String)>,
    /// Where takeScreenshot and recordings go.
    pub output_dir: Option<PathBuf>,
    /// Record each flow to `<flow name>.mp4` in the output folder.
    pub video: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            lookup_timeout: Duration::from_secs(17),
            optional_lookup_timeout: Duration::from_secs(7),
            condition_timeout: Duration::from_secs(7),
            settle_timeout: Duration::from_secs(10),
            env: vec![],
            output_dir: None,
            video: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Passed,
    Failed,
    /// An optional command that failed.
    Warned,
    /// A `when:` that did not hold.
    Skipped,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub name: String,
    pub depth: usize,
    pub status: StepStatus,
    pub duration: Duration,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FlowResult {
    pub name: String,
    pub status: FlowStatus,
    pub steps: Vec<StepResult>,
    pub duration: Duration,
    pub error: Option<String>,
}

/// Why a step did not pass: a failure, or a condition that skipped it.
enum Outcome {
    Done,
    Skipped,
}

pub type StepCallback<'a> = Box<dyn FnMut(&StepResult) + 'a>;

pub struct Runner<'a> {
    dev: &'a mut dyn Device,
    opts: RunOptions,
    js: Js,
    app_id: Option<String>,
    flow_dir: PathBuf,
    steps: Vec<StepResult>,
    depth: usize,
    copied: Option<String>,
    /// Variables' values (with their names), kept out of step names and errors.
    secrets: Vec<(String, String)>,
    /// Called with each finished step, for live output.
    pub on_step: Option<StepCallback<'a>>,
    /// When the last command that may have changed the app ended: `when: visible:` waits
    /// until the condition timeout has passed since then, so checks for popups that never
    /// show share one wait (as in speedrun) instead of taking the whole timeout each.
    last_change: Instant,
}

impl<'a> Runner<'a> {
    pub fn new(dev: &'a mut dyn Device, opts: RunOptions) -> Self {
        let js = Js::new(dev.platform());
        Runner { dev, opts, js, app_id: None, flow_dir: PathBuf::from("."), steps: vec![], depth: 0, copied: None, secrets: vec![], on_step: None, last_change: Instant::now() }
    }

    pub fn run_flow(&mut self, flow: &Flow) -> FlowResult {
        let start = Instant::now();
        let name = flow_name(flow);
        self.app_id = flow.config.app_id.clone();
        if let Some(dir) = flow.path.as_ref().and_then(|p| p.parent()) {
            self.flow_dir = dir.to_path_buf();
        }
        self.js.set_module_dirs(vec![self.flow_dir.clone()]);
        let mut error = self.setup_env(flow).err().map(|e| e.to_string());
        let recording = error.is_none() && self.opts.video;
        if recording {
            let started = self.output_file(&name, "mp4").and_then(|p| self.dev.start_recording(&p));
            error = started.err().map(|e| format!("could not record a video: {e}"));
        }
        if error.is_none() {
            error = self.run_steps(&flow.config.on_flow_start).err().map(|e| e.to_string());
        }
        if error.is_none() {
            error = self.run_steps(&flow.commands).err().map(|e| e.to_string());
        }
        // onFlowComplete runs whatever happened before, as in Maestro.
        let after = self.run_steps(&flow.config.on_flow_complete).err().map(|e| e.to_string());
        if recording {
            // A video that cannot be saved does not fail a flow that passed.
            let _ = self.dev.stop_recording();
        }
        let error = error.or(after).map(|e| self.mask(&e));
        FlowResult {
            name,
            status: if error.is_some() { FlowStatus::Failed } else { FlowStatus::Passed },
            steps: std::mem::take(&mut self.steps),
            duration: start.elapsed(),
            error,
        }
    }

    fn setup_env(&mut self, flow: &Flow) -> Result<()> {
        for (k, v) in self.opts.env.clone() {
            self.js.set_var(&k, &v);
            self.keep_secret(&k, &v);
        }
        if let Some(path) = &flow.path {
            self.js.set_var("MAESTRO_FILENAME", &path.file_stem().unwrap_or_default().to_string_lossy());
        }
        self.js.set_var("MAESTRO_DEVICE_UDID", &self.dev.id());
        for (k, v) in &flow.config.env {
            let v = self.js.expand(v)?;
            self.js.set_var(k, &v);
            self.keep_secret(k, &v);
        }
        Ok(())
    }

    /// A variable's value is masked wherever the run reports text. Very short values are
    /// left alone: masking "1" or "on" would garble every message.
    fn keep_secret(&mut self, name: &str, value: &str) {
        if value.chars().count() >= 4 && !self.secrets.iter().any(|(_, v)| v == value) {
            self.secrets.push((name.to_string(), value.to_string()));
            // Longest first, so a value inside another is not masked first.
            crate::sort::sort_small_by_key(&mut self.secrets, |(_, v)| std::cmp::Reverse(v.len()));
        }
    }

    /// `text` with every variable's value shown as its reference, `${NAME}`.
    fn mask(&self, text: &str) -> String {
        if self.secrets.is_empty() {
            return text.to_string();
        }
        self.secrets.iter().fold(text.to_string(), |t, (k, v)| t.replace(v.as_str(), &format!("${{{k}}}")))
    }

    /// Runs steps until one fails; that failure is returned.
    fn run_steps(&mut self, steps: &[Step]) -> Result<()> {
        for step in steps {
            let start = Instant::now();
            let name = self.mask(&step.label.clone().unwrap_or_else(|| describe(&step.cmd)));
            let at = self.steps.len();
            self.depth += 1;
            let depth = self.depth;
            let outcome = self.exec(&step.cmd, step.optional);
            if may_change_the_app(&step.cmd) {
                self.last_change = Instant::now();
            }
            self.depth -= 1;
            let (status, error) = match &outcome {
                Ok(Outcome::Done) => (StepStatus::Passed, None),
                Ok(Outcome::Skipped) => (StepStatus::Skipped, None),
                Err(e) if step.optional => (StepStatus::Warned, Some(self.mask(&e.to_string()))),
                Err(e) => (StepStatus::Failed, Some(self.mask(&e.to_string()))),
            };
            let result = StepResult { name, depth, status, duration: start.elapsed(), error };
            if let Some(cb) = self.on_step.as_mut() {
                cb(&result);
            }
            // Nested steps were recorded while this one ran; it goes before them.
            self.steps.insert(at, result);
            if status == StepStatus::Failed {
                return Err(outcome.err().unwrap());
            }
        }
        Ok(())
    }

    fn app(&self, given: &Option<String>) -> Result<String> {
        let app = given.clone().or_else(|| self.app_id.clone()).ok_or_else(|| anyhow!("no appId: give one in the flow header"))?;
        self.js.expand(&app)
    }

    fn exec(&mut self, cmd: &Cmd, optional: bool) -> Result<Outcome> {
        match cmd {
            Cmd::LaunchApp(l) => {
                let app = self.app(&l.app_id)?;
                let opts = LaunchOptions {
                    clear_state: l.clear_state,
                    stop_app: l.stop_app.unwrap_or(true),
                    permissions: l.permissions.clone(),
                    arguments: l
                        .arguments
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), self.js.expand(&crate::flow::scalar(v).unwrap_or_default())?)))
                        .collect::<Result<_>>()?,
                };
                if !l.permissions.is_empty() {
                    self.dev.set_permissions(&app, &l.permissions)?;
                }
                self.dev.launch_app(&app, &opts)?;
            }
            Cmd::TapOn(t) => self.tap(t, optional)?,
            Cmd::AssertVisible(sel) => {
                self.find(sel, optional)?;
            }
            Cmd::Back => {
                self.dev.press_key(Key::Back)?;
                self.settle()?;
            }
            Cmd::RunFlow(r) => {
                let take = match &r.when {
                    Some(when) => self.condition(when)?,
                    None => true,
                };
                let (file, commands) = if take { (&r.file, &r.commands) } else { (&r.else_file, &r.else_commands) };
                if !take && file.is_none() && commands.is_empty() {
                    return Ok(Outcome::Skipped);
                }
                self.with_env(&r.env)?;
                match file {
                    Some(file) => self.run_file(file)?,
                    None => self.run_steps(commands)?,
                }
            }
            Cmd::Repeat { times, while_, commands } => {
                let times: Option<u64> = match times {
                    Some(t) => Some(self.js.expand(t)?.trim().parse::<f64>().map_err(|_| anyhow!("times must be a number"))? as u64),
                    None => None,
                };
                if times.is_none() && while_.is_none() {
                    bail!("repeat needs `times` or `while`");
                }
                let mut n = 0;
                while times.is_none_or(|t| n < t) {
                    if let Some(cond) = while_ {
                        if !self.condition(cond)? {
                            break;
                        }
                    }
                    self.run_steps(commands)?;
                    n += 1;
                }
            }
            Cmd::Retry(r) => {
                let max: u32 = match &r.max_retries {
                    Some(m) => self.js.expand(m)?.trim().parse().map_err(|_| anyhow!("maxRetries must be a number"))?,
                    None => 1,
                };
                self.with_env(&r.env)?;
                let mut attempt = 0;
                loop {
                    let first = self.steps.len();
                    let result = match &r.file {
                        Some(file) => self.run_file(file),
                        None => self.run_steps(&r.commands),
                    };
                    match result {
                        Ok(()) => break,
                        Err(e) if attempt >= max.min(MAX_RETRIES) => return Err(e),
                        Err(_) => {
                            // This attempt's failures are not the flow's.
                            for s in &mut self.steps[first..] {
                                if s.status == StepStatus::Failed {
                                    s.status = StepStatus::Warned;
                                }
                            }
                            attempt += 1;
                        }
                    }
                }
            }
            Cmd::RunScript { file, env, when } => {
                if let Some(when) = when {
                    if !self.condition(when)? {
                        return Ok(Outcome::Skipped);
                    }
                }
                let file = self.js.expand(file)?;
                let path = self.flow_dir.join(&file);
                let code = std::fs::read_to_string(&path).map_err(|e| anyhow!("runScript {}: {e}", path.display()))?;
                // A relative require() looks beside the script first, then in the flow's folder.
                let script_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                self.js.set_module_dirs(vec![script_dir, self.flow_dir.clone()]);
                let result = self.js.run_script(&code, env);
                self.js.set_module_dirs(vec![self.flow_dir.clone()]);
                result?;
            }
            Cmd::SetOrientation { orientation } => {
                let o = self.js.expand(orientation)?;
                let o = Orientation::parse(&o).ok_or_else(|| anyhow!("`{o}` is not an orientation (PORTRAIT, LANDSCAPE_LEFT, LANDSCAPE_RIGHT, UPSIDE_DOWN)"))?;
                self.dev.set_orientation(o)?;
                self.settle()?;
            }
            Cmd::SetLocation { latitude, longitude } => {
                let lat = self.number(latitude)?;
                let lon = self.number(longitude)?;
                self.dev.set_location(lat, lon)?;
            }
            Cmd::Travel { points, speed } => self.travel(points, speed.unwrap_or(4.0))?,
            Cmd::SetAirplaneMode { enabled } => self.dev.set_airplane_mode(Some(*enabled))?,
            Cmd::ToggleAirplaneMode => self.dev.set_airplane_mode(None)?,
            Cmd::SetDarkMode { enabled } => self.dev.set_dark_mode(Some(*enabled))?,
            Cmd::ToggleDarkMode => self.dev.set_dark_mode(None)?,
            Cmd::AssertDarkMode { dark } => {
                let now = self.dev.dark_mode()?;
                if now != *dark {
                    let name = |d: bool| if d { "dark" } else { "light" };
                    bail!("expected {} mode, but the device is in {} mode", name(*dark), name(now));
                }
            }
            Cmd::SetPermissions { app_id, permissions } => {
                let app = self.app(app_id)?;
                self.dev.set_permissions(&app, permissions)?;
            }
            Cmd::AddMedia { files } => {
                let paths: Vec<PathBuf> = files.iter().map(|f| Ok(self.flow_dir.join(self.js.expand(f)?))).collect::<Result<_>>()?;
                if let Some(missing) = paths.iter().find(|p| !p.exists()) {
                    bail!("addMedia: {} does not exist", missing.display());
                }
                let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
                self.dev.add_media(&refs)?;
            }
            Cmd::TakeScreenshot { path, crop_on } => {
                let png = self.screenshot(crop_on.as_ref(), optional)?;
                let out = self.output_file(&self.js.expand(path)?, "png")?;
                std::fs::write(&out, png)?;
            }
            Cmd::AssertScreenshot { path, threshold, crop_on } => {
                let png = self.screenshot(crop_on.as_ref(), optional)?;
                let mut name = self.js.expand(path)?;
                if !name.to_ascii_lowercase().ends_with(".png") {
                    name.push_str(".png");
                }
                let reference = self.flow_dir.join(&name);
                if !reference.exists() {
                    // The first run records what the screen looks like.
                    std::fs::write(&reference, &png)?;
                } else {
                    let want = crate::image_util::decode(&std::fs::read(&reference)?)?;
                    let got = crate::image_util::decode(&png)?;
                    let alike = crate::image_util::similarity(&want, &got);
                    let needed = threshold.unwrap_or(95.0);
                    if alike < needed {
                        let diff = self.output_file(&format!("{}_actual", name.trim_end_matches(".png")), "png")?;
                        std::fs::write(&diff, &png)?;
                        bail!("the screen is {alike:.1}% like {name}, below {needed}% (it is saved as {})", diff.display());
                    }
                }
            }
            Cmd::StartRecording { path } => {
                let out = self.output_file(&self.js.expand(path)?, "mp4")?;
                self.dev.start_recording(&out)?;
            }
            Cmd::StopRecording => self.dev.stop_recording()?,
            Cmd::StopApp { app_id } => {
                let app = self.app(app_id)?;
                self.dev.stop_app(&app)?;
            }
            Cmd::KillApp { app_id } => {
                let app = self.app(app_id)?;
                self.dev.kill_app(&app)?;
            }
            Cmd::ClearState { app_id } => {
                let app = self.app(app_id)?;
                self.dev.clear_state(&app)?;
            }
            Cmd::ClearKeychain => self.dev.clear_keychain()?,
            Cmd::OpenLink { link, browser, .. } => {
                let link = self.js.expand(link)?;
                let app = if *browser { None } else { self.app(&None).ok() };
                self.dev.open_link(&link, app.as_deref(), *browser)?;
                self.settle()?;
            }
            Cmd::PressKey { key } => {
                let name = self.js.expand(key)?;
                let key = Key::parse(&name).ok_or_else(|| anyhow!("`{name}` is not a key virtuoso knows"))?;
                // With a keyboard up, Backspace and Enter are its own keys, as a person presses
                // them: an app can count only the deletes the keyboard makes (React Native's
                // onKeyPress), and the keyboard corrects the word it composed from typed key
                // events on an Enter key event ("hello" became "Hello"), not on its own key.
                let on_keyboard = if matches!(key, Key::Backspace | Key::Enter) { self.keyboard_key(key)? } else { None };
                match on_keyboard {
                    Some((x, y)) => self.dev.tap(x, y)?,
                    None => self.dev.press_key(key)?,
                }
                self.settle()?;
            }
            // The device returns once the keyboard has gone.
            Cmd::HideKeyboard => self.dev.hide_keyboard()?,
            Cmd::Swipe(sw) => self.swipe(sw, optional)?,
            Cmd::Scroll => {
                let screen = self.dev.screen()?;
                let (from, to) = self.direction_points("UP", &screen.screen_rect(), None)?;
                self.dev.swipe(from, to, 400, Lift::Fling)?;
                self.settle()?;
            }
            Cmd::ScrollUntilVisible(s) => self.scroll_until_visible(s)?,
            Cmd::InputText { text } => {
                let text = self.js.expand(text)?;
                self.type_text(&text)?;
            }
            Cmd::EraseText { chars } => {
                let n = match chars {
                    Some(c) => self.js.expand(c)?.trim().parse().map_err(|_| anyhow!("charactersToErase must be a number"))?,
                    None => 50,
                };
                self.dev.erase_text(n)?;
                self.settle_typed()?;
            }
            Cmd::InputRandom { kind, length } => {
                let text = self.random_text(*kind, *length)?;
                self.type_text(&text)?;
            }
            Cmd::CopyTextFrom(sel) => {
                let (screen, i) = self.find(sel, optional)?;
                let text = screen.elements[i].label().to_string();
                self.js.set_copied_text(&text);
                self.copied = Some(text);
            }
            Cmd::SetClipboard { text } => {
                let text = self.js.expand(text)?;
                self.dev.set_clipboard(&text)?;
                self.js.set_copied_text(&text);
                self.copied = Some(text);
            }
            Cmd::PasteText => {
                let text = self.copied.clone().ok_or_else(|| anyhow!("nothing to paste: copyTextFrom or setClipboard first"))?;
                self.type_text(&text)?;
            }
            Cmd::AssertNotVisible(sel) => {
                let timeout = self.lookup_timeout(optional);
                self.wait_gone(sel, timeout)?;
            }
            Cmd::ExtendedWaitUntil { visible, not_visible, timeout_ms } => {
                let timeout = match timeout_ms {
                    Some(t) => self.millis(t)?,
                    None => self.opts.lookup_timeout,
                };
                if let Some(sel) = visible {
                    self.find_within(sel, timeout)?;
                }
                if let Some(sel) = not_visible {
                    self.wait_gone(sel, timeout)?;
                }
            }
            Cmd::AssertTrue { condition } => {
                if !self.js.truthy(condition)? {
                    bail!("Assertion is false: {condition}");
                }
            }
            Cmd::EvalScript { script } => {
                self.js.expand(script)?;
            }
            Cmd::WaitForAnimationToEnd { timeout_ms } => {
                let timeout = match timeout_ms {
                    Some(t) => self.millis(t)?,
                    None => Duration::from_secs(15),
                };
                self.dev.mark()?;
                self.dev.settle(Duration::ZERO, timeout)?;
            }
            Cmd::DragAndDrop { from, to, hold_ms, move_ms } => {
                let hold = match hold_ms {
                    Some(v) => self.millis(v)?,
                    None => Duration::from_millis(1000),
                };
                let moving = match move_ms {
                    Some(v) => self.millis(v)?,
                    None => Duration::from_millis(1000),
                };
                let a = self.anchor(from, optional)?;
                let b = self.anchor(to, optional)?;
                self.dev.drag(a, b, hold.as_millis() as u64, moving.as_millis() as u64)?;
                self.settle()?;
            }
            Cmd::OpenNotifications => {
                self.dev.open_notifications()?;
                self.settle()?;
            }
            Cmd::Alert { accept } => {
                let labels = if *accept { ACCEPT_LABELS } else { DISMISS_LABELS };
                let deadline = std::time::Instant::now() + self.opts.condition_timeout;
                let button = loop {
                    let screen = self.dev.screen()?;
                    let found = labels.iter().find_map(|want| {
                        (0..screen.elements.len()).find(|&i| screen.matchable(i) && same_label(screen.elements[i].label(), want))
                    });
                    if let Some(i) = found {
                        break Some(screen.elements[i].bounds.center());
                    }
                    if std::time::Instant::now() >= deadline {
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                };
                let Some((x, y)) = button else { return Ok(Outcome::Skipped) };
                self.dev.tap(x, y)?;
                self.settle()?;
            }
            Cmd::Wait { ms } => std::thread::sleep(self.millis(ms)?),
            Cmd::RunShell { command, output, env, timeout_ms } => {
                let timeout = match timeout_ms {
                    Some(t) => self.millis(t)?,
                    None => RUN_SHELL_TIMEOUT,
                };
                let command = self.js.expand(command)?;
                let mut vars = vec![("MAESTRO_PLATFORM".to_string(), self.dev.platform().to_string()), ("MAESTRO_DEVICE_UDID".into(), self.dev.id())];
                for (k, v) in env {
                    vars.push((k.clone(), self.js.expand(v)?));
                }
                let text = run_shell(&command, &vars, timeout)?;
                if let Some(name) = output {
                    self.js.set_var(name, &text);
                }
            }
            #[allow(unreachable_patterns)]
            other => bail!("{} is not supported yet", describe(other)),
        }
        Ok(Outcome::Done)
    }

    fn with_env(&mut self, env: &[(String, String)]) -> Result<()> {
        for (k, v) in env {
            let v = self.js.expand(v)?;
            self.js.set_var(k, &v);
            self.keep_secret(k, &v);
        }
        Ok(())
    }

    /// Runs another flow file's commands (path relative to the current flow) in this run.
    fn run_file(&mut self, file: &str) -> Result<()> {
        let file = self.js.expand(file)?;
        let path = self.flow_dir.join(&file);
        let text = std::fs::read_to_string(&path).map_err(|e| anyhow!("runFlow {}: {e}", path.display()))?;
        let flow = crate::flow::parse_flow(&text, Some(&path))?;
        let saved_dir = std::mem::replace(&mut self.flow_dir, path.parent().map(|p| p.to_path_buf()).unwrap_or_default());
        self.js.set_module_dirs(vec![self.flow_dir.clone()]);
        let result = (|| {
            for (k, v) in &flow.config.env {
                let v = self.js.expand(v)?;
                self.js.set_var(k, &v);
            }
            self.run_steps(&flow.config.on_flow_start)?;
            let r = self.run_steps(&flow.commands);
            let after = self.run_steps(&flow.config.on_flow_complete);
            r.and(after)
        })();
        self.flow_dir = saved_dir;
        self.js.set_module_dirs(vec![self.flow_dir.clone()]);
        result
    }

    /// Every part of a `when:` or `while:` must hold.
    fn condition(&mut self, c: &crate::flow::Condition) -> Result<bool> {
        if let Some(p) = &c.platform {
            if !self.js.expand(p)?.eq_ignore_ascii_case(self.dev.platform()) {
                return Ok(false);
            }
        }
        if let Some(t) = &c.true_ {
            if !self.js.truthy(t)? {
                return Ok(false);
            }
        }
        // A zero timeout still reads the screen once.
        let timeout = self.opts.condition_timeout.saturating_sub(self.last_change.elapsed());
        if let Some(sel) = &c.visible {
            if self.find_within(sel, timeout).is_err() {
                return Ok(false);
            }
        }
        if let Some(sel) = &c.not_visible {
            if self.wait_gone(sel, timeout).is_err() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn number(&self, v: &str) -> Result<f64> {
        let v = self.js.expand(v)?;
        v.trim().parse().map_err(|_| anyhow!("`{v}` is not a number"))
    }

    /// A file in the output folder (the working directory without one), with `ext` added
    /// when the name has none.
    fn output_file(&self, name: &str, ext: &str) -> Result<PathBuf> {
        let dir = self.opts.output_dir.clone().unwrap_or_else(|| PathBuf::from("."));
        let mut path = dir.join(name);
        if path.extension().is_none() {
            path.set_extension(ext);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(path)
    }

    fn screenshot(&mut self, crop_on: Option<&Selector>, optional: bool) -> Result<Vec<u8>> {
        let png = self.dev.screenshot()?;
        let Some(sel) = crop_on else { return Ok(png) };
        let (screen, i) = self.find(sel, optional)?;
        let b = screen.elements[i].bounds;
        let img = crate::image_util::decode(&png)?;
        Ok(crate::image_util::encode(&crate::image_util::crop(&img, b.left, b.top, b.right, b.bottom)))
    }

    /// Walks the location through the points at `speed` m/s, one update a second.
    fn travel(&mut self, points: &[String], speed: f64) -> Result<()> {
        let pts: Vec<(f64, f64)> = points
            .iter()
            .map(|p| {
                let p = self.js.expand(p)?;
                let (a, b) = p.split_once(',').ok_or_else(|| anyhow!("`{p}` is not latitude,longitude"))?;
                Ok((a.trim().parse()?, b.trim().parse()?))
            })
            .collect::<Result<_>>()?;
        let Some(&first) = pts.first() else { bail!("travel needs points") };
        self.dev.set_location(first.0, first.1)?;
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let secs = haversine_m(a, b) / speed.max(0.001);
            let steps = (secs.ceil() as u64).max(1);
            for s in 1..=steps {
                std::thread::sleep(Duration::from_secs_f64(secs / steps as f64));
                let t = s as f64 / steps as f64;
                self.dev.set_location(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)?;
            }
        }
        Ok(())
    }

    fn millis(&self, v: &str) -> Result<Duration> {
        let v = self.js.expand(v)?;
        let ms: f64 = v.trim().parse().map_err(|_| anyhow!("`{v}` is not a number of milliseconds"))?;
        Ok(Duration::from_millis(ms.max(0.0) as u64))
    }

    fn type_text(&mut self, text: &str) -> Result<()> {
        self.dev.input_text(text)?;
        self.settle_typed()?;
        Ok(())
    }

    /// After typing, the field echoes at once; a short no-change wait is enough.
    fn settle_typed(&mut self) -> Result<()> {
        self.dev.settle(Duration::from_millis(100), self.opts.settle_timeout)?;
        Ok(())
    }

    fn random_text(&self, kind: crate::flow::RandomKind, length: Option<i32>) -> Result<String> {
        use crate::flow::RandomKind::*;
        let expr = match kind {
            Email => "faker.internet().emailAddress()".to_string(),
            Number => format!("faker.digits({})", length.unwrap_or(8).clamp(1, 18)),
            PersonName => "faker.name().fullName()".into(),
            Text => format!("faker.text({})", length.unwrap_or(8).max(1)),
            CityName => "faker.address().city()".into(),
            CountryName => "faker.address().country()".into(),
            ColorName => "faker.color().name()".into(),
        };
        self.js.expand(&format!("${{{expr}}}"))
    }

    /// Where a drag starts or ends: the element's centre, or the point on the screen.
    fn anchor(&mut self, a: &crate::flow::Anchor, optional: bool) -> Result<(i32, i32)> {
        if let Some(p) = &a.point {
            let p = self.js.expand(p)?;
            let screen = self.dev.screen()?;
            return parse_point(&p, &screen.screen_rect());
        }
        let sel = a.selector.as_ref().ok_or_else(|| anyhow!("dragAndDrop needs a selector or a point"))?;
        let (screen, i) = self.find(sel, optional)?;
        Ok(screen.elements[i].bounds.center())
    }

    fn settle(&mut self) -> Result<crate::device::Settled> {
        self.dev.settle(Duration::from_millis(300), self.opts.settle_timeout)
    }

    fn lookup_timeout(&self, optional: bool) -> Duration {
        if optional {
            self.opts.optional_lookup_timeout
        } else {
            self.opts.lookup_timeout
        }
    }

    /// `${...}` expanded in every text field of a selector.
    fn expand_selector(&self, s: &Selector) -> Result<Selector> {
        let ex = |o: &Option<String>| o.as_deref().map(|v| self.js.expand(v)).transpose();
        let sub = |o: &Option<Box<Selector>>| -> Result<Option<Box<Selector>>> {
            o.as_ref().map(|b| self.expand_selector(b).map(Box::new)).transpose()
        };
        Ok(Selector {
            text: ex(&s.text)?,
            id: ex(&s.id)?,
            index: ex(&s.index)?,
            below: sub(&s.below)?,
            above: sub(&s.above)?,
            left_of: sub(&s.left_of)?,
            right_of: sub(&s.right_of)?,
            child_of: sub(&s.child_of)?,
            contains_child: sub(&s.contains_child)?,
            contains_descendants: s.contains_descendants.iter().map(|d| self.expand_selector(d)).collect::<Result<_>>()?,
            ..s.clone()
        })
    }

    /// Waits for the selector's first match.
    fn find(&mut self, sel: &Selector, optional: bool) -> Result<(ScreenTree, usize)> {
        let timeout = self.lookup_timeout(optional);
        self.find_within(sel, timeout)
    }

    /// Waits until nothing matches the selector.
    fn wait_gone(&mut self, sel: &Selector, timeout: Duration) -> Result<()> {
        let sel = self.expand_selector(sel)?;
        let deadline = Instant::now() + timeout;
        let mut screen = self.dev.screen()?;
        loop {
            if select(&screen, &sel).map_err(|e| anyhow!(e))?.is_empty() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                bail!("{} is still visible", describe_selector(&sel));
            }
            screen = self.dev.screen_after_change(screen, (deadline - now).min(CHANGE_WAIT))?;
        }
    }

    fn find_within(&mut self, sel: &Selector, timeout: Duration) -> Result<(ScreenTree, usize)> {
        let sel = self.expand_selector(sel)?;
        let deadline = Instant::now() + timeout;
        let mut screen = self.dev.screen()?;
        loop {
            if let Some(&i) = select(&screen, &sel).map_err(|e| anyhow!(e))?.first() {
                return Ok((screen, i));
            }
            let now = Instant::now();
            if now >= deadline {
                bail!("Element not found: {}", describe_selector(&sel));
            }
            screen = self.dev.screen_after_change(screen, (deadline - now).min(CHANGE_WAIT))?;
        }
    }

    /// Waits (up to the lookup's time; then taps anyway) for a loading overlay over the element
    /// to go, and finds the element again where it rests: a tap would land on the overlay.
    fn wait_uncovered(&mut self, sel: &Selector, mut screen: ScreenTree, optional: bool) -> Result<(ScreenTree, usize)> {
        let expanded = self.expand_selector(sel)?;
        let timeout = if optional { self.opts.optional_lookup_timeout } else { self.opts.lookup_timeout };
        let deadline = Instant::now() + timeout;
        loop {
            match select(&screen, &expanded).map_err(|e| anyhow!(e))?.first() {
                Some(&i) if crate::select::loading_cover(&screen, i).is_none() => break,
                None => break,
                _ => {}
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            screen = self.dev.screen_after_change(screen, (deadline - now).min(CHANGE_WAIT))?;
        }
        self.settle()?;
        self.find(sel, optional)
    }

    /// The element's frame once it rests: a layout moving under it (a banner growing in above
    /// it) would take the tap where the element was. One fresh read on a still screen (the
    /// element is where the lookup found it); on a moving one, read until it has stayed put
    /// for REST_MS, up to REST_WAIT.
    fn resting(&mut self, sel: &Selector, screen: ScreenTree, i: usize) -> Result<Rect> {
        let sel = self.expand_selector(sel)?;
        let deadline = Instant::now() + REST_WAIT;
        let mut at = screen.elements[i].bounds;
        let mut since = Instant::now();
        let mut first = true;
        loop {
            // Past the cache: an animation outside the app's layout pass moves views without an
            // event, and a cached read keeps their old bounds.
            let next = self.dev.fresh_screen()?;
            let Some(&j) = select(&next, &sel).map_err(|e| anyhow!(e))?.first() else { return Ok(at) };
            let b = next.elements[j].bounds;
            if b != at {
                (at, since) = (b, Instant::now());
            } else if first || since.elapsed() >= REST_MS {
                // Where the lookup found it: still there.
                return Ok(at);
            }
            if Instant::now() >= deadline {
                return Ok(at);
            }
            first = false;
            std::thread::sleep(REST_POLL);
        }
    }

    fn tap(&mut self, t: &TapOn, optional: bool) -> Result<()> {
        let point = t.point.as_deref().map(|p| self.js.expand(p)).transpose()?;
        let (x, y) = if t.selector == Selector::default() {
            let p = point.ok_or_else(|| anyhow!("tapOn needs a selector or a point"))?;
            let screen = self.dev.screen()?;
            parse_point(&p, &screen.screen_rect())?
        } else {
            let (mut screen, mut i) = self.find(&t.selector, optional)?;
            if crate::select::loading_cover(&screen, i).is_some() {
                (screen, i) = self.wait_uncovered(&t.selector, screen, optional)?;
            }
            if crate::select::bar_cover(&screen, i).is_some() {
                (screen, i) = self.drag_clear(&t.selector, screen, i, optional)?;
            }
            let b = self.resting(&t.selector, screen, i)?;
            match point {
                Some(p) => parse_point(&p, &b)?,
                None => b.center(),
            }
        };
        let repeat: u32 = match &t.repeat {
            Some(r) => self.js.expand(r)?.trim().parse().map_err(|_| anyhow!("repeat must be a number"))?,
            None => 1,
        };
        let delay = match &t.delay_ms {
            Some(d) => self.millis(d)?,
            None => Duration::from_millis(100),
        };
        if t.long_press {
            self.dev.long_press(x, y, LONG_PRESS_MS)?;
        } else if repeat > 1 {
            self.dev.taps(x, y, repeat, delay)?;
        } else {
            self.dev.tap(x, y)?;
        }
        let settle_timeout = match &t.wait_to_settle_ms {
            Some(ms) => self.millis(ms)?.min(Duration::from_secs(30)),
            None => self.opts.settle_timeout,
        };
        let settled = self.dev.settle(Duration::from_millis(300), settle_timeout)?;
        if t.retry_if_no_change == Some(true) && !settled.changed {
            self.dev.tap(x, y)?;
            self.settle()?;
        }
        if t.wait_until_visible == Some(true) && !settled.changed && t.selector != Selector::default() {
            self.tap_when_uncovered(&t.selector)?;
        }
        Ok(())
    }

    /// An element under a sticky header or footer (bar_cover), which would take the tap: as a
    /// user would, the list around it is dragged away from the bar until the element shows, a
    /// screen's tenth past it, up to MAX_COVER_CLEARS drags while the list moves.
    /// Where the keyboard's key for `key` is, read past the cache (a keyboard that has just
    /// come up may not be in it). A field just tapped has focus before its keyboard is back up
    /// (Gboard's Done closes it): while a field has focus, wait a little for the keyboard.
    fn keyboard_key(&mut self, key: Key) -> Result<Option<(i32, i32)>> {
        use crate::screen::{EDITABLE, FOCUSED};
        let deadline = Instant::now() + KEYBOARD_WAIT;
        loop {
            let screen = self.dev.fresh_screen()?;
            if let Some(at) = keyboard_key(&screen, key) {
                return Ok(Some(at));
            }
            let typing = screen.elements.iter().any(|e| e.has(FOCUSED) && e.has(EDITABLE));
            if !typing || Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(REST_POLL);
        }
    }

    fn drag_clear(&mut self, sel: &Selector, mut screen: ScreenTree, mut i: usize, optional: bool) -> Result<(ScreenTree, usize)> {
        for _ in 0..MAX_COVER_CLEARS {
            let (Some(c), Some(list)) = (crate::select::bar_cover(&screen, i), screen.vertical_list_around(i)) else { break };
            let rect = screen.screen_rect();
            let (e, c, l) = (screen.elements[i].bounds, screen.elements[c].bounds, screen.elements[list].bounds.intersect(&rect));
            let (x, mid) = l.center();
            let margin = rect.height() / 10;
            // A bar in the screen's top half is a header: the content goes down, out from under it.
            let dy = if c.center().1 < rect.height() / 2 { c.bottom - e.top + margin } else { -(e.bottom - c.top + margin) };
            let to = (mid + dy).clamp(l.top + 1, l.bottom - 1);
            self.dev.swipe((x, mid), (x, to), 300, Lift::Cancel)?;
            self.settle()?;
            let was = e;
            (screen, i) = self.find(sel, optional)?;
            if screen.elements[i].bounds == was {
                break; // the list moves no further
            }
        }
        Ok((screen, i))
    }

    /// After a tap that changed nothing: if something covers the element (a snackbar, a
    /// scrim), wait for it to go, then tap the element again.
    fn tap_when_uncovered(&mut self, sel: &Selector) -> Result<()> {
        let sel = self.expand_selector(sel)?;
        let deadline = Instant::now() + self.opts.lookup_timeout.min(UNCOVER_TIMEOUT);
        let mut was_covered = false;
        loop {
            let screen = self.dev.screen()?;
            if let Some(&i) = select(&screen, &sel).map_err(|e| anyhow!(e))?.first() {
                let (x, y) = screen.elements[i].bounds.center();
                let on_it = screen.element_at(x, y).is_none_or(|h| h == i || screen.is_ancestor(h, i) || screen.is_ancestor(i, h));
                if !on_it {
                    was_covered = true;
                } else if was_covered {
                    self.dev.tap(x, y)?;
                    self.settle()?;
                    return Ok(());
                } else {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Start and end of a swipe in a direction. UP moves the content up (a finger going up).
    fn direction_points(&self, dir: &str, screen: &Rect, from: Option<(i32, i32)>) -> Result<((i32, i32), (i32, i32))> {
        let (w, h) = (screen.width(), screen.height());
        let pct = |x: i32, y: i32| (w * x / 100, h * y / 100);
        Ok(match (dir.to_ascii_uppercase().as_str(), from) {
            ("UP", None) => (pct(50, 50), pct(50, 10)),
            ("DOWN", None) => (pct(50, 20), pct(50, 90)),
            ("LEFT", None) => (pct(90, 50), pct(10, 50)),
            ("RIGHT", None) => (pct(10, 50), pct(90, 50)),
            ("UP", Some((x, y))) => ((x, y), (x, 0)),
            ("DOWN", Some((x, y))) => ((x, y), (x, h)),
            ("LEFT", Some((x, y))) => ((x, y), (0, y)),
            ("RIGHT", Some((x, y))) => ((x, y), (w, y)),
            (other, _) => bail!("`{other}` is not a direction (UP, DOWN, LEFT, RIGHT)"),
        })
    }

    fn swipe(&mut self, sw: &crate::flow::Swipe, optional: bool) -> Result<()> {
        let ms = match &sw.duration_ms {
            Some(d) => self.millis(d)?.as_millis() as u64,
            None => 400,
        };
        let screen = self.dev.screen()?;
        let rect = screen.screen_rect();
        let (from, to) = if let (Some(start), Some(end)) = (&sw.start, &sw.end) {
            (parse_point(&self.js.expand(start)?, &rect)?, parse_point(&self.js.expand(end)?, &rect)?)
        } else {
            let dir = sw.direction.as_deref().ok_or_else(|| anyhow!("swipe needs a direction, or start and end"))?;
            let anchor = match &sw.from {
                Some(sel) => {
                    let (s, i) = self.find(sel, optional)?;
                    Some(s.elements[i].bounds.center())
                }
                None => None,
            };
            self.direction_points(dir, &rect, anchor)?
        };
        self.dev.swipe(from, to, ms, Lift::Fling)?;
        self.settle()?;
        Ok(())
    }

    fn scroll_until_visible(&mut self, s: &crate::flow::ScrollUntilVisible) -> Result<()> {
        let sel = self.expand_selector(&s.element)?;
        let timeout = match &s.timeout_ms {
            Some(t) => self.millis(t)?,
            None => Duration::from_secs(20),
        };
        let dir = s.direction.clone().unwrap_or_else(|| "DOWN".into()).to_ascii_uppercase();
        let pct = s.visibility_percentage.unwrap_or(100).clamp(1, 100) as i64;
        let deadline = Instant::now() + timeout;
        let stateless = Selector { enabled: None, checked: None, focused: None, selected: None, ..sel.clone() };
        let shown = |screen: &ScreenTree| -> Result<Option<usize>> {
            let rect = screen.screen_rect();
            Ok(select(screen, &sel).map_err(|e| anyhow!(e))?.into_iter().find(|&i| {
                let b = screen.elements[i].bounds;
                b.intersect(&rect).area() * 100 >= b.area() * pct
            }))
        };
        loop {
            let mut screen = self.dev.screen()?;
            let mut found = shown(&screen)?;
            if found.is_none() && !select(&screen, &stateless).map_err(|e| anyhow!(e))?.is_empty() {
                // There, but not in the state wanted: past the cache before scrolling on. A state
                // that changed without an event (a button selected by a double tap) reads stale,
                // and the scroll would carry the element off. (Only then: a whole tree costs.)
                screen = self.dev.fresh_screen()?;
                found = shown(&screen)?;
            }
            let rect = screen.screen_rect();
            if let Some(i) = found {
                if s.center_element {
                    let (_, cy) = screen.elements[i].bounds.center();
                    let mid = rect.height() / 2;
                    if (cy - mid).abs() > rect.height() / 10 {
                        let x = rect.width() / 2;
                        self.dev.swipe((x, cy), (x, mid), 500, Lift::Cancel)?;
                        self.settle()?;
                    }
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("Element not found after scrolling: {}", describe_selector(&sel));
            }
            // The content scrolls DOWN when the finger drags up. A drag without a fling moves
            // the content by about the drag (55% of the screen), so nothing is skipped past;
            // it starts and ends clear of the status and navigation bars.
            // Sideways, across the list that scrolls sideways, else the screen's middle.
            let (w, h) = (rect.width(), rect.height());
            let side_y = screen.sideways_list().map_or(h / 2, |k| screen.elements[k].bounds.center().1);
            let (from, to) = match dir.as_str() {
                "DOWN" => ((w / 2, h * 75 / 100), (w / 2, h * 20 / 100)),
                "UP" => ((w / 2, h * 20 / 100), (w / 2, h * 75 / 100)),
                "RIGHT" => ((w * 8 / 10, side_y), (w * 2 / 10, side_y)),
                "LEFT" => ((w * 2 / 10, side_y), (w * 8 / 10, side_y)),
                other => bail!("`{other}` is not a direction (UP, DOWN, LEFT, RIGHT)"),
            };
            // Up and down, cancelled: no list flings. Sideways, lifted: a pager snaps to a page.
            // Quick (SCROLL_DRAG_MS): without a fling, the drag's speed does not change how far
            // it goes, and the next cannot start until it has ended.
            let end = if matches!(dir.as_str(), "UP" | "DOWN") { Lift::Cancel } else { Lift::Still };
            self.dev.swipe(from, to, SCROLL_DRAG_MS, end)?;
            self.settle()?;
        }
    }
}

const LONG_PRESS_MS: u64 = 1000;
/// scrollUntilVisible's drag (speedrun's too). At 300 ms, drags back to back waited ~380 ms each.
const SCROLL_DRAG_MS: u64 = 50;
const MAX_COVER_CLEARS: usize = 2;
/// How long a key press waits for the keyboard of a field that has focus.
const KEYBOARD_WAIT: Duration = Duration::from_millis(1500);

fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let dla = la2 - la1;
    let dlo = (b.1 - a.1).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.sqrt().asin()
}
/// Maestro caps retry's maxRetries at 3.
const MAX_RETRIES: u32 = 3;

/// "x,y" in pixels or "x%,y%" of `within` (the screen, or an element when there is a selector).
pub fn parse_point(p: &str, within: &Rect) -> Result<(i32, i32)> {
    let (a, b) = p.split_once(',').ok_or_else(|| anyhow!("`{p}` is not a point (x,y or x%,y%)"))?;
    let coord = |v: &str, origin: i32, size: i32| -> Result<i32> {
        let v = v.trim();
        if let Some(pc) = v.strip_suffix('%') {
            let pc: f64 = pc.trim().parse().map_err(|_| anyhow!("`{p}` is not a point"))?;
            Ok(origin + (size as f64 * pc / 100.0).round() as i32)
        } else {
            let px: f64 = v.parse().map_err(|_| anyhow!("`{p}` is not a point"))?;
            Ok(origin + px as i32)
        }
    };
    Ok((coord(a, within.left, within.width())?, coord(b, within.top, within.height())?))
}

pub fn flow_name(flow: &Flow) -> String {
    flow.config.name.clone().unwrap_or_else(|| {
        flow.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "flow".into())
    })
}

pub fn describe_selector(s: &Selector) -> String {
    let mut parts = Vec::new();
    if let Some(t) = &s.text {
        parts.push(format!("\"{t}\""));
    }
    if let Some(id) = &s.id {
        parts.push(format!("id: {id}"));
    }
    if let Some(i) = &s.index {
        parts.push(format!("index: {i}"));
    }
    for (name, rel) in [("below", &s.below), ("above", &s.above), ("leftOf", &s.left_of), ("rightOf", &s.right_of), ("childOf", &s.child_of), ("containsChild", &s.contains_child)] {
        if let Some(r) = rel {
            parts.push(format!("{name} {}", describe_selector(r)));
        }
    }
    for (name, v) in [("enabled", s.enabled), ("checked", s.checked), ("focused", s.focused), ("selected", s.selected)] {
        if let Some(v) = v {
            parts.push(format!("{name}: {v}"));
        }
    }
    if parts.is_empty() {
        "any element".into()
    } else {
        parts.join(", ")
    }
}

/// System dialog buttons that accept, most specific first (Android's permission dialog offers
/// "While using the app" and "Only this time" before a plain "Allow").
const ACCEPT_LABELS: &[&str] = &["While using the app", "Allow only while using the app", "Allow all", "Allow", "Only this time", "OK", "Accept", "Continue", "Yes"];
const DISMISS_LABELS: &[&str] = &["Don't allow", "Deny", "Deny & don't ask again", "Don't allow & don't ask again", "Not now", "Cancel", "No", "Dismiss"];

/// Labels compare without case, and with typographic apostrophes as plain ones.
/// The centre of the on-screen keyboard's key for `key` (Backspace or Enter), when a keyboard
/// shows one. Gboard labels its Enter key by the field's action ("Done", "Search"...).
fn keyboard_key(screen: &ScreenTree, key: Key) -> Option<(i32, i32)> {
    use crate::screen::{CLICKABLE, KEYBOARD};
    let is_key = |e: &crate::screen::Element| match key {
        Key::Backspace => e.label() == "Delete",
        _ => e.id.ends_with("key_pos_ime_action") || matches!(e.label(), "Enter" | "Return"),
    };
    let rect = screen.screen_rect();
    screen
        .elements
        .iter()
        .find(|e| e.has(KEYBOARD) && e.has(CLICKABLE) && is_key(e) && e.bounds.intersect(&rect).area() > 0)
        .map(|e| e.bounds.center())
}

fn same_label(have: &str, want: &str) -> bool {
    have.trim().replace('\u{2019}', "'").eq_ignore_ascii_case(want)
}

/// The longest a command waiting for an element (or for one to go) waits for the screen to
/// change before it looks again; a change ends the wait at once.
const CHANGE_WAIT: Duration = Duration::from_millis(200);

/// How long `waitUntilVisible` waits for whatever covers an element to go.
const UNCOVER_TIMEOUT: Duration = Duration::from_secs(10);

/// A host command that never returns must not hang the run.
const RUN_SHELL_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs a command line in the host's shell; its trimmed stdout, or an error with its output.
fn run_shell(command: &str, env: &[(String, String)], timeout: Duration) -> Result<String> {
    use std::process::{Command, Stdio};
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| anyhow!("runShell: cannot start the shell: {e}"))?;
    // Read the pipes on their own threads so a chatty command cannot block on a full pipe.
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut s);
        s
    });
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            bail!("runShell: `{command}` did not finish within {} ms", timeout.as_millis());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if !status.success() {
        bail!("runShell: `{command}` failed ({status}): {}", format!("{out}{err}").trim());
    }
    Ok(out.trim().to_string())
}

/// A one-line description of a command, as the live view and reports show it.
pub fn describe(cmd: &Cmd) -> String {
    match cmd {
        Cmd::LaunchApp(l) => format!("Launch app{}", l.app_id.as_ref().map(|a| format!(" {a}")).unwrap_or_default()),
        Cmd::TapOn(t) => {
            let verb = if t.long_press { "Long press on" } else if t.repeat.as_deref() == Some("2") { "Double tap on" } else { "Tap on" };
            match &t.point {
                Some(p) if t.selector == Selector::default() => format!("{verb} point {p}"),
                _ => format!("{verb} {}", describe_selector(&t.selector)),
            }
        }
        Cmd::AssertVisible(s) => format!("Assert that {} is visible", describe_selector(s)),
        Cmd::AssertNotVisible(s) => format!("Assert that {} is not visible", describe_selector(s)),
        Cmd::InputText { text } => format!("Input text {text}"),
        Cmd::RunFlow(r) => format!("Run flow {}", r.file.clone().unwrap_or_else(|| "(inline)".into())),
        other => name_of(other).to_string(),
    }
}

/// How long an element must stay at one place before a tap trusts its frame, and how long a
/// tap waits for that at most.
const REST_MS: Duration = Duration::from_millis(50);
const REST_WAIT: Duration = Duration::from_secs(2);
const REST_POLL: Duration = Duration::from_millis(15);

/// Whether a command may change what the app shows (an `evalScript` may, through the
/// variables later commands use). Checks, waits and containers do not: a container's own
/// commands count one by one.
fn may_change_the_app(cmd: &Cmd) -> bool {
    !matches!(
        cmd,
        Cmd::AssertVisible(..)
            | Cmd::AssertNotVisible(..)
            | Cmd::AssertTrue { .. }
            | Cmd::AssertScreenshot { .. }
            | Cmd::AssertDarkMode { .. }
            | Cmd::ExtendedWaitUntil { .. }
            | Cmd::WaitForAnimationToEnd { .. }
            | Cmd::CopyTextFrom(..)
            | Cmd::TakeScreenshot { .. }
            | Cmd::RunFlow(..)
            | Cmd::Repeat { .. }
            | Cmd::Retry { .. }
    )
}

/// A command's name, for those without their own wording (no Debug formatting: its code for
/// every command type is ~30 KB of the binary).
fn name_of(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::LaunchApp(..) => "LaunchApp",
        Cmd::TapOn(..) => "TapOn",
        Cmd::AssertVisible(..) => "AssertVisible",
        Cmd::AssertNotVisible(..) => "AssertNotVisible",
        Cmd::InputText { .. } => "InputText",
        Cmd::RunFlow(..) => "RunFlow",
        Cmd::Back => "Back",
        Cmd::HideKeyboard => "HideKeyboard",
        Cmd::WaitForAnimationToEnd { .. } => "WaitForAnimationToEnd",
        Cmd::StopApp { .. } => "StopApp",
        Cmd::KillApp { .. } => "KillApp",
        Cmd::ClearState { .. } => "ClearState",
        Cmd::ClearKeychain => "ClearKeychain",
        Cmd::PasteText => "PasteText",
        Cmd::Scroll => "Scroll",
        Cmd::EraseText { .. } => "EraseText",
        Cmd::InputRandom { .. } => "InputRandom",
        Cmd::AssertTrue { .. } => "AssertTrue",
        Cmd::ExtendedWaitUntil { .. } => "ExtendedWaitUntil",
        Cmd::Swipe(..) => "Swipe",
        Cmd::ScrollUntilVisible(..) => "ScrollUntilVisible",
        Cmd::PressKey { .. } => "PressKey",
        Cmd::CopyTextFrom(..) => "CopyTextFrom",
        Cmd::SetClipboard { .. } => "SetClipboard",
        Cmd::OpenLink { .. } => "OpenLink",
        Cmd::TakeScreenshot { .. } => "TakeScreenshot",
        Cmd::AssertScreenshot { .. } => "AssertScreenshot",
        Cmd::StartRecording { .. } => "StartRecording",
        Cmd::StopRecording => "StopRecording",
        Cmd::SetOrientation { .. } => "SetOrientation",
        Cmd::SetLocation { .. } => "SetLocation",
        Cmd::Travel { .. } => "Travel",
        Cmd::SetAirplaneMode { .. } => "SetAirplaneMode",
        Cmd::ToggleAirplaneMode => "ToggleAirplaneMode",
        Cmd::SetPermissions { .. } => "SetPermissions",
        Cmd::AddMedia { .. } => "AddMedia",
        Cmd::SetDarkMode { .. } => "SetDarkMode",
        Cmd::ToggleDarkMode => "ToggleDarkMode",
        Cmd::AssertDarkMode { .. } => "AssertDarkMode",
        Cmd::Repeat { .. } => "Repeat",
        Cmd::Retry(..) => "Retry",
        Cmd::RunScript { .. } => "RunScript",
        Cmd::EvalScript { .. } => "EvalScript",
        Cmd::Wait { .. } => "Wait",
        Cmd::RunShell { .. } => "RunShell",
        Cmd::OpenNotifications => "OpenNotifications",
        Cmd::DragAndDrop { .. } => "DragAndDrop",
        Cmd::Alert { .. } => "Alert",
    }
}
