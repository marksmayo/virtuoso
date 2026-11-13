//! Android devices through adb, and the agent's lifecycle on them.

use crate::agent::AgentClient;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The agent, built by build.rs from agent/android.
pub static AGENT_JAR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/virtuoso-agent.jar"));
/// The agent jar's FNV-1a hash, in its file name on the device and in `--version`.
pub const AGENT_HASH: &str = env!("VIRTUOSO_AGENT_HASH");

const AGENT_PORT: u16 = 7999;
/// How long the agent may take to say it is listening: on a slow emulator the system can take
/// a while to grant it the UiAutomation (the agent waits up to 20 s for it).
const AGENT_START_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_NAME: &str = "virtuoso-agent";
/// Stops a running agent and waits (up to 2 s) for it to exit, so its UiAutomation and port are free.
const STOP_AGENT: &str = "p=$(pidof virtuoso-agent) && kill $p && for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do pidof virtuoso-agent >/dev/null || break; sleep 0.1; done;";

#[derive(Clone, Debug)]
pub struct Adb {
    pub serial: String,
}

/// adb from ANDROID_HOME when it is there (the same build the emulator talks to), else PATH.
pub fn adb_path() -> PathBuf {
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(home) = std::env::var(var) {
            let p = PathBuf::from(home).join("platform-tools").join(if cfg!(windows) { "adb.exe" } else { "adb" });
            if p.exists() {
                return p;
            }
        }
    }
    PathBuf::from("adb")
}

/// The emulator next to adb in the SDK, else on PATH.
pub fn emulator_path() -> PathBuf {
    let exe = if cfg!(windows) { "emulator.exe" } else { "emulator" };
    adb_path()
        .parent()
        .and_then(Path::parent)
        .map(|sdk| sdk.join("emulator").join(exe))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(exe))
}

/// The AVD to start from `emulator -list-avds` output: `wanted`, or the first one listed.
pub fn pick_avd(listed: &str, wanted: Option<&str>) -> Option<String> {
    // The emulator mixes log lines ("INFO | ...") into the list.
    let mut avds = listed.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains('|') && !l.contains(' '));
    match wanted {
        Some(w) => avds.find(|a| *a == w).map(str::to_string),
        None => avds.next().map(str::to_string),
    }
}

/// Starts an emulator (`wanted`, or the first AVD) and waits until it has booted; its serial.
pub fn start_emulator(wanted: Option<&str>, timeout: Duration) -> Result<String> {
    let emulator = emulator_path();
    let listed = Command::new(&emulator).arg("-list-avds").output().with_context(|| format!("running {}", emulator.display()))?;
    let listed = String::from_utf8_lossy(&listed.stdout).to_string();
    let avd = pick_avd(&listed, wanted).ok_or_else(|| match wanted {
        Some(w) => anyhow!("no AVD named {w} (emulator -list-avds: {})", listed.split_whitespace().collect::<Vec<_>>().join(", ")),
        None => anyhow!("no AVD to start: create one in Android Studio's Device Manager"),
    })?;
    let before = devices()?;
    eprintln!("starting emulator {avd}...");
    Command::new(&emulator)
        .args(["-avd", &avd, "-no-snapshot-save"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("starting {}", emulator.display()))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(serial) = devices()?.into_iter().find(|d| d.starts_with("emulator-") && !before.contains(d)) {
            let booted = Adb::new(&serial).shell("getprop sys.boot_completed").map(|v| v.trim() == "1").unwrap_or(false);
            if booted {
                return Ok(serial);
            }
        }
        if std::time::Instant::now() >= deadline {
            bail!("emulator {avd} did not boot within {} s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Serials of the devices adb can use now (state `device`).
pub fn devices() -> Result<Vec<String>> {
    if let Ok(d) = crate::adb_wire::AdbWire::local().devices() {
        return Ok(d);
    }
    let out = Command::new(adb_path()).arg("devices").output().context("running adb")?;
    Ok(parse_devices(&String::from_utf8_lossy(&out.stdout)))
}

pub fn parse_devices(out: &str) -> Vec<String> {
    out.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            match (parts.next(), parts.next()) {
                (Some(serial), Some("device")) => Some(serial.to_string()),
                _ => None,
            }
        })
        .collect()
}

impl Adb {
    pub fn new(serial: &str) -> Self {
        Adb { serial: serial.to_string() }
    }

    pub fn command(&self) -> Command {
        let mut c = Command::new(adb_path());
        c.arg("-s").arg(&self.serial);
        c
    }

    /// Runs adb with `args`; fails with adb's output when it exits non-zero.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let out = self.command().args(args).output().context("running adb")?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.status.success() {
            bail!("adb {} failed: {}{}", args.join(" "), text, String::from_utf8_lossy(&out.stderr));
        }
        Ok(text)
    }

    pub fn shell(&self, cmd: &str) -> Result<String> {
        self.run(&["shell", cmd])
    }

    /// Forwards local TCP `local` to the device's `remote`, through the adb server directly
    /// when it answers, else through `adb`.
    pub fn forward(&self, local: u16, remote: u16) -> Result<()> {
        if crate::adb_wire::AdbWire::local().forward(&self.serial, local, remote).is_ok() {
            return Ok(());
        }
        self.run(&["forward", &format!("tcp:{local}"), &format!("tcp:{remote}")]).map(|_| ())
    }
}

/// The adb shell the agent runs in: a socket to the adb server, or an `adb` process.
enum AgentShell {
    Wire(std::net::TcpStream),
    Process(Child),
}

impl AgentShell {
    fn kill(&mut self) {
        match self {
            AgentShell::Wire(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            AgentShell::Process(c) => {
                let _ = c.kill();
            }
        }
    }
}

/// A running agent and the connection to it. Dropping it stops the agent.
pub struct AndroidAgent {
    adb: Adb,
    child: AgentShell,
    client: AgentClient,
    display: Display,
    /// Already torn down: dropping it must not stop the agent that replaced it.
    stopped: bool,
}

fn new_token() -> String {
    let mut s = String::new();
    for _ in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        h.write_u32(std::process::id());
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

/// Windows sleeps in 15.6 ms ticks by default: a 10 ms poll would wait 15.6 ms. This asks for
/// 1 ms ticks until the process exits. winmm is loaded by name, not imported, so commands that
/// need no device do not pay its ~250 KB.
fn fine_timer() {
    #[cfg(windows)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        // SAFETY: plain Win32 calls; timeBeginPeriod has this signature in winmm.
        ONCE.call_once(|| unsafe {
            use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
            // UTF-16 built here: an ASCII "winmm.dll" literal in the binary would read as an import.
            const NAME: [u16; 10] = {
                let b = *b"winmm.dll\0";
                let mut w = [0u16; 10];
                let mut i = 0;
                while i < 10 {
                    w[i] = b[i] as u16;
                    i += 1;
                }
                w
            };
            let lib = LoadLibraryW(NAME.as_ptr());
            if lib.is_null() {
                return;
            }
            if let Some(f) = GetProcAddress(lib, c"timeBeginPeriod".as_ptr().cast()) {
                let time_begin_period: unsafe extern "system" fn(u32) -> u32 = std::mem::transmute(f);
                time_begin_period(1);
            }
        });
    }
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// What the agent reports about the display when it starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Display {
    pub width: i32,
    pub height: i32,
    pub rotation: i32,
    pub sdk: i32,
}

/// The local port forwarded to a device's agent: the same every run, so the forward is simply
/// rebound and never needs removing.
pub fn local_port_for(serial: &str) -> u16 {
    20000 + (fnv1a(serial.as_bytes()) % 10000) as u16
}

const ANIMATION_KEYS: &str = "window_animation_scale transition_animation_scale animator_duration_scale";

impl AndroidAgent {
    pub fn start(adb: &Adb) -> Result<Self> {
        Self::start_with(adb, false)
    }

    /// Starts the agent in one adb shell: stop an agent already running, drop other builds,
    /// turn animations off (`animations_off`), start this build (pushed first when missing). The
    /// port forward is set up while the agent boots.
    pub fn start_with(adb: &Adb, animations_off: bool) -> Result<Self> {
        if AGENT_JAR.is_empty() {
            bail!("this build of virtuoso has no Android agent (built without a JDK or Android SDK)");
        }
        fine_timer();
        let remote = format!("/data/local/tmp/{AGENT_NAME}-{AGENT_HASH}.jar");
        let token = new_token();
        let port = local_port_for(&adb.serial);
        let forward = {
            let adb = adb.clone();
            std::thread::spawn(move || adb.forward(port, AGENT_PORT))
        };
        // Animations go off in a shell of their own while the agent boots (~300 ms), joined
        // before this returns: inline, the settings reads and writes (~100 ms when all three were
        // on) came before the agent could start. Inline still when the adb server cannot be
        // reached directly.
        let prep = format!("l=$(cmd settings list global); for k in {ANIMATION_KEYS}; do echo \"$l\" | grep -qxE \"$k=0(\\.0)?\" || cmd settings put global $k 0 & done; wait");
        let side = if animations_off { crate::adb_wire::AdbWire::local().shell(&adb.serial, &prep).ok() } else { None };
        let animations = if animations_off && side.is_none() { format!("{prep};") } else { String::new() };
        let script = agent_start_script(&remote, &animations, &token, true);
        let (child, display) = match launch(adb, &script)? {
            Launched::Missing => {
                // Straight from memory through the adb server; `adb push` of a temp file if not.
                if crate::adb_wire::AdbWire::local().push(&adb.serial, AGENT_JAR, &remote, 0o644).is_err() {
                    let tmp = std::env::temp_dir().join(format!("{AGENT_NAME}-{}.jar", std::process::id()));
                    std::fs::write(&tmp, AGENT_JAR)?;
                    let pushed = adb.run(&["push", &tmp.to_string_lossy(), &remote]);
                    let _ = std::fs::remove_file(&tmp);
                    pushed?;
                }
                // The first script stopped any agent already: not again.
                match launch(adb, &agent_start_script(&remote, &animations, &token, false))? {
                    Launched::Running(child, display) => (child, display),
                    Launched::Missing => bail!("the agent is still missing on the device after pushing it"),
                }
            }
            Launched::Running(child, display) => (child, display),
        };

        if let Some(mut side) = side {
            // Its output to its end: the shell has exited, the settings are written.
            std::io::Read::read_to_end(&mut side, &mut Vec::new()).context("turning animations off")?;
        }
        // A port someone else holds: let adb pick one.
        let local_port: u16 = match forward.join().map_err(|_| anyhow!("adb forward panicked"))? {
            Ok(_) => port,
            Err(_) => {
                let fwd = adb.run(&["forward", "tcp:0", &format!("tcp:{AGENT_PORT}")])?;
                fwd.trim().parse().map_err(|_| anyhow!("unexpected adb forward output '{}'", fwd.trim()))?
            }
        };
        let addr: SocketAddr = ([127, 0, 0, 1], local_port).into();
        let mut client = AgentClient::connect(addr, &token)?;
        client.set_timeout(Some(Duration::from_secs(60)))?;
        let mut agent = AndroidAgent { adb: adb.clone(), child, client, display, stopped: false };
        if agent.display == Display::default() {
            // An agent that could not read the display at start-up.
            let info = agent.client.call(serde_json::json!({ "op": "info" })).context("the agent does not answer")?;
            agent.display = display_from(&info);
        }
        Ok(agent)
    }

    pub fn display(&self) -> Display {
        self.display
    }

    pub fn client(&mut self) -> &mut AgentClient {
        &mut self.client
    }

    /// Replaces a lost agent with a fresh one.
    pub fn restart(&mut self) -> Result<()> {
        self.child.kill();
        let fresh = AndroidAgent::start(&self.adb)?;
        let mut old = std::mem::replace(self, fresh);
        old.stopped = true;
        Ok(())
    }
}

/// The script that starts the agent from the jar at `remote`: stops one already running (with
/// `stop`), runs `animations`, and says `missing` (doing nothing more) when the jar is not on
/// the device.
pub fn agent_start_script(remote: &str, animations: &str, token: &str, stop: bool) -> String {
    let stop = if stop { STOP_AGENT } else { "" };
    format!(
        // Other builds' agents stay (two builds sharing a device would push over each other's
        // on every run) unless unused for a week; starting an agent marks it used. That
        // housekeeping runs alongside the start, not before it: this start does not need it.
        "{stop} [ -f {remote} ] || {{ echo missing; exit 0; }}; \
         (touch {remote}; find /data/local/tmp -maxdepth 1 -name '{AGENT_NAME}-*.jar' -mtime +7 -delete) & {animations} \
         CLASSPATH={remote} exec app_process / --nice-name={AGENT_NAME} dev.virtuoso.agent.Main {AGENT_PORT} {token}"
    )
}

enum Launched {
    /// The jar is not on the device yet.
    Missing,
    Running(AgentShell, Display),
}

/// Runs the start script and waits for the agent's ready line.
fn launch(adb: &Adb, script: &str) -> Result<Launched> {
    let (mut child, stdout): (AgentShell, Box<dyn std::io::Read + Send>) = match crate::adb_wire::AdbWire::local().shell(&adb.serial, script) {
        Ok(s) => {
            let reader = s.try_clone()?;
            (AgentShell::Wire(s), Box::new(reader))
        }
        Err(_) => {
            let mut child = adb
                .command()
                .args(["shell", script])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("starting the agent")?;
            let stdout = child.stdout.take().unwrap();
            (AgentShell::Process(child), Box::new(stdout))
        }
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut said = Vec::new();
    loop {
        match rx.recv_timeout(AGENT_START_TIMEOUT) {
            Ok(line) if line.trim() == "missing" => {
                child.kill();
                return Ok(Launched::Missing);
            }
            Ok(line) if line.starts_with("listening") => {
                let display = line.find('{').and_then(|i| serde_json::from_str::<serde_json::Value>(&line[i..]).ok()).map(|v| display_from(&v)).unwrap_or_default();
                return Ok(Launched::Running(child, display));
            }
            Ok(line) => said.push(line),
            Err(_) => {
                child.kill();
                bail!("the agent did not start: {}", said.join("\n"));
            }
        }
    }
}

fn display_from(v: &serde_json::Value) -> Display {
    let n = |k: &str| v[k].as_i64().unwrap_or(0) as i32;
    Display { width: n("width"), height: n("height"), rotation: n("rotation"), sdk: n("sdk") }
}

impl Drop for AndroidAgent {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        // The agent exits on its own; the next start stops one that did not. The forward stays
        // for the next run, which rebinds it.
        self.client.quit();
        self.child.kill();
    }
}

/// Android permissions behind a Maestro permission name (`camera`, `location`...), or the name
/// itself when it is already a permission (`android.permission.CAMERA`).
pub fn android_permissions(name: &str) -> Vec<String> {
    let p = |names: &[&str]| names.iter().map(|n| format!("android.permission.{n}")).collect();
    match name.to_ascii_lowercase().as_str() {
        "location" => p(&["ACCESS_FINE_LOCATION", "ACCESS_COARSE_LOCATION"]),
        "camera" => p(&["CAMERA"]),
        "contacts" => p(&["READ_CONTACTS", "WRITE_CONTACTS"]),
        "phone" => p(&["CALL_PHONE", "READ_PHONE_STATE"]),
        "microphone" => p(&["RECORD_AUDIO"]),
        "bluetooth" => p(&["BLUETOOTH_CONNECT", "BLUETOOTH_SCAN"]),
        "storage" | "medialibrary" | "photos" => {
            p(&["READ_EXTERNAL_STORAGE", "WRITE_EXTERNAL_STORAGE", "READ_MEDIA_IMAGES", "READ_MEDIA_VIDEO", "READ_MEDIA_AUDIO"])
        }
        "notifications" => p(&["POST_NOTIFICATIONS"]),
        "sms" => p(&["SEND_SMS", "READ_SMS", "RECEIVE_SMS"]),
        "calendar" => p(&["READ_CALENDAR", "WRITE_CALENDAR"]),
        _ if name.contains('.') => vec![name.to_string()],
        _ => vec![],
    }
}

/// The runtime permissions `dumpsys package <app>` lists for the app, granted or not.
pub fn runtime_permissions(dumpsys: &str) -> Vec<String> {
    runtime_permission_states(dumpsys).into_iter().map(|(p, _)| p).collect()
}

/// The app's runtime permissions that are not granted yet.
pub fn ungranted_runtime_permissions(dumpsys: &str) -> Vec<String> {
    runtime_permission_states(dumpsys).into_iter().filter(|(_, granted)| !granted).map(|(p, _)| p).collect()
}

fn runtime_permission_states(dumpsys: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in dumpsys.lines() {
        let t = line.trim();
        if t.starts_with("runtime permissions:") {
            inside = true;
            continue;
        }
        if inside {
            match t.split_once(": granted=") {
                Some((perm, rest)) if perm.contains('.') => {
                    if !out.iter().any(|(p, _)| p == perm) {
                        out.push((perm.to_string(), rest.starts_with("true")));
                    }
                }
                _ => inside = false,
            }
        }
    }
    out
}

pub use crate::android_device::AndroidDevice;
