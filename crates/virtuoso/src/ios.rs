//! iOS simulators: found and driven through `xcrun simctl`, with an XCUITest agent
//! (agent/ios) on the simulator for what is on screen and the touches.
//!
//! The agent is a UI test bundle built for the simulator (`xcodebuild build-for-testing`):
//! the binary carries its sources and builds it once, or VIRTUOSO_IOS_AGENT names a built
//! one's `.xctestrun`. `xcodebuild test-without-building` starts it on
//! the simulator, where it serves the same wire protocol as the Android agent (see
//! [`crate::agent`]). The simulator shares the Mac's network, so the host connects to it on
//! 127.0.0.1 with no forwarding. Coordinates are in points, the screen's own units on iOS.

use crate::agent::AgentClient;
use crate::device::{Device, Key, LaunchOptions, Lift, Orientation, Settled};
use crate::screen::{ScreenReply, ScreenTree};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The booted iOS simulators in `xcrun simctl list devices -j` output (not watches or TVs).
pub fn booted_simulators(json: &str) -> Result<Vec<String>> {
    let v: Value = serde_json::from_str(json).context("simctl's device list")?;
    let mut out = Vec::new();
    for (runtime, sims) in v["devices"].as_object().into_iter().flatten() {
        if !runtime.contains(".iOS-") {
            continue;
        }
        for s in sims.as_array().into_iter().flatten() {
            if s["state"] == "Booted" {
                out.extend(s["udid"].as_str().map(String::from));
            }
        }
    }
    Ok(out)
}

include!(concat!(env!("OUT_DIR"), "/ios_agent.rs"));

/// The iOS agent's sources, path and contents, as the binary carries them.
pub fn agent_sources() -> &'static [(&'static str, &'static [u8])] {
    IOS_AGENT
}

/// Of the files `xcodebuild build-for-testing` leaves in the products folder, the simulator
/// build's test run description.
pub fn simulator_xctestrun<'a>(names: &[&'a str]) -> Option<&'a str> {
    names.iter().copied().find(|n| n.contains("iphonesimulator") && n.ends_with(".xctestrun"))
}

/// The booted iOS simulators on this Mac.
pub fn devices() -> Result<Vec<String>> {
    let out = Command::new("xcrun").args(["simctl", "list", "devices", "booted", "-j"]).output().context("running xcrun simctl (is Xcode installed?)")?;
    if !out.status.success() {
        bail!("xcrun simctl list: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    booted_simulators(&String::from_utf8_lossy(&out.stdout))
}

/// What the agent presses for a key; `None` for a key iOS does not have (Back, a TV remote's)
/// or the simulator cannot press (the volume buttons).
pub fn ios_key(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Home => "home",
        Key::Enter => "enter",
        Key::Backspace => "backspace",
        Key::Lock | Key::Power => "lock",
        Key::Tab => "tab",
        Key::Space => "space",
        _ => return None,
    })
}

/// A Maestro permission and value as a `simctl privacy` action and service; `None` for one
/// simctl cannot set (the camera and notifications are the user's to grant).
pub fn privacy(permission: &str, value: &str) -> Option<(&'static str, &'static str)> {
    let action = match value {
        "allow" => "grant",
        "deny" => "revoke",
        "unset" => "reset",
        _ => return None,
    };
    let service = match permission.to_ascii_lowercase().as_str() {
        "all" => "all",
        "location" => "location",
        "locationalways" | "location-always" => "location-always",
        "photos" => "photos",
        "photosadd" | "photos-add" => "photos-add",
        "contacts" => "contacts",
        "calendar" => "calendar",
        "reminders" => "reminders",
        "microphone" => "microphone",
        "motion" => "motion",
        "medialibrary" | "media-library" => "media-library",
        "siri" => "siri",
        _ => return None,
    };
    Some((action, service))
}

/// The Open button of the system's "Open in “App”?" prompt, which a link to another app
/// raises.
pub fn open_prompt_button(s: &ScreenTree) -> Option<(i32, i32)> {
    let sb = |e: &crate::screen::Element| e.package == "com.apple.springboard";
    s.elements.iter().find(|e| sb(e) && e.text.starts_with("Open in “"))?;
    s.elements.iter().find(|e| sb(e) && e.class == "Button" && e.text == "Open").map(|e| e.bounds.center())
}

fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// The agent's test run description: VIRTUOSO_IOS_AGENT's, else the agent this binary carries,
/// built for the simulator once (in ~/Library/Caches/virtuoso, by its sources' hash) and kept.
fn agent_xctestrun() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("VIRTUOSO_IOS_AGENT") {
        return Ok(PathBuf::from(p));
    }
    let mut hash: u64 = 0xcbf29ce484222325;
    for (path, bytes) in agent_sources() {
        for b in path.as_bytes().iter().chain(bytes.iter()) {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    let dir = PathBuf::from(home).join(format!("Library/Caches/virtuoso/ios-agent-{hash:016x}"));
    let products = dir.join("build/Build/Products");
    let built = || -> Option<PathBuf> {
        let names: Vec<String> = std::fs::read_dir(&products).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        simulator_xctestrun(&names).map(|n| products.join(n))
    };
    if let Some(x) = built() {
        return Ok(x);
    }
    let src = dir.join("src");
    for (path, bytes) in agent_sources() {
        let f = src.join(path);
        std::fs::create_dir_all(f.parent().unwrap())?;
        std::fs::write(&f, bytes).with_context(|| format!("writing {}", f.display()))?;
    }
    eprintln!("Building the iOS agent for the simulator (once; about a minute)...");
    let log = dir.join("build.log");
    let out = std::fs::File::create(&log)?;
    let status = Command::new("xcodebuild")
        .args(["build-for-testing", "-project"])
        .arg(src.join("VirtuosoAgent.xcodeproj"))
        .args(["-scheme", "VirtuosoAgent", "-destination", "generic/platform=iOS Simulator", "-derivedDataPath"])
        .arg(dir.join("build"))
        .arg("CODE_SIGNING_ALLOWED=NO")
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .status()
        .context("running xcodebuild (is Xcode installed?)")?;
    if !status.success() {
        bail!("building the iOS agent failed: see {}", log.display());
    }
    built().with_context(|| format!("the iOS agent built, but no .xctestrun is in {}", products.display()))
}

/// How long the agent may take to start: xcodebuild installs and launches the test runner.
const AGENT_START: Duration = Duration::from_secs(180);

/// The agent on the simulator: the xcodebuild that runs it, and the connection to it.
struct IosAgent {
    client: AgentClient,
    xcodebuild: Child,
}

impl IosAgent {
    fn start(udid: &str) -> Result<IosAgent> {
        let xctestrun = agent_xctestrun()?;
        let xctestrun = xctestrun.to_string_lossy();
        // A free port on the Mac is a free port on the simulator: they share the network.
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let token = new_token();
        let log = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}-{}.log", &token[..8]));
        let out = std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
        let mut xcodebuild = Command::new("xcodebuild")
            .args(["test-without-building", "-xctestrun", xctestrun.as_ref(), "-destination", &format!("id={udid}")])
            .arg("-only-testing:VirtuosoAgent/VirtuosoAgent/testServe")
            .env("TEST_RUNNER_VIRTUOSO_PORT", port.to_string())
            .env("TEST_RUNNER_VIRTUOSO_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .spawn()
            .context("starting xcodebuild (is Xcode installed?)")?;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let start = Instant::now();
        loop {
            if let Ok(mut client) = AgentClient::connect(addr, &token) {
                client.set_timeout(Some(Duration::from_secs(60)))?;
                if client.ping().is_ok() {
                    return Ok(IosAgent { client, xcodebuild });
                }
            }
            if let Some(status) = xcodebuild.try_wait()? {
                bail!("the iOS agent exited ({status}) before it answered: see {}", log.display());
            }
            if start.elapsed() > AGENT_START {
                let _ = xcodebuild.kill();
                bail!("the iOS agent did not answer in {} s: see {}", AGENT_START.as_secs(), log.display());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for IosAgent {
    fn drop(&mut self) {
        self.client.quit();
        // The test ends when the agent quits; xcodebuild follows within a few seconds.
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(Some(_)) = self.xcodebuild.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.xcodebuild.kill();
    }
}

pub struct IosDevice {
    udid: String,
    agent: IosAgent,
    width: i32,
    height: i32,
    recording: Option<Child>,
}

impl IosDevice {
    /// The whole tree of one app, in front or not (the home screen's, a system prompt's).
    pub fn app_screen(&mut self, app: &str) -> Result<ScreenTree> {
        let body = self.agent.client.call_raw(&json!({"op": "screen", "app": app}))?;
        Ok(ScreenReply::parse(&body, self.width, self.height)?.tree)
    }

    pub fn connect(udid: &str) -> Result<Self> {
        let mut agent = IosAgent::start(udid)?;
        let info = agent.client.call(json!({"op": "info"}))?;
        let (width, height) = (info["width"].as_i64().unwrap_or(0) as i32, info["height"].as_i64().unwrap_or(0) as i32);
        Ok(IosDevice { udid: udid.to_string(), agent, width, height, recording: None })
    }

    fn call(&mut self, req: Value) -> Result<Value> {
        self.agent.client.call(req)
    }

    fn simctl(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("xcrun").arg("simctl").args(args).output().context("running xcrun simctl")?;
        if !out.status.success() {
            bail!("simctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl Device for IosDevice {
    fn platform(&self) -> &'static str {
        "ios"
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        let body = self.agent.client.call_raw(&json!({"op": "screen"}))?;
        let mut tree = ScreenReply::parse(&body, self.width, self.height)?.tree;
        // The screen turns with the device: the app's own frame is its size now.
        if let Some(app) = tree.elements.iter().find(|e| e.depth == 0 && e.class == "Application" && e.bounds.area() > 0) {
            (self.width, self.height) = (app.bounds.width(), app.bounds.height());
            (tree.width, tree.height) = (self.width, self.height);
        }
        Ok(tree)
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<()> {
        self.call(json!({"op": "tap", "x": x, "y": y})).map(|_| ())
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.call(json!({"op": "press", "x": x, "y": y, "ms": ms})).map(|_| ())
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, end: Lift) -> Result<()> {
        self.call(json!({"op": "swipe", "x1": from.0, "y1": from.1, "x2": to.0, "y2": to.1, "ms": ms, "fling": end == Lift::Fling}))
            .map(|_| ())
    }

    fn press_key(&mut self, key: Key) -> Result<()> {
        let Some(name) = ios_key(key) else { bail!("{key:?} is not a key on iOS") };
        self.call(json!({"op": "key", "name": name})).map(|_| ())
    }

    fn input_text(&mut self, text: &str) -> Result<()> {
        self.call(json!({"op": "type", "text": text})).map(|_| ())
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        self.call(json!({"op": "erase", "count": chars})).map(|_| ())
    }

    fn mark(&mut self) -> Result<()> {
        Ok(())
    }

    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let r = self.call(json!({
            "op": "settle",
            "quiet": 300,
            "nochange": no_change.as_millis() as u64,
            "timeout": timeout.as_millis() as u64,
        }))?;
        Ok(Settled { changed: r["changed"] == true, timed_out: r["timedOut"] == true })
    }

    fn launch_app(&mut self, app: &str, o: &LaunchOptions) -> Result<()> {
        if o.clear_state {
            self.clear_state(app)?;
        }
        self.set_permissions(app, &o.permissions)?;
        let args: Vec<String> = o.arguments.iter().flat_map(|(k, v)| [format!("-{k}"), v.clone()]).collect();
        self.call(json!({"op": "launch", "app": app, "args": args, "activate": !o.stop_app && !o.clear_state})).map(|_| ())
    }

    fn stop_app(&mut self, app: &str) -> Result<()> {
        self.call(json!({"op": "terminate", "app": app})).map(|_| ())
    }

    fn clear_state(&mut self, app: &str) -> Result<()> {
        self.stop_app(app)?;
        let data = self.simctl(&["get_app_container", &self.udid, app, "data"])?;
        let data = PathBuf::from(data.trim());
        for dir in ["Documents", "Library", "tmp"] {
            let d = data.join(dir);
            if d.exists() {
                std::fs::remove_dir_all(&d).with_context(|| format!("clearing {}", d.display()))?;
                std::fs::create_dir_all(&d)?;
            }
        }
        Ok(())
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        Ok(self.agent.client.call_with_blob(json!({"op": "screenshot"}))?.1)
    }

    fn set_orientation(&mut self, o: Orientation) -> Result<()> {
        let v = match o {
            Orientation::Portrait => "portrait",
            Orientation::LandscapeLeft => "landscapeLeft",
            Orientation::LandscapeRight => "landscapeRight",
            Orientation::UpsideDown => "upsideDown",
        };
        self.call(json!({"op": "orientation", "value": v})).map(|_| ())
    }

    fn drag(&mut self, from: (i32, i32), to: (i32, i32), hold_ms: u64, move_ms: u64) -> Result<()> {
        self.call(json!({"op": "drag", "x1": from.0, "y1": from.1, "x2": to.0, "y2": to.1, "hold": hold_ms, "move": move_ms})).map(|_| ())
    }

    /// Pulled down from the top left, as a finger opens it (the top right is the control centre).
    fn open_notifications(&mut self) -> Result<()> {
        let (w, h) = (self.width, self.height);
        self.call(json!({"op": "swipe", "x1": w / 5, "y1": 0, "x2": w / 5, "y2": h * 7 / 10, "ms": 300, "fling": true, "system": true})).map(|_| ())
    }

    fn hide_keyboard(&mut self) -> Result<()> {
        self.call(json!({"op": "hideKeyboard"})).map(|_| ())
    }

    fn open_link(&mut self, link: &str, app: Option<&str>, browser: bool) -> Result<()> {
        // Into the flow's app, as XCUITest opens it: no prompt. A web link, or one for the
        // browser, goes to whatever handles it.
        let web = link.starts_with("http://") || link.starts_with("https://");
        if let (Some(app), false, false) = (app, browser, web) {
            return self.call(json!({"op": "openURL", "url": link, "app": app})).map(|_| ());
        }
        self.simctl(&["openurl", &self.udid, link])?;
        // A link to an app asks first ("Open in “App”?"), as Maestro answers it: Open.
        let end = Instant::now() + Duration::from_secs(8);
        while Instant::now() < end {
            if let Some((x, y)) = open_prompt_button(&self.screen()?) {
                return self.tap(x, y);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    fn set_location(&mut self, lat: f64, lon: f64) -> Result<()> {
        self.simctl(&["location", &self.udid, "set", &format!("{lat},{lon}")]).map(|_| ())
    }

    fn set_permissions(&mut self, app: &str, perms: &[(String, String)]) -> Result<()> {
        for (p, v) in perms {
            // What simctl cannot set (the camera, notifications) is left to the app to ask.
            if let Some((action, service)) = privacy(p, v) {
                self.simctl(&["privacy", &self.udid, action, service, app])?;
            }
        }
        Ok(())
    }

    fn add_media(&mut self, files: &[&Path]) -> Result<()> {
        let paths: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        let mut args = vec!["addmedia", self.udid.as_str()];
        args.extend(paths.iter().map(String::as_str));
        self.simctl(&args).map(|_| ())
    }

    fn set_dark_mode(&mut self, on: Option<bool>) -> Result<()> {
        let on = match on {
            Some(on) => on,
            None => !self.dark_mode()?,
        };
        self.simctl(&["ui", &self.udid, "appearance", if on { "dark" } else { "light" }]).map(|_| ())
    }

    fn dark_mode(&mut self) -> Result<bool> {
        Ok(self.simctl(&["ui", &self.udid, "appearance"])?.trim() == "dark")
    }

    fn start_recording(&mut self, path: &Path) -> Result<()> {
        let child = Command::new("xcrun")
            .args(["simctl", "io", &self.udid, "recordVideo", "--codec=h264", "--force"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting simctl recordVideo")?;
        self.recording = Some(child);
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<()> {
        let Some(mut child) = self.recording.take() else { return Ok(()) };
        // An interrupt, not a kill: simctl writes the video's index on the way out.
        let _ = Command::new("kill").args(["-INT", &child.id().to_string()]).status();
        child.wait()?;
        Ok(())
    }

    fn set_clipboard(&mut self, text: &str) -> Result<()> {
        use std::io::Write;
        let mut child = Command::new("xcrun").args(["simctl", "pbcopy", &self.udid]).stdin(Stdio::piped()).spawn()?;
        child.stdin.take().context("simctl pbcopy's input")?.write_all(text.as_bytes())?;
        if !child.wait()?.success() {
            bail!("simctl pbcopy failed");
        }
        Ok(())
    }

    fn clear_keychain(&mut self) -> Result<()> {
        self.simctl(&["keychain", &self.udid, "reset"]).map(|_| ())
    }

    fn id(&self) -> String {
        self.udid.clone()
    }
}
