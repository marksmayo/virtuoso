//! What the runner needs from a device. Android implements it over the agent; tests use
//! [`crate::testing::FakeDevice`].

use crate::screen::ScreenTree;
use anyhow::{bail, Result};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Backspace,
    Back,
    Home,
    Lock,
    Tab,
    Escape,
    Space,
    Menu,
    Search,
    Power,
    VolumeUp,
    VolumeDown,
    Camera,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    DpadCenter,
    MediaPlayPause,
    MediaStop,
    MediaNext,
    MediaPrevious,
    MediaRewind,
    MediaFastForward,
}

impl Key {
    /// A Maestro key name (`Enter`, `Volume Up`, `Remote Dpad Up`...), any case and spacing.
    pub fn parse(name: &str) -> Option<Key> {
        let n: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let n = n.strip_prefix("remote").unwrap_or(&n);
        Some(match n {
            "enter" => Key::Enter,
            "backspace" | "delete" => Key::Backspace,
            "back" => Key::Back,
            "home" => Key::Home,
            "lock" => Key::Lock,
            "tab" => Key::Tab,
            "escape" => Key::Escape,
            "space" => Key::Space,
            "menu" => Key::Menu,
            "search" => Key::Search,
            "power" => Key::Power,
            "volumeup" => Key::VolumeUp,
            "volumedown" => Key::VolumeDown,
            "camera" => Key::Camera,
            "dpadup" => Key::DpadUp,
            "dpaddown" => Key::DpadDown,
            "dpadleft" => Key::DpadLeft,
            "dpadright" => Key::DpadRight,
            "dpadcenter" => Key::DpadCenter,
            "mediaplaypause" => Key::MediaPlayPause,
            "mediastop" => Key::MediaStop,
            "medianext" => Key::MediaNext,
            "mediaprevious" => Key::MediaPrevious,
            "mediarewind" => Key::MediaRewind,
            "mediafastforward" => Key::MediaFastForward,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    LandscapeLeft,
    LandscapeRight,
    UpsideDown,
}

impl Orientation {
    pub fn parse(s: &str) -> Option<Orientation> {
        Some(match s.to_ascii_uppercase().replace([' ', '-'], "_").as_str() {
            "PORTRAIT" => Orientation::Portrait,
            "LANDSCAPE_LEFT" | "LANDSCAPE" => Orientation::LandscapeLeft,
            "LANDSCAPE_RIGHT" => Orientation::LandscapeRight,
            "UPSIDE_DOWN" | "PORTRAIT_UPSIDE_DOWN" => Orientation::UpsideDown,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settled {
    /// The UI changed after the last action.
    pub changed: bool,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaunchOptions {
    pub clear_state: bool,
    pub stop_app: bool,
    pub permissions: Vec<(String, String)>,
    pub arguments: Vec<(String, String)>,
}

fn unsupported<T>(what: &str) -> Result<T> {
    bail!("{what} is not supported on this device")
}

pub trait Device {
    fn platform(&self) -> &'static str;
    fn screen(&mut self) -> Result<ScreenTree>;
    /// The screen once something may have changed since `current` (the last read), waiting
    /// at most `wait`: `current` itself when nothing did. A device that cannot tell reads
    /// again after a short pause.
    fn screen_after_change(&mut self, current: ScreenTree, wait: Duration) -> Result<ScreenTree> {
        drop(current);
        std::thread::sleep(wait.min(Duration::from_millis(10)));
        self.screen()
    }
    fn tap(&mut self, x: i32, y: i32) -> Result<()>;
    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()>;
    /// `fling`: lift at full speed so the content keeps moving; otherwise stop, then lift.
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, fling: bool) -> Result<()>;
    fn press_key(&mut self, key: Key) -> Result<()>;
    fn input_text(&mut self, text: &str) -> Result<()>;
    fn erase_text(&mut self, chars: usize) -> Result<()>;
    /// Starts a settle measurement without an action (waitForAnimationToEnd).
    fn mark(&mut self) -> Result<()>;
    /// Waits for the UI to stop changing after the last action. `no_change` is how long to
    /// wait for a reaction when nothing changes.
    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled>;
    fn launch_app(&mut self, app: &str, opts: &LaunchOptions) -> Result<()>;
    fn stop_app(&mut self, app: &str) -> Result<()>;
    fn kill_app(&mut self, app: &str) -> Result<()> {
        self.stop_app(app)
    }
    fn clear_state(&mut self, app: &str) -> Result<()>;

    fn open_link(&mut self, _link: &str, _app: Option<&str>, _browser: bool) -> Result<()> {
        unsupported("openLink")
    }
    fn hide_keyboard(&mut self) -> Result<()> {
        unsupported("hideKeyboard")
    }
    fn screenshot(&mut self) -> Result<Vec<u8>> {
        unsupported("screenshots")
    }
    fn set_orientation(&mut self, _o: Orientation) -> Result<()> {
        unsupported("setOrientation")
    }
    fn set_location(&mut self, _lat: f64, _lon: f64) -> Result<()> {
        unsupported("setLocation")
    }
    /// `None` toggles.
    fn set_airplane_mode(&mut self, _on: Option<bool>) -> Result<()> {
        unsupported("airplane mode")
    }
    fn set_permissions(&mut self, _app: &str, _perms: &[(String, String)]) -> Result<()> {
        unsupported("setPermissions")
    }
    fn add_media(&mut self, _files: &[&Path]) -> Result<()> {
        unsupported("addMedia")
    }
    /// `None` toggles.
    fn set_dark_mode(&mut self, _on: Option<bool>) -> Result<()> {
        unsupported("dark mode")
    }
    fn dark_mode(&mut self) -> Result<bool> {
        unsupported("dark mode")
    }
    fn start_recording(&mut self, _path: &Path) -> Result<()> {
        unsupported("recording")
    }
    fn stop_recording(&mut self) -> Result<()> {
        unsupported("recording")
    }
    /// Presses `from`, holds `hold_ms` so the item lifts, moves to `to` over `move_ms`, releases.
    fn drag(&mut self, _from: (i32, i32), _to: (i32, i32), _hold_ms: u64, _move_ms: u64) -> Result<()> {
        unsupported("dragAndDrop")
    }
    fn open_notifications(&mut self) -> Result<()> {
        unsupported("openNotifications")
    }
    /// Puts text on the device clipboard. virtuoso keeps its own copy for pasteText and
    /// maestro.copiedText, so a device without a clipboard loses nothing.
    fn set_clipboard(&mut self, _text: &str) -> Result<()> {
        Ok(())
    }
    fn clear_keychain(&mut self) -> Result<()> {
        Ok(())
    }
    /// Device id for MAESTRO_DEVICE_UDID.
    fn id(&self) -> String {
        String::new()
    }
}
