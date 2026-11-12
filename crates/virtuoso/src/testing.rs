//! A scripted device for tests and `--dry-run`: named screens, and taps that move between them.

use crate::device::{Device, Key, LaunchOptions, Lift, Orientation, Settled};
use crate::screen::{Element, Rect, ScreenTree, ENABLED, VISIBLE};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FakeNode {
    pub el: Element,
    /// Tapping it shows this screen.
    pub goes_to: Option<String>,
    /// It takes the taps on it even though it goes nowhere: it covers what is under it.
    pub covers: bool,
}

/// A visible, enabled node with `text`, at depth 1 under the screen's root.
pub fn node(text: &str, b: [i32; 4]) -> FakeNode {
    FakeNode {
        el: Element {
            depth: 1,
            text: text.into(),
            bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
            flags: VISIBLE | ENABLED,
            ..Default::default()
        },
        goes_to: None,
        covers: false,
    }
}

impl FakeNode {
    pub fn covers(mut self) -> Self {
        self.covers = true;
        self
    }

    pub fn goes_to(mut self, screen: &str) -> Self {
        self.goes_to = Some(screen.into());
        self
    }
    pub fn id(mut self, id: &str) -> Self {
        self.el.id = id.into();
        self
    }
    pub fn depth(mut self, d: u16) -> Self {
        self.el.depth = d;
        self
    }
    pub fn flags(mut self, f: u32) -> Self {
        self.el.flags = f;
        self
    }
    pub fn desc(mut self, d: &str) -> Self {
        self.el.desc = d.into();
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct FakeScreen {
    pub nodes: Vec<FakeNode>,
    /// Back shows this screen.
    pub back: Option<String>,
    /// Scrolling down (a swipe up) shows this screen.
    pub scrolls_to: Option<String>,
    /// Scrolling up (a drag down) shows this screen.
    pub scrolls_back: Option<String>,
}

impl FakeScreen {
    pub fn new(nodes: Vec<FakeNode>) -> Self {
        FakeScreen { nodes, ..Default::default() }
    }
    pub fn back_to(mut self, s: &str) -> Self {
        self.back = Some(s.into());
        self
    }
    pub fn scrolls_to(mut self, s: &str) -> Self {
        self.scrolls_to = Some(s.into());
        self
    }
    pub fn scrolls_back(mut self, s: &str) -> Self {
        self.scrolls_back = Some(s.into());
        self
    }
}

pub struct FakeDevice {
    pub screens: HashMap<String, FakeScreen>,
    pub current: String,
    /// Shown after that many more screen reads.
    pending: Option<(String, usize)>,
    /// Every action, in order: "tap 200,130", "launch com.app", "key Back", "text hello"...
    pub log: Vec<String>,
    pub changed_since_mark: bool,
    pub dark: bool,
    pub airplane: bool,
    pub reads: usize,
    pub width: i32,
    pub height: i32,
    pub keyboard: bool,
    /// How many times the runner waited for the UI to settle.
    pub settles: usize,
    /// Taps made in the same request as the check that the screen had not changed.
    pub taps_if_unchanged: usize,
    /// Looks taken to act on (a tap's lookup): a screen that held still, or a fresh one.
    pub looks_to_act_on: usize,
    pub fresh_reads: usize,
    /// What cached reads still show until a fresh read: a change that sent no event.
    stale: Option<String>,
    /// "android" or "ios".
    pub platform: &'static str,
    /// Taps to come that land while the app is still animating: ignored, and reported so.
    pub busy_taps: u32,
    busy: bool,
}

impl Default for FakeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDevice {
    pub fn new() -> Self {
        FakeDevice {
            screens: HashMap::new(),
            current: String::new(),
            pending: None,
            log: vec![],
            changed_since_mark: false,
            dark: false,
            airplane: false,
            reads: 0,
            width: 1080,
            height: 2400,
            keyboard: false,
            settles: 0,
            taps_if_unchanged: 0,
            looks_to_act_on: 0,
            fresh_reads: 0,
            stale: None,
            platform: "android",
            busy_taps: 0,
            busy: false,
        }
    }

    pub fn add_screen(&mut self, name: &str, s: FakeScreen) {
        self.screens.insert(name.into(), s);
    }

    pub fn show(&mut self, name: &str) {
        if self.current != name {
            self.changed_since_mark = true;
        }
        self.current = name.into();
    }

    pub fn show_after_reads(&mut self, name: &str, reads: usize) {
        self.pending = Some((name.into(), reads));
    }

    /// Cached reads show `name` until a fresh read: the app changed without an event.
    pub fn cache_stale(&mut self, name: &str) {
        self.stale = Some(name.into());
    }

    fn go(&mut self, to: Option<String>) {
        if let Some(to) = to {
            self.show(&to);
        }
    }
}

impl Device for FakeDevice {
    fn platform(&self) -> &'static str {
        self.platform
    }

    fn screen_to_act_on(&mut self) -> Result<ScreenTree> {
        self.looks_to_act_on += 1;
        self.screen()
    }

    fn touched_while_busy(&self) -> bool {
        self.busy
    }

    fn tap_if_unchanged(&mut self, screen: &ScreenTree, x: i32, y: i32) -> Result<Option<ScreenTree>> {
        let now = self.fresh_screen()?;
        if now.elements != screen.elements {
            return Ok(Some(now));
        }
        self.taps_if_unchanged += 1;
        self.tap(x, y)?;
        Ok(None)
    }

    fn screen_rect(&mut self) -> Result<Rect> {
        Ok(Rect { left: 0, top: 0, right: self.width, bottom: self.height })
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        self.reads += 1;
        if let Some((name, left)) = self.pending.take() {
            if left <= 1 {
                self.show(&name);
            } else {
                self.pending = Some((name, left - 1));
            }
        }
        let mut els = vec![Element {
            bounds: Rect { left: 0, top: 0, right: self.width, bottom: self.height },
            flags: VISIBLE | ENABLED,
            ..Default::default()
        }];
        if let Some(s) = self.screens.get(self.stale.as_ref().unwrap_or(&self.current)) {
            els.extend(s.nodes.iter().map(|n| n.el.clone()));
        }
        Ok(ScreenTree::new(els, self.width, self.height))
    }

    fn fresh_screen(&mut self) -> Result<ScreenTree> {
        self.fresh_reads += 1;
        self.stale = None;
        self.screen()
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<()> {
        self.log.push(format!("tap {x},{y}"));
        self.changed_since_mark = false;
        self.busy = self.busy_taps > 0;
        if self.busy {
            self.busy_taps -= 1;
            return Ok(());
        }
        let to = self.screens.get(&self.current).and_then(|s| {
            s.nodes.iter().rev().find(|n| n.el.bounds.contains(x, y) && (n.goes_to.is_some() || n.covers)).and_then(|n| n.goes_to.clone())
        });
        self.go(to);
        Ok(())
    }

    fn taps(&mut self, x: i32, y: i32, count: u32, interval: std::time::Duration) -> Result<()> {
        self.log.push(format!("taps {x},{y} {count}x {}ms", interval.as_millis()));
        self.changed_since_mark = false;
        Ok(())
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.log.push(format!("long press {x},{y} {ms}ms"));
        Ok(())
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, end: Lift) -> Result<()> {
        self.changed_since_mark = false;
        let end = match end {
            Lift::Fling => " fling",
            Lift::Still => "",
            Lift::Cancel => " cancel",
        };
        self.log.push(format!("swipe {},{} -> {},{} {ms}ms{end}", from.0, from.1, to.0, to.1));
        let next = match to.1.cmp(&from.1) {
            std::cmp::Ordering::Less => self.screens.get(&self.current).and_then(|s| s.scrolls_to.clone()),
            std::cmp::Ordering::Greater => self.screens.get(&self.current).and_then(|s| s.scrolls_back.clone()),
            std::cmp::Ordering::Equal => None,
        };
        self.go(next);
        Ok(())
    }

    fn press_key(&mut self, key: Key) -> Result<()> {
        self.log.push(format!("key {key:?}"));
        self.changed_since_mark = false;
        if key == Key::Back {
            if self.keyboard {
                self.keyboard = false;
                return Ok(());
            }
            let to = self.screens.get(&self.current).and_then(|s| s.back.clone());
            self.go(to);
        }
        Ok(())
    }

    fn input_text(&mut self, text: &str) -> Result<()> {
        self.log.push(format!("text {text}"));
        Ok(())
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        self.log.push(format!("erase {chars}"));
        Ok(())
    }

    fn mark(&mut self) -> Result<()> {
        self.changed_since_mark = false;
        Ok(())
    }

    fn settle(&mut self, _no_change: Duration, _timeout: Duration) -> Result<Settled> {
        self.settles += 1;
        let changed = self.changed_since_mark;
        self.changed_since_mark = false;
        Ok(Settled { changed, timed_out: false })
    }

    fn launch_app(&mut self, app: &str, o: &LaunchOptions) -> Result<()> {
        let mut line = format!("launch {app}");
        if o.clear_state {
            line.push_str(" clearState");
        }
        if !o.stop_app {
            line.push_str(" noStop");
        }
        if o.clear_keychain {
            line.push_str(" clearKeychain");
        }
        for (k, v) in &o.arguments {
            line.push_str(&format!(" {k}={v}"));
        }
        self.log.push(line);
        Ok(())
    }

    fn stop_app(&mut self, app: &str) -> Result<()> {
        self.log.push(format!("stop {app}"));
        Ok(())
    }

    fn kill_app(&mut self, app: &str) -> Result<()> {
        self.log.push(format!("kill {app}"));
        Ok(())
    }

    fn clear_state(&mut self, app: &str) -> Result<()> {
        self.log.push(format!("clear {app}"));
        Ok(())
    }

    fn clear_keychain(&mut self) -> Result<()> {
        self.log.push("keychain cleared".into());
        Ok(())
    }

    fn open_link(&mut self, link: &str, app: Option<&str>, browser: bool) -> Result<()> {
        self.log.push(format!("open {link}{}{}", app.map(|a| format!(" in {a}")).unwrap_or_default(), if browser { " browser" } else { "" }));
        Ok(())
    }

    fn hide_keyboard(&mut self) -> Result<()> {
        self.log.push("hide keyboard".into());
        self.keyboard = false;
        // A screen with its keyboard up goes back to the one without, as a device hides it; with
        // none up yet, nothing happens.
        let shown = self.screens.get(&self.current).is_some_and(|s| s.nodes.iter().any(|n| n.el.has(crate::screen::KEYBOARD)));
        if shown {
            let to = self.screens.get(&self.current).and_then(|s| s.back.clone());
            self.go(to);
        }
        Ok(())
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        self.log.push("screenshot".into());
        Ok(crate::image_util::solid_png(4, 8, [200, 200, 200]))
    }

    fn set_orientation(&mut self, o: Orientation) -> Result<()> {
        self.log.push(format!("orientation {o:?}"));
        Ok(())
    }

    fn set_location(&mut self, lat: f64, lon: f64) -> Result<()> {
        self.log.push(format!("location {lat},{lon}"));
        Ok(())
    }

    fn set_airplane_mode(&mut self, on: Option<bool>) -> Result<()> {
        self.airplane = on.unwrap_or(!self.airplane);
        self.log.push(format!("airplane {}", self.airplane));
        Ok(())
    }

    fn set_permissions(&mut self, app: &str, perms: &[(String, String)]) -> Result<()> {
        let p: Vec<String> = perms.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.log.push(format!("permissions {app} {}", p.join(",")));
        Ok(())
    }

    fn add_media(&mut self, files: &[&Path]) -> Result<()> {
        for f in files {
            self.log.push(format!("media {}", f.file_name().unwrap().to_string_lossy()));
        }
        Ok(())
    }

    fn set_dark_mode(&mut self, on: Option<bool>) -> Result<()> {
        self.dark = on.unwrap_or(!self.dark);
        self.log.push(format!("dark {}", self.dark));
        Ok(())
    }

    fn dark_mode(&mut self) -> Result<bool> {
        Ok(self.dark)
    }

    fn start_recording(&mut self, path: &Path) -> Result<()> {
        self.log.push(format!("record {}", path.file_name().unwrap().to_string_lossy()));
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<()> {
        self.log.push("stop recording".into());
        Ok(())
    }

    fn drag(&mut self, from: (i32, i32), to: (i32, i32), hold_ms: u64, move_ms: u64) -> Result<()> {
        self.changed_since_mark = false;
        self.log.push(format!("drag {},{} -> {},{} hold {hold_ms}ms move {move_ms}ms", from.0, from.1, to.0, to.1));
        Ok(())
    }
    fn set_clipboard(&mut self, text: &str) -> Result<()> {
        self.log.push(format!("clipboard {text}"));
        Ok(())
    }
    fn open_notifications(&mut self) -> Result<()> {
        self.log.push("notifications".into());
        Ok(())
    }
    fn id(&self) -> String {
        "fake-1".into()
    }
}
