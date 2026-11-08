//! iOS simulators: found and driven through `xcrun simctl`, with an XCUITest agent
//! (agent/ios) on the simulator for what is on screen and the touches.
//!
//! The agent is a UI test bundle built for the simulator (`xcodebuild build-for-testing`);
//! VIRTUOSO_IOS_AGENT names its `.xctestrun`. `xcodebuild test-without-building` starts it on
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

fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
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
        let xctestrun = std::env::var("VIRTUOSO_IOS_AGENT")
            .context("set VIRTUOSO_IOS_AGENT to the iOS agent's .xctestrun (xcodebuild build-for-testing of agent/ios)")?;
        // A free port on the Mac is a free port on the simulator: they share the network.
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let token = new_token();
        let log = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.log"));
        let out = std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
        let mut xcodebuild = Command::new("xcodebuild")
            .args(["test-without-building", "-xctestrun", &xctestrun, "-destination", &format!("id={udid}")])
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
        Ok(ScreenReply::parse(&body, self.width, self.height)?.tree)
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

    fn open_link(&mut self, link: &str, _app: Option<&str>, _browser: bool) -> Result<()> {
        self.simctl(&["openurl", &self.udid, link]).map(|_| ())
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
