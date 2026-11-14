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
use crate::device::{Act, Device, Key, LaunchOptions, Lift, Orientation, Settled};
use crate::screen::{ScreenReply, ScreenTree};
use anyhow::{anyhow, bail, Context, Result};
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

/// The hash of those sources (FNV-1a over each path and its bytes, in order), worked out when
/// virtuoso is built: it names the agent's build.
pub fn agent_sources_hash() -> u64 {
    IOS_AGENT_HASH
}

/// Of the files `xcodebuild build-for-testing` leaves in the products folder, the simulator
/// build's test run description.
pub fn simulator_xctestrun<'a>(names: &[&'a str]) -> Option<&'a str> {
    names.iter().copied().find(|n| n.contains("iphonesimulator") && n.ends_with(".xctestrun"))
}

/// simctl in a developer folder (Xcode.app/Contents/Developer), when it is there.
pub fn simctl_in(developer_dir: &Path) -> Option<PathBuf> {
    let p = developer_dir.join("usr/bin/simctl");
    p.is_file().then_some(p)
}

/// The developer folder in use: DEVELOPER_DIR's, else xcode-select's.
fn developer_dir() -> Option<PathBuf> {
    std::env::var_os("DEVELOPER_DIR").map(PathBuf::from).or_else(|| std::fs::read_link("/var/db/xcode_select_link").ok())
}

/// Where the agent built from sources hashing to `sources_hash` is kept: the Xcode that builds
/// it is part of its name (another Xcode's runner and XCTest are another build).
pub fn agent_cache_dir(home: &Path, sources_hash: u64, developer_dir: Option<&Path>) -> PathBuf {
    let mut hash = sources_hash;
    for b in developer_dir.map(|d| d.to_string_lossy().into_owned()).unwrap_or_default().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    home.join(format!("Library/Caches/virtuoso/ios-agent-{hash:016x}"))
}

/// xcodebuild's arguments for building the agent from `src` into `build`: for the simulator
/// of this Mac's architecture (`arch`, Rust's name for it) alone, with no debug info (nothing
/// reads it, and the runner installed at each agent start is smaller) or index store, and a
/// module cache shared by every build in `cache`.
pub fn agent_build_args(src: &Path, build: &Path, cache: &Path, arch: &str) -> Vec<String> {
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    vec![
        "build-for-testing".into(),
        "-project".into(),
        src.join("VirtuosoAgent.xcodeproj").to_string_lossy().into_owned(),
        "-scheme".into(),
        "VirtuosoAgent".into(),
        "-destination".into(),
        "generic/platform=iOS Simulator".into(),
        "-derivedDataPath".into(),
        build.to_string_lossy().into_owned(),
        "CODE_SIGNING_ALLOWED=NO".into(),
        format!("ARCHS={arch}"),
        "ONLY_ACTIVE_ARCH=YES".into(),
        "DEBUG_INFORMATION_FORMAT=dwarf".into(),
        "GCC_GENERATE_DEBUGGING_SYMBOLS=NO".into(),
        "COMPILER_INDEX_STORE_ENABLE=NO".into(),
        format!("MODULE_CACHE_DIR={}", cache.join("ModuleCache").to_string_lossy().replace('\\', "/")),
    ]
}

/// xcodebuild's arguments for running the agent's one test on simulator `udid`.
pub fn agent_test_args(xctestrun: &str, udid: &str) -> Vec<String> {
    vec![
        "test-without-building".into(),
        "-xctestrun".into(),
        xctestrun.into(),
        "-destination".into(),
        format!("id={udid}"),
        "-only-testing:VirtuosoAgent/VirtuosoAgent/testServe".into(),
        "-parallel-testing-enabled".into(),
        "NO".into(),
        "-test-timeouts-enabled".into(),
        "NO".into(),
    ]
}

static SIMCTL_RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many simctl processes this process has started: each is a process and a CoreSimulator
/// round trip (~0.1-0.3 s), what host-side savings are counted in.
pub fn simctl_runs() -> u64 {
    SIMCTL_RUNS.load(std::sync::atomic::Ordering::Relaxed)
}

/// A simctl command: simctl itself from the developer folder (DEVELOPER_DIR, else
/// xcode-select's), found once; `xcrun simctl` where it is not (a process more and an SDK
/// lookup on every call).
fn simctl_command() -> Command {
    SIMCTL_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    static SIMCTL: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    let found = SIMCTL.get_or_init(|| developer_dir().and_then(|d| simctl_in(&d)));
    match found {
        Some(p) => Command::new(p),
        None => {
            let mut c = Command::new("xcrun");
            c.arg("simctl");
            c
        }
    }
}

/// `simctl <args>`'s output, or its error; timed for VIRTUOSO_IOS_TRACE.
fn run_simctl(args: &[&str]) -> Result<String> {
    let start = Instant::now();
    let out = simctl_command().args(args).output().context("running xcrun simctl")?;
    if tracing() {
        trace(&format!("simctl {}", args.first().unwrap_or(&"")), start);
    }
    if !out.status.success() {
        bail!("simctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Where an app's data folder was last found on a simulator (`simctl get_app_container`
/// takes a process and a CoreSimulator round trip): kept between runs, in a temp file.
fn container_note(udid: &str, app: &str) -> PathBuf {
    std::env::temp_dir().join(format!("virtuoso-ios-container-{udid}-{app}"))
}

pub fn remember_container(udid: &str, app: &str, data: &Path) {
    let _ = std::fs::write(container_note(udid, app), data.to_string_lossy().as_bytes());
}

/// The data folder remembered for the app, while it is still there (an uninstall removes it).
pub fn remembered_container(udid: &str, app: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::fs::read_to_string(container_note(udid, app)).ok()?.trim());
    p.is_dir().then_some(p)
}

/// A simulator's own data folder, where it keeps its permission grants among the rest.
fn simulator_data(udid: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{home}/Library/Developer/CoreSimulator/Devices/{udid}/data"))
}

/// The files a simulator keeps its permission grants in (in a simulator data folder): tccd's
/// database and its log, and locationd's clients.
const PERMISSION_STORES: [&str; 3] = ["Library/TCC/TCC.db", "Library/TCC/TCC.db-wal", "Library/Caches/locationd/clients.plist"];

/// The permission stores as they are now (each one's size and time written), and the app's data
/// folder as remembered (a reinstall moves it): any grant or revoke since, by whatever means,
/// changes it.
pub fn permission_fingerprint(sim_data: &Path, udid: &str, app: &str) -> String {
    let mut fp = String::new();
    for store in PERMISSION_STORES {
        match std::fs::metadata(sim_data.join(store)) {
            Ok(m) => {
                let at = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).unwrap_or_default();
                fp += &format!("{} {} {};", m.len(), at.as_secs(), at.subsec_nanos());
            }
            Err(_) => fp += "-;",
        }
    }
    fp += &remembered_container(udid, app).map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    fp
}

/// Where the permissions a launch last granted an app are noted, with the stores' fingerprint
/// right after: kept between runs, in a temp file, as the data folder is.
fn grant_note(udid: &str, app: &str) -> PathBuf {
    std::env::temp_dir().join(format!("virtuoso-ios-granted-{udid}-{app}"))
}

fn grant_note_text(perms: &[(String, String)], fingerprint: &str) -> String {
    let perms: Vec<String> = perms.iter().map(|(p, v)| format!("{p}={v}")).collect();
    format!("{}\n{fingerprint}", perms.join(","))
}

pub fn remember_grant(udid: &str, app: &str, perms: &[(String, String)], fingerprint: &str) {
    let _ = std::fs::write(grant_note(udid, app), grant_note_text(perms, fingerprint));
}

/// A launch granted the app `perms` and the stores are untouched since (`fingerprint` is
/// theirs now): the grant stands, and need not be made again.
pub fn granted_already(udid: &str, app: &str, perms: &[(String, String)], fingerprint: &str) -> bool {
    std::fs::read_to_string(grant_note(udid, app)).is_ok_and(|t| t == grant_note_text(perms, fingerprint))
}

/// The flow set the app's permissions itself: the next launch grants its own again.
pub fn forget_grant(udid: &str, app: &str) {
    let _ = std::fs::remove_file(grant_note(udid, app));
}

/// The booted iOS simulators on this Mac.
pub fn devices() -> Result<Vec<String>> {
    let out = simctl_command().args(["list", "devices", "booted", "-j"]).output().context("running xcrun simctl (is Xcode installed?)")?;
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

/// The last screen read, for the next command's first look: until an action (simctl's too),
/// and while it is fresh. A settle's held still, so it stands a second; a plain read 300 ms
/// (read-only commands in a row share it). A wait for a change reads afresh
/// (`screen_after_change`).
/// An action's first look takes only a settle's screen (`get_settled`): a plain read can catch
/// an element just appearing, and a modal still fading in takes no touch.
/// Once cleared, the last screen read is still known (`last_seq`), not given: a settle that ends
/// on it again gets no tree back, and takes it up again (`renew`).
#[derive(Default)]
pub struct SettledScreen(Option<(ScreenTree, Instant, bool)>, bool);

impl SettledScreen {
    /// A settle's screen.
    pub fn put(&mut self, tree: ScreenTree, at: Instant) {
        *self = SettledScreen(Some((tree, at, true)), true);
    }
    /// A plain read's: as a settle's taken 700 ms earlier, so it stands 300 ms.
    pub fn put_read(&mut self, tree: ScreenTree, at: Instant) {
        *self = SettledScreen(Some((tree, at.checked_sub(Duration::from_millis(700)).unwrap_or(at), false)), true);
    }
    pub fn get(&self, now: Instant) -> Option<ScreenTree> {
        self.0.as_ref().filter(|(_, at, _)| self.1 && now.duration_since(*at) < Duration::from_secs(1)).map(|(t, _, _)| t.clone())
    }
    pub fn get_settled(&self, now: Instant) -> Option<ScreenTree> {
        self.0.as_ref().filter(|(_, _, settled)| *settled).and_then(|_| self.get(now))
    }
    pub fn clear(&mut self) {
        self.1 = false;
    }
    /// The number of the last screen read, cleared or not.
    pub fn last_seq(&self) -> Option<u64> {
        self.0.as_ref().and_then(|(t, _, _)| t.seq)
    }
    /// A settle ended on the last screen read (`seq`): it is the settle's screen again.
    pub fn renew(&mut self, seq: Option<u64>, at: Instant) -> bool {
        match self.0.take() {
            Some((tree, _, _)) if seq.is_some() && tree.seq == seq => {
                self.put(tree, at);
                true
            }
            kept => {
                self.0 = kept;
                false
            }
        }
    }
}

/// The process `simctl launch` started, from its output (`<bundle id>: <pid>`).
pub fn launched_pid(out: &str) -> Option<i64> {
    out.lines().find_map(|l| l.rsplit_once(": ").and_then(|(_, pid)| pid.trim().parse().ok()))
}

/// The permissions an app is launched with: as in Maestro, every one unless the flow says
/// otherwise.
pub fn launch_permissions(perms: &[(String, String)]) -> Vec<(String, String)> {
    if perms.is_empty() {
        vec![("all".to_string(), "allow".to_string())]
    } else {
        perms.to_vec()
    }
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

/// A `simctl privacy` call: its action and service.
pub type PrivacyCall = (&'static str, &'static str);

/// The `simctl privacy` calls for `perms`, in two parts: those up to the last `all`, one after
/// another (`all` covers every service, so what came before it must go first); then each
/// service's own calls in order, the services side by side (one call is ~0.3 s). Location and
/// location-always are one setting in locationd: they stay in one line.
pub fn privacy_steps(perms: &[(String, String)]) -> (Vec<PrivacyCall>, Vec<Vec<PrivacyCall>>) {
    let steps: Vec<_> = perms.iter().filter_map(|(p, v)| privacy(p, v)).collect();
    let split = steps.iter().rposition(|(_, s)| *s == "all").map_or(0, |i| i + 1);
    let line = |s: &str| if s == "location-always" { "location" } else { s }.to_string();
    let mut apart: Vec<(String, Vec<PrivacyCall>)> = Vec::new();
    for &(action, service) in &steps[split..] {
        match apart.iter_mut().find(|(l, _)| *l == line(service)) {
            Some((_, calls)) => calls.push((action, service)),
            None => apart.push((line(service), vec![(action, service)])),
        }
    }
    (steps[..split].to_vec(), apart.into_iter().map(|(_, calls)| calls).collect())
}

/// The permissions `simctl privacy` cannot set, by their Maestro names and TCC services.
const TCC_SERVICES: &[(&str, &str)] = &[
    ("camera", "kTCCServiceCamera"),
    ("faceid", "kTCCServiceFaceID"),
    ("speech", "kTCCServiceSpeechRecognition"),
    ("usertracking", "kTCCServiceUserTracking"),
    ("homekit", "kTCCServiceWillow"),
];

/// SQL for the simulator's TCC database (as applesimutils writes it) setting the permissions
/// simctl cannot (the camera...): allow, deny or unset each; `None` when there are none.
pub fn tcc_sql(app: &str, perms: &[(String, String)]) -> Option<String> {
    let mut wanted: Vec<(&str, &str)> = Vec::new();
    for (p, v) in perms {
        let p = p.to_ascii_lowercase();
        for &(name, service) in TCC_SERVICES {
            if p == "all" || p == name {
                wanted.retain(|(s, _)| *s != service);
                wanted.push((service, v.as_str()));
            }
        }
    }
    let client = app.replace('\'', "''");
    let mut sql = String::new();
    for (service, value) in wanted {
        let auth = match value {
            "allow" => Some(2),
            "deny" => Some(0),
            "unset" => None,
            _ => continue,
        };
        sql += &format!("DELETE FROM access WHERE service = '{service}' AND client = '{client}' AND client_type = 0;");
        if let Some(auth) = auth {
            sql += &format!(
                "INSERT INTO access (service, client, client_type, auth_value, auth_reason, auth_version, flags) VALUES ('{service}', '{client}', 0, {auth}, 2, 1, 0);"
            );
        }
    }
    (!sql.is_empty()).then(|| format!("BEGIN;{sql}COMMIT;"))
}

/// The Open button of the system's "Open in “App”?" prompt, which a link to another app
/// raises.
pub fn open_prompt_button(s: &ScreenTree) -> Option<(i32, i32)> {
    let sb = |e: &crate::screen::Element| &*e.package == "com.apple.springboard";
    s.elements.iter().find(|e| sb(e) && e.text.starts_with("Open in “"))?;
    s.elements.iter().find(|e| sb(e) && &*e.class == "Button" && e.text == "Open").map(|e| e.bounds.center())
}

/// The app in front, by its bundle id: the screen's top-level app that is not SpringBoard's
/// (`None` on the home screen).
pub fn front_app(s: &ScreenTree) -> Option<&str> {
    s.elements.iter().find(|e| e.depth == 0 && &*e.class == "Application" && &*e.package != "com.apple.springboard").map(|e| &*e.package)
}

fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// The build an agent's state file names (`saved`), while it is still a simulator build in
/// `products` (this binary's sources, this Xcode): no listing of the folder needed to find it.
pub fn saved_xctestrun(products: &Path, saved: &str) -> Option<PathBuf> {
    let p = Path::new(saved);
    let name = p.file_name()?.to_str()?;
    (p.parent() == Some(products) && simulator_xctestrun(&[name]).is_some() && p.is_file()).then(|| p.to_path_buf())
}

/// The agent's test run description, built now if it is not yet (`virtuoso --platform ios
/// doctor`): no simulator needed.
pub fn built_agent() -> Result<PathBuf> {
    agent_xctestrun(None)
}

/// The agent's test run description: VIRTUOSO_IOS_AGENT's, else the agent this binary carries,
/// built for the simulator once (in ~/Library/Caches/virtuoso, by its sources' hash) and kept;
/// `saved` is the build the last run's agent was (`saved_xctestrun`).
fn agent_xctestrun(saved: Option<&str>) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("VIRTUOSO_IOS_AGENT") {
        return Ok(PathBuf::from(p));
    }
    let hash = agent_sources_hash();
    let home = PathBuf::from(std::env::var("HOME").context("HOME is not set")?);
    let dir = agent_cache_dir(&home, hash, developer_dir().as_deref());
    let products = dir.join("build/Build/Products");
    if let Some(x) = saved.and_then(|s| saved_xctestrun(&products, s)) {
        return Ok(x);
    }
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
        .args(agent_build_args(&src, &dir.join("build"), &home.join("Library/Caches/virtuoso"), std::env::consts::ARCH))
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

/// How long the agent may take to start: xcodebuild installs and launches the test runner. On
/// a simulator just booted on CI, the first start has taken over 300 s.
const AGENT_START: Duration = Duration::from_secs(600);

/// The agent on the simulator: the xcodebuild that runs it, and the connection to it.
/// The agent on the simulator, and the connection to it. The agent outlives the run: xcodebuild
/// takes most of half a minute to start one, so the next run on the simulator reconnects to it
/// (its port and token in a state file), as speedrun keeps its own agent up between runs.
struct IosAgent {
    client: AgentClient,
    /// The agent's `info`, when reconnecting already asked for it (it proves the agent alive).
    info: Option<Value>,
}

/// Where a simulator's running agent is recorded: its port, token, and which build it is.
fn agent_state(udid: &str) -> PathBuf {
    std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"))
}

/// An app installed on the simulator, through simctl found once (`xcrun` looks up the SDK).
pub fn install(udid: &str, app: &Path) -> Result<()> {
    let out = simctl_command().args(["install", udid]).arg(app).output().context("running simctl install")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// The connections `agent_answers` found agents on, by simulator, with the state each read and
/// the agent's `info`: a run's device goes on its simulator's (the agent serves one connection
/// at a time). One per simulator: `--device A,B` checks both before either connects.
static ANSWERED: std::sync::Mutex<Vec<(String, AgentClient, Value, Value)>> = std::sync::Mutex::new(Vec::new());

/// The simulator's agent, recorded by an earlier run, answers: the simulator is booted.
pub fn agent_answers(udid: &str) -> bool {
    let Some(saved) = std::fs::read_to_string(agent_state(udid)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return false };
    let port = saved["port"].as_u64().unwrap_or(0) as u16;
    let Ok(mut client) = AgentClient::connect(SocketAddr::from(([127, 0, 0, 1], port)), saved["token"].as_str().unwrap_or("")) else { return false };
    // Its `info` proves it alive as a ping would, and is what the run asks for first.
    let info = client.set_timeout(Some(Duration::from_secs(2))).ok().and_then(|_| client.call(json!({"op": "info"})).ok());
    let Some(info) = info else { return false };
    let mut answered = ANSWERED.lock().unwrap_or_else(|e| e.into_inner());
    answered.retain(|(u, _, _, _)| u != udid);
    answered.push((udid.to_string(), client, saved, info));
    true
}

/// Whether xcodebuild's log says it launched the agent's test runner before the simulator
/// knew of it (just installed, just booted): a second try works.
pub fn agent_launch_raced(log: &str) -> bool {
    log.contains("is unknown to FrontBoard")
}

impl IosAgent {
    fn start(udid: &str) -> Result<IosAgent> {
        let state = agent_state(udid);
        // The connection the device check just found the agent on; else the state file the last
        // run left (read once).
        let answered = {
            let mut all = ANSWERED.lock().unwrap_or_else(|e| e.into_inner());
            all.iter().position(|(u, _, _, _)| u == udid).map(|i| all.swap_remove(i))
        };
        let saved_state = match answered {
            Some(_) => None,
            None => std::fs::read_to_string(&state).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()),
        };
        let saved_build = answered.as_ref().map(|(_, _, saved, _)| saved).or(saved_state.as_ref()).and_then(|s| s["build"].as_str());
        let xctestrun = agent_xctestrun(saved_build)?;
        let build = xctestrun.to_string_lossy().into_owned();
        let port_of = |saved: &Value| SocketAddr::from(([127, 0, 0, 1], saved["port"].as_u64().unwrap_or(0) as u16));
        match answered {
            Some((_, mut client, saved, info)) if saved["build"] == build.as_str() => {
                client.set_timeout(Some(Duration::from_secs(60)))?;
                return Ok(IosAgent { client, info: Some(info) });
            }
            // The check's agent is another build's: stopped on the check's own connection.
            Some((_, client, saved, _)) => stop_agent(client, port_of(&saved)),
            // The agent the last run left, if it is this build's and still answers.
            None => {
                if let Some(saved) = saved_state {
                    let addr = port_of(&saved);
                    if let Ok(mut client) = AgentClient::connect(addr, saved["token"].as_str().unwrap_or("")) {
                        client.set_timeout(Some(Duration::from_secs(10)))?;
                        if saved["build"] == build.as_str() {
                            if let Ok(info) = client.call(json!({"op": "info"})) {
                                client.set_timeout(Some(Duration::from_secs(60)))?;
                                return Ok(IosAgent { client, info: Some(info) });
                            }
                        }
                        stop_agent(client, addr);
                    }
                }
            }
        }
        match Self::launch(udid, &build, &state) {
            Err(e) if e.downcast_ref::<LaunchRaced>().is_some() => {
                std::thread::sleep(Duration::from_secs(2));
                Self::launch(udid, &build, &state)
            }
            r => r,
        }
    }

    /// The agent started afresh by xcodebuild, waited for until it answers.
    fn launch(udid: &str, build: &str, state: &Path) -> Result<IosAgent> {
        // A free port on the Mac is a free port on the simulator: they share the network.
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let token = new_token();
        let log = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}-{}.log", &token[..8]));
        let out = std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
        // Not waited for: it runs the agent after this run ends.
        let mut xcodebuild = Command::new("xcodebuild")
            .args(agent_test_args(build, udid))
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
                // Its `info` proves it up as a ping would, and connect asks for it next.
                if let Ok(info) = client.call(json!({"op": "info"})) {
                    let saved = json!({"port": port, "token": token, "build": build});
                    std::fs::write(state, saved.to_string()).with_context(|| format!("writing {}", state.display()))?;
                    return Ok(IosAgent { client, info: Some(info) });
                }
            }
            if let Some(status) = xcodebuild.try_wait()? {
                let e = anyhow!("the iOS agent exited ({status}) before it answered: see {}", log.display());
                if agent_launch_raced(&std::fs::read_to_string(&log).unwrap_or_default()) {
                    return Err(e.context(LaunchRaced));
                }
                return Err(e);
            }
            if start.elapsed() > AGENT_START {
                let _ = xcodebuild.kill();
                bail!("the iOS agent did not answer in {} s: see {}", AGENT_START.as_secs(), log.display());
            }
            // A refused connection on the Mac costs nothing: looked for often.
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Another build's agent (or one that no longer answers) told to go, so two do not drive one
/// simulator. Gone when its port closes (its test process has ended), within 2 s.
fn stop_agent(mut client: AgentClient, addr: SocketAddr) {
    client.quit();
    drop(client);
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end && std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The agent's first launch raced the simulator (`agent_launch_raced`).
#[derive(Debug)]
struct LaunchRaced;

impl std::fmt::Display for LaunchRaced {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("the simulator did not know the agent yet")
    }
}

pub struct IosDevice {
    udid: String,
    agent: IosAgent,
    width: i32,
    height: i32,
    recording: Option<Child>,
    /// The permissions launchApp last set for each app.
    launch_permissions: std::collections::HashMap<String, Vec<(String, String)>>,
    /// The screen the last settle ended on (cleared by every action, simctl's too).
    settled: std::cell::RefCell<SettledScreen>,
    /// The appearance this run set (dark or not), until an action or another simctl call: a
    /// touch in Settings can change it.
    appearance: std::cell::Cell<Option<bool>>,
    /// The last screen given out was the kept one (it can be most of a second old): a wait
    /// from it reads at once.
    gave_kept: std::cell::Cell<bool>,
    /// The wait for an app just launched to be in front: made by the next agent request (what
    /// comes between, simctl's or the host's, need not wait for it).
    due: Option<Value>,
    trimmed: bool,
    /// The last tap went in while the app was still animating (`ScreenReply::busy`).
    busy: bool,
    /// With VIRTUOSO_IOS_TRACE, what each launch did with its default grant: printed at the end.
    grant_trace: Vec<String>,
}

/// With VIRTUOSO_IOS_TRACE, one line at the end of the run: the simctl processes it started,
/// what became of each launch's default grant, and its CPU time apart from its children's (the
/// benchmark's wait4 counts both: every simctl and sqlite3 the run waited for is in it).
impl Drop for IosDevice {
    fn drop(&mut self) {
        if !tracing() {
            return;
        }
        let mut grants = if self.grant_trace.is_empty() { "none".to_string() } else { self.grant_trace.join("; ") };
        // Whether the stores moved after this run's grants were noted (a late write by tccd or
        // locationd makes the next run grant again).
        let sim_data = simulator_data(&self.udid);
        for (app, perms) in &self.launch_permissions {
            let now = permission_fingerprint(&sim_data, &self.udid, app);
            let still = granted_already(&self.udid, app, perms, &now);
            grants += &format!("; at exit {app} note {}", if still { "still matches".to_string() } else { format!("stale (now {now:?})") });
        }
        println!("    [ios] exit: {} simctl runs; default grants: {grants}; {}", simctl_runs(), cpu_split());
    }
}

/// This process's CPU time (user + system, ms) and its reaped children's.
#[cfg(unix)]
fn cpu_split() -> String {
    let ms = |who| {
        // SAFETY: rusage is plain data, filled by getrusage for this process.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(who, &mut u) };
        let t = |v: libc::timeval| v.tv_sec as i64 * 1000 + v.tv_usec as i64 / 1000;
        (t(u.ru_utime), t(u.ru_stime), u.ru_maxrss as i64)
    };
    let (su, ss, srss) = ms(libc::RUSAGE_SELF);
    let (cu, cs, crss) = ms(libc::RUSAGE_CHILDREN);
    format!("cpu own {} ms ({su} user + {ss} sys, maxrss {srss}), children {} ms ({cu} user + {cs} sys, maxrss {crss})", su + ss, cu + cs)
}

#[cfg(not(unix))]
fn cpu_split() -> String {
    String::new()
}

/// The agent request for an action.
fn action_request(act: Act) -> Result<Value> {
    Ok(match act {
        Act::Tap(x, y) => json!({"op": "tap", "x": x, "y": y}),
        Act::Swipe(from, to, ms, end) => {
            json!({"op": "swipe", "x1": from.0, "y1": from.1, "x2": to.0, "y2": to.1, "ms": ms, "fling": end == Lift::Fling})
        }
        Act::Key(key) => {
            let Some(name) = ios_key(key) else { bail!("{key:?} is not a key on iOS") };
            json!({"op": "key", "name": name})
        }
        Act::Type(text) => json!({"op": "type", "text": text}),
        Act::Erase(chars) => json!({"op": "erase", "count": chars}),
    })
}

/// A settle; `held` is the screen the host holds (its read's number): ending on it, the agent
/// sends no tree.
fn settle_request(no_change: Duration, timeout: Duration, held: Option<u64>) -> Value {
    let mut req = json!({"op": "settle", "nochange": no_change.as_millis() as u64, "timeout": timeout.as_millis() as u64});
    if let Some(seq) = held {
        req["ifNot"] = seq.into();
    }
    req
}

impl IosDevice {
    /// The whole tree of one app, in front or not (the home screen's, a system prompt's).
    pub fn app_screen(&mut self, app: &str) -> Result<ScreenTree> {
        self.pay_due()?;
        let body = self.agent.client.call_raw(&json!({"op": "screen", "app": app}))?;
        let tree = ScreenReply::parse(&body, self.width, self.height)?.tree;
        self.agent.client.recycle(body);
        Ok(tree)
    }

    /// The app in front by its bundle id, as the agent finds it, with no read of its tree;
    /// `None` when the agent does not say.
    fn front_package(&mut self) -> Result<Option<String>> {
        Ok(self.call(json!({"op": "front"}))?["app"].as_str().map(String::from))
    }

    /// The screen read afresh, unless it is still the read `seq` names: `None` then (the agent
    /// sends no tree).
    pub fn screen_unless(&mut self, seq: u64) -> Result<Option<ScreenTree>> {
        let body = self.call_raw(&json!({"op": "screen", "ifNot": seq}))?;
        let r = ScreenReply::parse(&body, self.width, self.height)?;
        self.agent.client.recycle(body);
        trace_agent(&r);
        if r.unchanged {
            return Ok(None);
        }
        self.trimmed = r.trimmed;
        Ok(Some(self.sized(r.tree)))
    }

    /// The screen from XCTest's whole snapshot, every attribute fetched: what a read of only
    /// the attributes a screen uses must agree with.
    pub fn full_screen(&mut self) -> Result<ScreenTree> {
        let body = self.call_raw(&json!({"op": "screen", "full": true}))?;
        let r = ScreenReply::parse(&body, self.width, self.height)?;
        self.agent.client.recycle(body);
        Ok(self.sized(r.tree))
    }

    /// The last read fetched only the attributes a screen uses.
    pub fn trimmed(&self) -> bool {
        self.trimmed
    }

    pub fn connect(udid: &str) -> Result<Self> {
        let mut agent = IosAgent::start(udid)?;
        let info = match agent.info.take() {
            Some(info) => info,
            None => agent.client.call(json!({"op": "info"}))?,
        };
        let (width, height) = (info["width"].as_i64().unwrap_or(0) as i32, info["height"].as_i64().unwrap_or(0) as i32);
        Ok(IosDevice { udid: udid.to_string(), agent, width, height, recording: None, launch_permissions: Default::default(), settled: Default::default(), appearance: Default::default(), gave_kept: Default::default(), due: None, trimmed: false, busy: false, grant_trace: Vec::new() })
    }

    /// The launched app's wait, if one is due.
    fn pay_due(&mut self) -> Result<()> {
        if let Some(req) = self.due.take() {
            let start = Instant::now();
            self.agent.client.call(req)?;
            trace("launched", start);
        }
        Ok(())
    }

    /// A request to the agent; any but a settle may change the screen.
    fn call(&mut self, req: Value) -> Result<Value> {
        self.pay_due()?;
        if req["op"] != "settle" {
            self.settled.borrow_mut().clear();
            self.appearance.set(None);
        }
        // The op's name kept only for the trace (it goes with the request).
        let (op, start) = (tracing().then(|| req["op"].as_str().unwrap_or("").to_string()), Instant::now());
        let r = self.agent.client.call(req);
        if let Some(op) = op {
            trace(&op, start);
        }
        r
    }

    /// A raw request (a screen, a settle), timed for VIRTUOSO_IOS_TRACE.
    fn call_raw(&mut self, req: &Value) -> Result<Vec<u8>> {
        // A read (a screen, a settle) goes in the same write as the launch's wait it pays: one
        // round trip. An action still waits for the app to be in front before it is sent.
        if self.due.is_some() && matches!(req["op"].as_str(), Some("screen" | "settle")) {
            let due = self.due.take().unwrap_or_default();
            let start = Instant::now();
            let (launched, r) = self.agent.client.call_raw_pair(&due, req)?;
            trace("launched + read", start);
            let launched: Value = serde_json::from_slice(&launched)?;
            if launched["ok"] != true {
                bail!("agent: {}", launched["error"].as_str().unwrap_or("request failed"));
            }
            return Ok(r);
        }
        self.pay_due()?;
        let start = Instant::now();
        let r = self.agent.client.call_raw(req);
        trace(req["op"].as_str().unwrap_or(""), start);
        r
    }

    /// A screen reply as a tree, the screen's size taken from the app's frame.
    fn parse_screen(&mut self, body: Vec<u8>) -> Result<ScreenTree> {
        let reply = ScreenReply::parse(&body, self.width, self.height)?;
        self.agent.client.recycle(body);
        trace_agent(&reply);
        self.trimmed = reply.trimmed;
        Ok(self.sized(reply.tree))
    }

    /// The tree with the screen's size taken from the app's frame.
    fn sized(&mut self, mut tree: ScreenTree) -> ScreenTree {
        // The screen turns with the device: the app's own frame is its size now.
        if let Some(app) = tree.elements.iter().find(|e| e.depth == 0 && &*e.class == "Application" && e.bounds.area() > 0) {
            (self.width, self.height) = (app.bounds.width(), app.bounds.height());
            (tree.width, tree.height) = (self.width, self.height);
        }
        tree
    }

    /// A settle's reply: its outcome, and the screen it ended on kept for the next read.
    fn settled_from(&mut self, body: Vec<u8>) -> Result<Settled> {
        let r = ScreenReply::parse(&body, self.width, self.height)?;
        self.agent.client.recycle(body);
        if tracing() {
            let (idle, snaps, snap_ms) = r.settle_trace;
            println!("    [ios]   settle: idle waits {idle} ms, {snaps} snapshots {snap_ms} ms; {}", r.trace);
        }
        // The screen it ended on comes with it: the next read. The one the host holds when it
        // ended on that (no tree sent).
        if r.unchanged {
            self.settled.borrow_mut().renew(r.seq, Instant::now());
        } else if r.has_screen {
            let tree = self.sized(r.tree);
            self.settled.borrow_mut().put(tree, Instant::now());
        }
        Ok(Settled { changed: r.changed, timed_out: r.timed_out })
    }

    fn simctl(&self, args: &[&str]) -> Result<String> {
        self.settled.borrow_mut().clear();
        self.appearance.set(None);
        run_simctl(args)
    }
}

impl Device for IosDevice {
    fn platform(&self) -> &'static str {
        "ios"
    }

    fn screen(&mut self) -> Result<ScreenTree> {
        let kept = self.settled.borrow().get(Instant::now());
        self.gave_kept.set(kept.is_some());
        if let Some(tree) = kept {
            return Ok(tree);
        }
        let body = self.call_raw(&json!({"op": "screen"}))?;
        let tree = self.parse_screen(body)?;
        self.settled.borrow_mut().put_read(tree.clone(), Instant::now());
        Ok(tree)
    }

    /// A wait reads afresh (the kept screen is the one the wait began from), and the agent
    /// sends the tree only when it is no longer `current`: a still screen costs its snapshot,
    /// not its transfer.
    /// The first look after the kept screen is not paused: that screen may be long gone.
    fn screen_after_change(&mut self, current: ScreenTree, wait: Duration) -> Result<ScreenTree> {
        if !self.gave_kept.replace(false) {
            std::thread::sleep(wait.min(Duration::from_millis(10)));
        }
        let Some(seq) = current.seq else { return self.fresh_screen() };
        let tree = match self.screen_unless(seq)? {
            Some(tree) => tree,
            None => current,
        };
        self.settled.borrow_mut().put_read(tree.clone(), Instant::now());
        Ok(tree)
    }

    /// The agent checks the screen is still the lookup's (the same tree, by its hash) and taps
    /// in the same request; or sends the screen as it is now, untapped.
    fn tap_if_unchanged(&mut self, screen: &ScreenTree, x: i32, y: i32) -> Result<Option<ScreenTree>> {
        let Some(seq) = screen.seq else { return Ok(Some(self.fresh_screen()?)) };
        self.settled.borrow_mut().clear();
        self.appearance.set(None);
        // What it gives back is read now.
        self.gave_kept.set(false);
        let body = self.call_raw(&json!({"op": "tap", "x": x, "y": y, "ifSeq": seq}))?;
        let r = ScreenReply::parse(&body, self.width, self.height)?;
        self.agent.client.recycle(body);
        if !r.has_screen {
            self.busy = r.busy;
            return Ok(None);
        }
        let tree = self.sized(r.tree);
        self.settled.borrow_mut().put_read(tree.clone(), Instant::now());
        Ok(Some(tree))
    }

    /// A settle's screen (it held still), else a fresh read.
    fn screen_to_act_on(&mut self) -> Result<ScreenTree> {
        let kept = self.settled.borrow().get_settled(Instant::now());
        match kept {
            Some(tree) => {
                self.gave_kept.set(true);
                Ok(tree)
            }
            None => self.fresh_screen(),
        }
    }

    /// Kept from the agent's info and every read since (it turns with the device).
    fn screen_rect(&mut self) -> Result<crate::screen::Rect> {
        Ok(crate::screen::Rect { left: 0, top: 0, right: self.width, bottom: self.height })
    }

    fn fresh_screen(&mut self) -> Result<ScreenTree> {
        self.settled.borrow_mut().clear();
        self.screen()
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<()> {
        let r = self.call(action_request(Act::Tap(x, y))?)?;
        self.busy = r["busy"] == true;
        Ok(())
    }

    fn touched_while_busy(&self) -> bool {
        self.busy
    }

    /// One gesture: taps sent one by one reach the app unevenly spaced, and React Native drops a
    /// press that comes too soon after the last (a double tap lands as one tap).
    fn taps(&mut self, x: i32, y: i32, count: u32, interval: std::time::Duration) -> Result<()> {
        self.call(json!({"op": "taps", "x": x, "y": y, "count": count, "ms": interval.as_millis() as u64})).map(|_| ())
    }

    fn long_press(&mut self, x: i32, y: i32, ms: u64) -> Result<()> {
        self.call(json!({"op": "press", "x": x, "y": y, "ms": ms})).map(|_| ())
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32), ms: u64, end: Lift) -> Result<()> {
        self.call(action_request(Act::Swipe(from, to, ms, end))?).map(|_| ())
    }

    fn press_key(&mut self, key: Key) -> Result<()> {
        self.call(action_request(Act::Key(key))?).map(|_| ())
    }

    fn input_text(&mut self, text: &str) -> Result<()> {
        self.call(action_request(Act::Type(text))?).map(|_| ())
    }

    fn erase_text(&mut self, chars: usize) -> Result<()> {
        self.call(action_request(Act::Erase(chars))?).map(|_| ())
    }

    fn mark(&mut self) -> Result<()> {
        Ok(())
    }

    fn settle(&mut self, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let held = self.settled.borrow().last_seq();
        let body = self.call_raw(&settle_request(no_change, timeout, held))?;
        self.settled_from(body)
    }

    /// The settle goes in the same write as the action: the agent takes them in order, and
    /// starts the settle as soon as the action is done, with no round trip between. Both
    /// answers are read off; an action's error is the step's.
    fn act_and_settle(&mut self, act: Act, no_change: Duration, timeout: Duration) -> Result<Settled> {
        let req = action_request(act)?;
        // The action waits for the app just launched to be in front.
        self.pay_due()?;
        self.settled.borrow_mut().clear();
        self.appearance.set(None);
        let held = self.settled.borrow().last_seq();
        let start = Instant::now();
        let (acted, settled) = self.agent.client.call_raw_pair(&req, &settle_request(no_change, timeout, held))?;
        if tracing() {
            trace(&format!("{} + settle", req["op"].as_str().unwrap_or("")), start);
        }
        let acted: Value = serde_json::from_slice(&acted)?;
        if acted["ok"] != true {
            bail!("agent: {}", acted["error"].as_str().unwrap_or("request failed"));
        }
        if matches!(act, Act::Tap(..)) {
            self.busy = acted["busy"] == true;
        }
        self.settled_from(settled)
    }

    fn launch_app(&mut self, app: &str, o: &LaunchOptions) -> Result<()> {
        self.pay_due()?;
        // The permissions the flow names (set here, `launch_sets_permissions`), else every one.
        // `simctl privacy` takes ~0.3 s: the default grant this run made already is not made
        // again, nor an earlier run's while the stores it wrote are untouched since. An app
        // brought back as it was (stopApp: false) gets no default grant: a change stops it.
        let activate = !o.stop_app && !o.clear_state;
        let named = !o.permissions.is_empty();
        let perms = launch_permissions(&o.permissions);
        if named {
            // As setPermissions: the next default launch grants its own again.
            self.launch_permissions.remove(app);
            forget_grant(&self.udid, app);
        }
        let sim_data = simulator_data(&self.udid);
        let set_default = !activate
            && !named
            && self.launch_permissions.get(app) != Some(&perms)
            && !granted_already(&self.udid, app, &perms, &permission_fingerprint(&sim_data, &self.udid, app));
        if tracing() && !activate && !named {
            let now = permission_fingerprint(&sim_data, &self.udid, app);
            let line = if self.launch_permissions.get(app) == Some(&perms) {
                format!("{app} granted by this run")
            } else if granted_already(&self.udid, app, &perms, &now) {
                format!("{app} skipped (note matches)")
            } else {
                let note = std::fs::read_to_string(grant_note(&self.udid, app)).unwrap_or_else(|_| "none".into());
                format!("{app} granted (note {:?}, now {now:?})", note.replace('\n', " | "))
            };
            self.grant_trace.push(line);
        }
        self.settled.borrow_mut().clear();
        self.appearance.set(None);
        if activate && named {
            // Before the app is brought back, as a setPermissions step before the launch.
            apply_permissions(&self.udid, app, &perms)?;
        }
        let set_perms = set_default || (named && !activate);
        // The app's data, its permissions and the keychain are separate things: all at once.
        let udid = self.udid.as_str();
        std::thread::scope(|s| -> Result<()> {
            let keychain = o.clear_keychain.then(|| s.spawn(|| run_simctl(&["keychain", udid, "reset"]).map(|_| ())));
            let permissions = set_perms.then(|| s.spawn(|| apply_permissions(udid, app, &perms)));
            let cleared = if o.clear_state { clear_app_data(udid, app) } else { Ok(()) };
            for job in [keychain, permissions].into_iter().flatten() {
                job.join().map_err(|_| anyhow!("a launch step panicked"))??;
            }
            cleared
        })?;
        if set_default {
            // The stores as the grant (and a clear's data folder lookup) left them.
            remember_grant(&self.udid, app, &perms, &permission_fingerprint(&sim_data, &self.udid, app));
            self.launch_permissions.insert(app.to_string(), perms);
        }
        let args: Vec<String> = o.arguments.iter().flat_map(|(k, v)| [format!("-{k}"), v.clone()]).collect();
        if activate {
            return self.call(json!({"op": "launch", "app": app, "args": args, "activate": true})).map(|_| ());
        }
        // As speedrun does: simctl, back once the process starts (XCTest's launch waits for the
        // app to go idle, ~4 s for Settings), then the agent waits for it in the foreground.
        // One simctl call: it stops the app first if it runs (clearState has stopped it already).
        let mut cmd = vec!["launch", "--terminate-running-process", self.udid.as_str(), app];
        cmd.extend(args.iter().map(String::as_str));
        let udid = self.udid.clone();
        let out = self.simctl(&cmd).with_context(|| format!("launching {app} on {udid}"))?;
        // The new process by its pid: XCTest can still hold the one simctl stopped under the
        // same bundle id, and a read of that is "Lost connection to the application".
        let pid = launched_pid(&out).unwrap_or(0);
        self.due = Some(json!({"op": "launched", "app": app, "pid": pid, "timeout": 10000}));
        Ok(())
    }

    fn stop_app(&mut self, app: &str) -> Result<()> {
        // What changes the app waits for its launch first (as when the launch waited itself).
        self.pay_due()?;
        // Not running is stopped too: simctl's error for it is not one.
        let _ = self.simctl(&["terminate", &self.udid, app]);
        Ok(())
    }

    fn clear_state(&mut self, app: &str) -> Result<()> {
        self.pay_due()?;
        self.settled.borrow_mut().clear();
        self.appearance.set(None);
        clear_app_data(&self.udid, app)
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        self.pay_due()?;
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
        self.call(json!({"op": "notifications"})).map(|_| ())
    }

    fn hide_keyboard(&mut self) -> Result<()> {
        self.call(json!({"op": "hideKeyboard"})).map(|_| ())
    }

    fn open_link(&mut self, link: &str, app: Option<&str>, browser: bool) -> Result<()> {
        // Into the flow's app, as XCUITest opens it: no prompt. A web link, or one for the
        // browser, goes to whatever handles it.
        self.pay_due()?;
        let web = link.starts_with("http://") || link.starts_with("https://");
        if let (Some(app), false, false) = (app, browser, web) {
            return self.call(json!({"op": "openURL", "url": link, "app": app})).map(|_| ());
        }
        let before = front_app(&self.screen()?).map(String::from);
        self.simctl(&["openurl", &self.udid, link])?;
        // A link to an app asks first ("Open in “App”?"), as Maestro answers it: Open. The
        // prompt comes before any switch: once another app is in front (Safari, a universal
        // link's app), none is coming.
        let end = Instant::now() + Duration::from_secs(8);
        let mut seq = None;
        while Instant::now() < end {
            // Another app in front, by the agent's word alone: its tree is not read (a web
            // page's; XCTest waits seconds for a just-launched app's accessibility).
            if let Some(app) = self.front_package()? {
                if app != "com.apple.springboard" && Some(app.as_str()) != before.as_deref() {
                    return Ok(());
                }
            }
            // After the first look, the screen only when it changed (a still one is a few bytes).
            let read = match seq {
                Some(seq) => self.screen_unless(seq)?,
                None => Some(self.fresh_screen()?),
            };
            if let Some(s) = read {
                if let Some((x, y)) = open_prompt_button(&s) {
                    return self.tap(x, y);
                }
                if front_app(&s).is_some_and(|app| Some(app) != before.as_deref()) {
                    return Ok(());
                }
                seq = s.seq.or(seq);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }

    fn set_location(&mut self, lat: f64, lon: f64) -> Result<()> {
        self.simctl(&["location", &self.udid, "set", &format!("{lat},{lon}")]).map(|_| ())
    }

    fn set_permissions(&mut self, app: &str, perms: &[(String, String)]) -> Result<()> {
        self.pay_due()?;
        self.launch_permissions.remove(app);
        forget_grant(&self.udid, app);
        self.settled.borrow_mut().clear();
        self.appearance.set(None);
        apply_permissions(&self.udid, app, perms)
    }

    /// Alongside clearState and the keychain reset, which touch other things.
    fn launch_sets_permissions(&self) -> bool {
        true
    }

    /// `simctl addmedia`, waited for by its exit, not its output (what it starts can hold the
    /// pipe for minutes). The simulator reads the file itself, and a folder macOS protects
    /// (Downloads, Documents) makes it ask for access first: each file goes through a temp copy.
    fn add_media(&mut self, files: &[&Path]) -> Result<()> {
        let tmp = std::env::temp_dir().join(format!("virtuoso-media-{}-{}", std::process::id(), new_token()));
        std::fs::create_dir_all(&tmp)?;
        let result = add_media_files(&self.udid, files, &tmp);
        let _ = std::fs::remove_dir_all(&tmp);
        result
    }

    fn set_dark_mode(&mut self, on: Option<bool>) -> Result<()> {
        self.pay_due()?;
        let on = match on {
            Some(on) => on,
            None => !self.dark_mode()?,
        };
        self.simctl(&["ui", &self.udid, "appearance", if on { "dark" } else { "light" }])?;
        self.appearance.set(Some(on));
        Ok(())
    }

    fn dark_mode(&mut self) -> Result<bool> {
        if let Some(on) = self.appearance.get() {
            return Ok(on);
        }
        // A read: the screen kept stands.
        Ok(run_simctl(&["ui", &self.udid, "appearance"])?.trim() == "dark")
    }

    fn start_recording(&mut self, path: &Path) -> Result<()> {
        let child = simctl_command()
            .args(["io", &self.udid, "recordVideo", "--codec=h264", "--force"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting simctl recordVideo")?;
        self.recording = Some(child);
        // simctl starts recording a moment after it starts: the file shows when it has. A look
        // costs microseconds: looked for often.
        let end = Instant::now() + Duration::from_secs(5);
        while !path.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<()> {
        let Some(mut child) = self.recording.take() else { return Ok(()) };
        // An interrupt, not a kill: simctl writes the video's index on the way out.
        interrupt(&child);
        child.wait()?;
        Ok(())
    }

    fn set_clipboard(&mut self, text: &str) -> Result<()> {
        // By the agent (`simctl pbcopy` took ~5 s on a CI runner); the screen is as it was.
        self.pay_due()?;
        self.agent.client.call(json!({"op": "clipboard", "text": text})).map(|_| ())
    }

    fn clear_keychain(&mut self) -> Result<()> {
        self.pay_due()?;
        self.simctl(&["keychain", &self.udid, "reset"]).map(|_| ())
    }

    fn id(&self) -> String {
        self.udid.clone()
    }
}

/// The app stopped and its data cleared (Documents, Library, tmp): the data folder is looked up
/// (or remembered) while the app stops.
fn clear_app_data(udid: &str, app: &str) -> Result<()> {
    let data = if let Some(d) = remembered_container(udid, app) {
        let _ = run_simctl(&["terminate", udid, app]);
        d
    } else {
        look_up_while_stopping(udid, app)?
    };
    // Three folders apart: cleared at once.
    std::thread::scope(|s| {
        let cleared: Vec<_> = ["Documents", "Library", "tmp"]
            .into_iter()
            .map(|dir| {
                let d = data.join(dir);
                s.spawn(move || -> Result<()> {
                    if d.exists() {
                        std::fs::remove_dir_all(&d).with_context(|| format!("clearing {}", d.display()))?;
                        std::fs::create_dir_all(&d)?;
                    }
                    Ok(())
                })
            })
            .collect();
        cleared.into_iter().try_for_each(|h| h.join().map_err(|_| anyhow!("clearing {app}'s data panicked"))?)
    })
}

/// The app's data folder looked up while the app stops.
fn look_up_while_stopping(udid: &str, app: &str) -> Result<PathBuf> {
    std::thread::scope(|s| {
        let data = s.spawn(|| -> Result<PathBuf> {
            if let Some(d) = remembered_container(udid, app) {
                return Ok(d);
            }
            let d = PathBuf::from(run_simctl(&["get_app_container", udid, app, "data"])?.trim());
            remember_container(udid, app, &d);
            Ok(d)
        });
        // Not running is stopped too: simctl's error for it is not one.
        let _ = run_simctl(&["terminate", udid, app]);
        data.join().map_err(|_| anyhow!("looking up {app}'s data panicked"))
    })?
}

/// The permissions set for an app: in the simulator's TCC database what simctl cannot set,
/// then through `simctl privacy`.
fn apply_permissions(udid: &str, app: &str, perms: &[(String, String)]) -> Result<()> {
    if let Some(sql) = tcc_sql(app, perms) {
        let db = simulator_data(udid).join(PERMISSION_STORES[0]).to_string_lossy().into_owned();
        // tccd can hold the database a while after the simulator boots: its lock is waited for.
        let o = Command::new("sqlite3").args(["-cmd", ".timeout 5000", &db, &sql]).output().context("setting permissions needs sqlite3")?;
        if !o.status.success() {
            bail!("could not set permissions in {db}: {}", String::from_utf8_lossy(&o.stderr).trim());
        }
    }
    // What simctl cannot set (the camera, notifications) is left to the app to ask.
    let (first, apart) = privacy_steps(perms);
    for (action, service) in first {
        run_simctl(&["privacy", udid, action, service, app])?;
    }
    std::thread::scope(|s| {
        let lines: Vec<_> = apart
            .into_iter()
            .map(|calls| s.spawn(move || calls.into_iter().try_for_each(|(action, service)| run_simctl(&["privacy", udid, action, service, app]).map(|_| ()))))
            .collect();
        lines.into_iter().try_for_each(|h| h.join().map_err(|_| anyhow!("setting {app}'s permissions panicked"))?)
    })
}

/// With VIRTUOSO_IOS_TRACE, how long an agent request or a simctl command took: printed among
/// the steps (stdout), to see where a step's time goes.
/// SIGINT to a process this run started.
#[cfg(unix)]
fn interrupt(child: &Child) {
    // SAFETY: a signal to a child this run started and has not yet waited for.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGINT);
    }
}

#[cfg(not(unix))]
fn interrupt(child: &Child) {
    let _ = Command::new("kill").args(["-INT", &child.id().to_string()]).status();
}

/// VIRTUOSO_IOS_TRACE, read once.
pub(crate) fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VIRTUOSO_IOS_TRACE").is_some())
}

fn trace(what: &str, start: Instant) {
    if tracing() {
        println!("    [ios] {what} {} ms", start.elapsed().as_millis());
    }
}

/// Where the agent's time went on a read, for VIRTUOSO_IOS_TRACE.
fn trace_agent(r: &ScreenReply) {
    if !r.trace.is_empty() && tracing() {
        println!("    [ios]   {}", r.trace);
    }
}

/// `simctl addmedia` for copies of `files` in `tmp`, one file per import as speedrun does, each
/// waited for by its exit. A simulator's first import can take minutes on CI (one took 339 s),
/// and one stopped part-way left the simulator's accessibility broken for the tests after it:
/// each is waited for up to MEDIA_IMPORT_LIMIT.
fn add_media_files(udid: &str, files: &[&Path], tmp: &Path) -> Result<()> {
    for (k, f) in files.iter().enumerate() {
        // The file's own name last: simctl tells a photo from a video by its extension.
        let copy = tmp.join(format!("{k}-{}", f.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
        std::fs::copy(f, &copy).with_context(|| format!("reading {}", f.display()))?;
        let start = Instant::now();
        let mut child =
            simctl_command().args(["addmedia", udid]).arg(&copy).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        loop {
            if let Some(status) = child.try_wait()? {
                trace("simctl addmedia", start);
                if !status.success() {
                    bail!("`simctl addmedia` failed");
                }
                break;
            }
            if start.elapsed() >= MEDIA_IMPORT_LIMIT {
                let _ = child.kill();
                let _ = child.wait();
                bail!("`simctl addmedia` did not finish within {} s: is macOS asking whether the simulator may read a folder?", MEDIA_IMPORT_LIMIT.as_secs());
            }
            // Looked for often while an import could be quick (a check costs microseconds);
            // a long one (a first import on CI) is not timed that finely.
            std::thread::sleep(Duration::from_millis(if start.elapsed() < Duration::from_secs(2) { 1 } else { 20 }));
        }
    }
    Ok(())
}

const MEDIA_IMPORT_LIMIT: Duration = Duration::from_secs(360);
