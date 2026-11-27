//! What the runner needs from a device. Android implements it over the agent; tests use
//! [`crate::testing::FakeDevice`].

use crate::screen::ScreenTree;
use anyhow::{bail, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Whether the host's waits between two looks at a device are traced: VIRTUOSO_SLEEP_TRACE, or
/// the iOS trace (VIRTUOSO_IOS_TRACE). Says first, on a Mac, what the system makes of the
/// process: timers of one below the default quality of service are coalesced.
fn sleep_tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| sleep_trace_starts(std::env::var_os("VIRTUOSO_SLEEP_TRACE").is_some() || std::env::var_os("VIRTUOSO_IOS_TRACE").is_some()))
}

/// The trace's first line, when it is on: what the system makes of the process.
pub fn sleep_trace_starts(on: bool) -> bool {
    if on {
        println!("{}", quality_of_service());
    }
    on
}

/// This thread's and the main thread's quality-of-service class, and whether the process is
/// marked background, on a Mac.
#[cfg(target_os = "macos")]
pub fn quality_of_service() -> String {
    extern "C" {
        fn qos_class_self() -> u32;
        fn qos_class_main() -> u32;
    }
    let (own, main) = unsafe { (qos_class_self(), qos_class_main()) };
    let background = unsafe { libc::getpriority(libc::PRIO_DARWIN_PROCESS, 0) };
    format!(
        "    [sleep] quality of service: this thread {own:#x}, the main thread {main:#x} (0x21 user-interactive, 0x19 user-initiated, 0x15 default, 0x11 utility, 0x09 background, 0 unspecified); background process {background}"
    )
}

/// Asks the system to wake this thread when its waits end, and returns the class it now has. On
/// a CI Mac the process runs at utility quality of service, whose timers macOS coalesces: a
/// 10 ms wait took a median 65 ms, a 50 ms one up to 1.07 s (#133's trace). User-interactive
/// threads' timers are not coalesced. The waits and what they wait for are unchanged; threads
/// this one starts inherit the class.
#[cfg(target_os = "macos")]
pub fn prompt_wakeups() -> u32 {
    extern "C" {
        fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
        fn qos_class_self() -> u32;
    }
    const USER_INTERACTIVE: u32 = 0x21;
    unsafe {
        pthread_set_qos_class_self_np(USER_INTERACTIVE, 0);
        qos_class_self()
    }
}

/// Off a Mac nothing is asked: the class is 0.
#[cfg(not(target_os = "macos"))]
pub fn prompt_wakeups() -> u32 {
    0
}

#[cfg(not(target_os = "macos"))]
pub fn quality_of_service() -> String {
    "    [sleep] quality of service: not a Mac".into()
}

/// What a traced wait says: how long it asked for and how long it took.
pub fn sleep_trace(asked: Duration, took: Duration) -> String {
    format!("    [sleep] asked {} ms, took {:.1} ms", asked.as_millis(), took.as_secs_f64() * 1000.0)
}

/// The host's wait between two looks at a device: a sleep, with a line for the trace saying
/// how long it took (on a CI Mac a 10 ms sleep took ~60 ms).
pub fn pause(d: Duration) {
    pause_with(d, sleep_tracing(), Instant::now, |line| println!("{line}"));
}

/// `pause` with its parts: untraced, a sleep and nothing else (no clock read).
pub fn pause_with(d: Duration, traced: bool, mut clock: impl FnMut() -> Instant, mut say: impl FnMut(String)) {
    if !traced {
        std::thread::sleep(d);
        return;
    }
    let start = clock();
    std::thread::sleep(d);
    say(sleep_trace(d, clock().duration_since(start)));
}

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
    /// The keychain is reset first (iOS; nothing on Android).
    pub clear_keychain: bool,
    /// UIKit's animations run at a tenth of their length (iOS), as a run turns Android's off;
    /// `--keep-animations` leaves them.
    pub fast_animations: bool,
}

fn unsupported<T>(what: &str) -> Result<T> {
    bail!("{what} is not supported on this device")
}

/// How a swipe ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lift {
    /// Lifted at full speed: the content keeps moving.
    Fling,
    /// Held still, then lifted: the content moves by about the drag. A paging list snaps.
    Still,
    /// Cancelled where it is: no list flings, as one sometimes did on a still lift (React
    /// Native's), but a paging list does not snap either.
    Cancel,
}

/// An action a settle follows at once (`Device::act_and_settle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act<'a> {
    Tap(i32, i32),
    Swipe((i32, i32), (i32, i32), u64, Lift),
    Key(Key),
    Type(&'a str),
    Erase(usize),
}

/// `Device::await_keyboard` by whole reads of the screen past the cache.
pub fn await_keyboard_reading<D: Device + ?Sized>(dev: &mut D, wait: Duration, poll: Duration) -> Result<()> {
    use crate::screen::{EDITABLE, FOCUSED, KEYBOARD};
    let deadline = std::time::Instant::now() + wait;
    loop {
        let screen = dev.fresh_screen()?;
        let keyboard = screen.elements.iter().any(|e| e.has(KEYBOARD));
        let typing = screen.elements.iter().any(|e| e.has(FOCUSED) && e.has(EDITABLE));
        if keyboard || !typing || std::time::Instant::now() >= deadline {
            return Ok(());
        }
        pause(poll);
    }
}

pub trait Device {
    fn platform(&self) -> &'static str;
    fn screen(&mut self) -> Result<ScreenTree>;
    /// The screen once something may have changed since `current` (the last read), waiting
    /// at most `wait`: `current` itself when nothing did. A device that cannot tell reads
    /// again after a short pause.
    /// The screen read past any cache: bounds a layout change moved without an accessibility
    /// event (an animation run outside the app's own layout pass) are current.
    fn fresh_screen(&mut self) -> Result<ScreenTree> {
        self.screen()
    }
    /// The screen for an action's first look: one that held still (a settle's), else read
    /// afresh. A plain read can catch an element just appearing.
    fn screen_to_act_on(&mut self) -> Result<ScreenTree> {
        self.screen()
    }
    /// Taps (x, y) if the screen is still `screen` (nothing moved since the lookup): `None` once
    /// tapped, else the screen as it is now, untapped. A device that cannot check and tap in
    /// one request reads the screen afresh and leaves the tap to the caller.
    fn tap_if_unchanged(&mut self, screen: &ScreenTree, x: i32, y: i32) -> Result<Option<ScreenTree>> {
        let _ = (screen, x, y);
        Ok(Some(self.fresh_screen()?))
    }
    /// Whether the next step starts by reading the screen (a lookup): a device whose settle can
    /// bring the screen it settled on back with it does, for this step's settles, and gives it
    /// to that read.
    fn next_step_reads(&mut self, reads: bool) {
        let _ = reads;
    }
    /// Whether the next step types (a key, text or an erase): a device that hands input over in
    /// order need not wait for quiet between this step's typing and that one's.
    fn next_step_types(&mut self, _types: bool) {}
    /// The text the next step waits to see (a plain `assertVisible` or `extendedWaitUntil`): a
    /// settle may end once the screen shows it.
    fn next_step_expects(&mut self, _text: Option<&str>) {}
    /// `tap_if_unchanged`, and once tapped the settle after it: `Ok` with the settle's outcome,
    /// else `Err` with the screen as it is now, untapped. A device that can send the check, the
    /// tap and the settle as one request does.
    fn tap_if_unchanged_and_settle(&mut self, screen: &ScreenTree, x: i32, y: i32, no_change: Duration, timeout: Duration) -> Result<std::result::Result<Settled, ScreenTree>> {
        match self.tap_if_unchanged(screen, x, y)? {
            None => Ok(Ok(self.settle(no_change, timeout)?)),
            Some(now) => Ok(Err(now)),
        }
    }
    /// The screen's size, for where a swipe or a tap at a point goes. A device that keeps it
    /// (iOS: every read updates it) gives it without reading the whole screen.
    fn screen_rect(&mut self) -> Result<crate::screen::Rect> {
        Ok(self.screen()?.screen_rect())
    }
    fn screen_after_change(&mut self, current: ScreenTree, wait: Duration) -> Result<ScreenTree> {
        drop(current);
        pause(wait.min(Duration::from_millis(10)));
        self.screen()
    }
    fn tap(&mut self, x: i32, y: i32) -> Result<()>;
    /// Whether the last tap went in while the app was still animating (iOS: an endless one, or a
    /// popup presenting, held XCTest's idle wait to its cap): an app may take no touch then.
    fn touched_while_busy(&self) -> bool {
        false
    }
    /// `count` taps at a point, `interval` apart (a double tap, tapOn's repeat): one after
    /// another, unless the device plays them as one gesture.
    fn taps(&mut self, x: i32, y: i32, count: u32, interval: Duration) -> Result<()> {
        for n in 0..count.max(1) {
            if n > 0 {
                std::thread::sleep(interval);
            }
            self.tap(x, y)?;
        }
        Ok(())
    }
    /// The gap between repeated taps (a double tap, tapOn's repeat) when the flow names none.
    fn repeat_tap_interval(&self) -> Duration {
        Duration::from_millis(100)
    }
    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()>;
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, end: Lift) -> Result<()>;
    fn press_key(&mut self, key: Key) -> Result<()>;
    fn input_text(&mut self, text: &str) -> Result<()>;
    fn erase_text(&mut self, chars: usize) -> Result<()>;
    /// Starts a settle measurement without an action (waitForAnimationToEnd).
    fn mark(&mut self) -> Result<()>;
    /// Waits for the UI to stop changing after the last action. `no_change` is how long to
    /// wait for a reaction when nothing changes.
    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled>;
    /// The action, then the settle after it. A device whose agent takes requests in order can
    /// send both at once (iOS): the settle costs no round trip of its own.
    fn act_and_settle(&mut self, act: Act, no_change: Duration, timeout: Duration) -> Result<Settled> {
        match act {
            Act::Tap(x, y) => self.tap(x, y)?,
            Act::Swipe(from, to, ms, end) => self.swipe(from, to, ms, end)?,
            Act::Key(key) => self.press_key(key)?,
            Act::Type(text) => self.input_text(text)?,
            Act::Erase(chars) => self.erase_text(chars)?,
        }
        self.settle(no_change, timeout)
    }
    /// How long a scrollUntilVisible drag waits for the screen to stop changing before the
    /// next look. A device that sees loads itself (the web's tracker: requests, short timers,
    /// animations) needs none: the look waits for those, not for a list to stop re-rendering.
    /// Whether a scroll's drag can go any distance (a browser: a wheel, or a finger's drags sent
    /// together), from anywhere over the box, where elsewhere it has to fit on the screen.
    fn scrolls_any_distance(&self) -> bool {
        false
    }
    fn scroll_quiet(&self) -> Duration {
        Duration::from_millis(300)
    }
    fn launch_app(&mut self, app: &str, opts: &LaunchOptions) -> Result<()>;
    fn stop_app(&mut self, app: &str) -> Result<()>;
    fn kill_app(&mut self, app: &str) -> Result<()> {
        self.stop_app(app)
    }
    fn clear_state(&mut self, app: &str) -> Result<()>;

    fn open_link(&mut self, _link: &str, _app: Option<&str>, _browser: bool) -> Result<()> {
        unsupported("openLink")
    }
    /// openLink, then the settle after it. A device whose agent takes requests in order can
    /// send both at once (iOS, a link into the flow's app).
    fn open_link_and_settle(&mut self, link: &str, app: Option<&str>, browser: bool, no_change: Duration, timeout: Duration) -> Result<Settled> {
        self.open_link(link, app, browser)?;
        self.settle(no_change, timeout)
    }
    /// A field just tapped has focus before its keyboard is up: while one has focus and no
    /// keyboard shows, wait (up to `wait`) for it, so hideKeyboard does not find none and leave
    /// the keyboard coming up. Read past the cache, every `poll`.
    fn await_keyboard(&mut self, wait: Duration, poll: Duration) -> Result<()> {
        await_keyboard_reading(self, wait, poll)
    }
    fn hide_keyboard(&mut self) -> Result<()> {
        unsupported("hideKeyboard")
    }
    /// hideKeyboard, and on a device that settles after it, the settle in the same request
    /// (iOS: the next tap then starts on a screen that held still). Otherwise hideKeyboard
    /// alone: it returns once the keyboard has gone.
    fn hide_keyboard_and_settle(&mut self, no_change: Duration, timeout: Duration) -> Result<()> {
        let _ = (no_change, timeout);
        self.hide_keyboard()
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
    /// `launch_app` sets the launch's own permissions (`LaunchOptions::permissions`) itself,
    /// alongside its other steps; else they are set before it, with `set_permissions`.
    fn launch_sets_permissions(&self) -> bool {
        false
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
    /// Closes the device at the run's end: a browser virtuoso launched is stopped, and how its
    /// start and stop went is the answer. `None` for every other device (closed when dropped).
    fn close(&mut self) -> Option<crate::phases::Phases> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::Device;

    #[test]
    fn a_device_other_than_a_launched_browser_has_no_phases_to_tell_at_its_close() {
        let mut d = crate::testing::FakeDevice::new();
        assert!(d.close().is_none());
    }
}
