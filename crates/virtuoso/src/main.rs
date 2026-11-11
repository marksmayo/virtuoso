//! The `virtuoso` command: `virtuoso test` takes `maestro test`'s arguments.
//!
//! Exit status: 0 everything passed, 1 a flow failed (or `lint` found a problem), 2 the run
//! could not start (bad arguments, missing files, no device).

use anyhow::{anyhow, bail, Result};
use virtuoso::android::{devices, AndroidDevice};
use virtuoso::args::{Cli, Command, LintArgs, SessionCmd, TestArgs};
use virtuoso::device::Device;
use virtuoso::flow::{Cmd, Flow, Step};
use virtuoso::report::{FlowReport, SuiteReport};
use virtuoso::runner::{FlowStatus, RunOptions, Runner, StepResult, StepStatus};
use virtuoso::workspace::collect;
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() -> ExitCode {
    let cli = virtuoso::args::from_env();
    if cli.platform != "android" && cli.platform != "ios" {
        eprintln!("virtuoso: `--platform` is android or ios, not {}", cli.platform);
        return ExitCode::from(2);
    }
    let result = match &cli.command {
        Command::Test(a) if a.continuous => continuous(&cli, a),
        Command::Test(a) => test(&cli, a),
        Command::Lint(a) => lint(a),
        Command::Devices => list_devices(),
        Command::Hierarchy { all } => hierarchy(&cli, *all),
        Command::Mcp => mcp(&cli),
        Command::Skills { dir, print } => skills(dir.clone(), *print),
        Command::Doctor => doctor(),
        Command::Session(SessionCmd::Start { app }) => session_start(&cli, app),
        Command::Session(SessionCmd::Serve { app }) => session_serve(&cli, app),
        Command::Session(SessionCmd::Log) => session_call(json_op("log")),
        Command::Session(SessionCmd::Save { path }) => session_save(path),
        Command::Session(SessionCmd::Clear) => session_call(json_op("clear")),
        Command::Session(SessionCmd::Stop) => session_call(json_op("stop")),
        Command::Look { all } => session_call(serde_json::json!({"op": "look", "all": all})),
        Command::Do { commands } => session_call(serde_json::json!({"op": "do", "commands": commands})),
        Command::Latency { count } => latency(&cli, *count),
        Command::Screenshot { path } => screenshot(&cli, path.clone().unwrap_or_else(|| PathBuf::from("screenshot.png"))),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("virtuoso: {e:#}");
            ExitCode::from(2)
        }
    }
}

// ---- session ----
//
// `session start` spawns `session serve`, which holds the device and listens on a local port.
// The other commands send it one JSON line (with the token from the session file) and print
// the reply's text; the reply's `code` is their exit status.

fn session_file() -> PathBuf {
    std::env::var_os("VIRTUOSO_SESSION_FILE").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("virtuoso-session.json"))
}

fn json_op(op: &str) -> serde_json::Value {
    serde_json::json!({ "op": op })
}

fn session_start(cli: &Cli, app: &str) -> Result<u8> {
    let file = session_file();
    if file.exists() && session_call(json_op("ping")).is_ok() {
        bail!("a session is already running: `virtuoso session stop` first");
    }
    let _ = std::fs::remove_file(&file);
    let mut serve = std::process::Command::new(std::env::current_exe()?);
    if let Some(d) = &cli.device {
        serve.args(["--device", d]);
    }
    // The session outlives this command: its errors go to a log file, not to our pipes, and
    // it must not inherit our stdout and stderr (whoever reads them would wait for it to end).
    let log = file.with_extension("log");
    serve.args(["session", "serve", "--app", app]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::fs::File::create(&log)?);
    keep_std_handles_to_ourselves();
    let mut child = serve.spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !file.exists() {
        if let Some(status) = child.try_wait()? {
            let err = std::fs::read_to_string(&log).unwrap_or_default();
            bail!("the session did not start ({status}): {}", err.trim());
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            bail!("the session did not start within 60 s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("session started for {app}: `virtuoso look`, `virtuoso do <command>...`, `virtuoso session save <file>`");
    Ok(0)
}

/// Windows hands every inheritable handle to a child process, our own stdout and stderr
/// included; a child that outlives us would then hold the pipes of whoever runs us.
fn keep_std_handles_to_ourselves() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: plain Win32 calls on this process's own standard handles.
            unsafe {
                SetHandleInformation(GetStdHandle(which), HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn session_serve(cli: &Cli, app: &str) -> Result<u8> {
    use std::io::BufRead;
    let serial = pick_devices(cli, 1)?.remove(0);
    let mut dev = connect(&cli.platform, &serial, true)?;
    let mut session = virtuoso::session::Session::new(app, RunOptions::default());
    let listener = std::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))?;
    let token = format!("{:x}{:x}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos());
    let file = session_file();
    std::fs::write(&file, serde_json::json!({ "port": listener.local_addr()?.port(), "token": token }).to_string())?;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut line = String::new();
        if std::io::BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        if req["token"] != token.as_str() {
            continue;
        }
        let (code, text, stop) = match req["op"].as_str().unwrap_or("") {
            "ping" => (0, String::new(), false),
            "look" => match dev.screen() {
                Ok(s) if req["all"] == true => (0, s.elements.iter().map(|e| format!("{}{} {}", "  ".repeat(e.depth as usize), e.label(), e.bounds)).collect::<Vec<_>>().join("\n"), false),
                Ok(s) => (0, virtuoso::mcp::describe_screen(&s), false),
                Err(e) => (1, format!("{e:#}"), false),
            },
            "do" => {
                let commands: Vec<String> = req["commands"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
                let results = session.run(dev.as_mut(), &commands);
                let failed = results.iter().any(|r| !r.passed);
                let text = results
                    .iter()
                    .map(|r| match &r.error {
                        None => format!("✓ {} ({} ms)", r.command, r.duration_ms),
                        Some(e) => format!("✗ {}: {e}", r.command),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (u8::from(failed), text, false)
            }
            "log" => (0, session.log().join("\n"), false),
            "flow" => (0, session.flow_yaml(), false),
            "clear" => {
                session.clear();
                (0, "cleared".into(), false)
            }
            "stop" => (0, "session stopped".into(), true),
            other => (2, format!("unknown session request {other}"), false),
        };
        let _ = writeln!(stream, "{}", serde_json::json!({ "code": code, "text": text }));
        if stop {
            break;
        }
    }
    let _ = std::fs::remove_file(&file);
    Ok(0)
}

/// Sends a request to the running session; the reply's text and exit status.
fn session_request(mut req: serde_json::Value) -> Result<(u8, String)> {
    use std::io::BufRead;
    let file = session_file();
    let state: serde_json::Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or_else(|| anyhow!("no session is running: `virtuoso session start --app <appId>` first"))?;
    req["token"] = state["token"].clone();
    let port = state["port"].as_u64().unwrap_or(0) as u16;
    let mut stream = std::net::TcpStream::connect(std::net::SocketAddr::from(([127, 0, 0, 1], port))).map_err(|_| {
        let _ = std::fs::remove_file(&file);
        anyhow!("no session is running (it ended): `virtuoso session start --app <appId>`")
    })?;
    writeln!(stream, "{req}")?;
    let mut line = String::new();
    std::io::BufReader::new(&stream).read_line(&mut line)?;
    let reply: serde_json::Value = serde_json::from_str(&line).map_err(|_| anyhow!("the session ended without an answer"))?;
    Ok((reply["code"].as_u64().unwrap_or(2) as u8, reply["text"].as_str().unwrap_or("").to_string()))
}

fn session_call(req: serde_json::Value) -> Result<u8> {
    let (code, text) = session_request(req)?;
    if !text.is_empty() {
        println!("{text}");
    }
    Ok(code)
}

fn session_save(path: &Path) -> Result<u8> {
    let (code, yaml) = session_request(json_op("flow"))?;
    if code != 0 {
        bail!("{yaml}");
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, &yaml)?;
    println!("saved {}", path.display());
    Ok(0)
}

const SKILL: &str = include_str!("skill.md");

fn skills(dir: Option<PathBuf>, print: bool) -> Result<u8> {
    if print {
        print!("{SKILL}");
        return Ok(0);
    }
    let dir = match dir {
        Some(d) => d,
        None => {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or_else(|| anyhow!("no home folder: give --dir"))?;
            PathBuf::from(home).join(".claude").join("skills").join("virtuoso")
        }
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, SKILL)?;
    println!("installed {}", path.display());
    Ok(0)
}

/// Each check prints ✓ or ✗ with what it found; a missing adb fails the doctor.
fn doctor() -> Result<u8> {
    let run = |cmd: &Path, args: &[&str]| -> Option<String> {
        let out = std::process::Command::new(cmd).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
    };
    let mut ok = true;
    let adb = virtuoso::android::adb_path();
    match run(&adb, &["version"]) {
        Some(v) => println!("✓ adb {} ({})", v.lines().next().unwrap_or("").trim_start_matches("Android Debug Bridge version "), adb.display()),
        None => {
            ok = false;
            println!("✗ adb: not found (install Android platform-tools, or set ANDROID_HOME)");
        }
    }
    match devices() {
        Ok(d) if !d.is_empty() => println!("✓ devices: {}", d.join(", ")),
        Ok(_) => println!("✗ devices: none connected (start an emulator or plug in a phone)"),
        Err(e) => println!("✗ devices: {e}"),
    }
    match run(Path::new("curl"), &["--version"]) {
        Some(v) => println!("✓ curl {}", v.split_whitespace().nth(1).unwrap_or("")),
        None => println!("✗ curl: not found (scripts' http needs it)"),
    }
    Ok(if ok { 0 } else { 1 })
}

fn latency(cli: &Cli, count: usize) -> Result<u8> {
    if cli.platform == "ios" {
        bail!("latency measures the Android agent's round trips; it does not run on iOS yet");
    }
    let serial = pick_devices(cli, 1)?.remove(0);
    let mut d = AndroidDevice::connect(&serial)?;
    println!("{serial}, {count} round trips each:");
    let mut measure = |name: &str, op: &mut dyn FnMut(&mut AndroidDevice) -> Result<()>| -> Result<()> {
        let mut samples = Vec::with_capacity(count);
        for _ in 0..count.max(1) {
            let t = std::time::Instant::now();
            op(&mut d)?;
            samples.push(t.elapsed());
        }
        let s = virtuoso::latency::Summary::of(&samples).expect("at least one sample");
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!("{name:<11} p50 {:>7.1} ms   p95 {:>7.1} ms   max {:>7.1} ms", ms(s.p50), ms(s.p95), ms(s.max));
        Ok(())
    };
    measure("ping", &mut |d| d.ping())?;
    measure("screen", &mut |d| d.screen().map(|_| ()))?;
    measure("screenshot", &mut |d| d.screenshot().map(|_| ()))?;
    Ok(0)
}

fn list_devices() -> Result<u8> {
    for d in devices()? {
        println!("{d}");
    }
    Ok(0)
}

/// Connects to a device of the run's platform: an Android device over adb, or a booted iOS
/// simulator.
fn connect(platform: &str, serial: &str, animations_off: bool) -> Result<Box<dyn Device>> {
    Ok(match platform {
        "ios" => Box::new(virtuoso::ios::IosDevice::connect(serial)?),
        _ => Box::new(AndroidDevice::connect_with(serial, animations_off)?),
    })
}

fn pick_devices(cli: &Cli, want: usize) -> Result<Vec<String>> {
    let ios = cli.platform == "ios";
    // Simulators named whose agents answer are booted: no `simctl list` (a process and a
    // CoreSimulator round trip on every run).
    if let (true, Some(list)) = (ios, &cli.device) {
        let named: Vec<String> = list.split(',').map(|s| s.trim().to_string()).collect();
        if named.len() >= want.max(1) && named.iter().all(|d| virtuoso::ios::agent_answers(d)) {
            return Ok(named);
        }
    }
    let mut connected = if ios { virtuoso::ios::devices()? } else { devices()? };
    if !ios && connected.is_empty() && cli.device.is_none() && (cli.auto_start_emulator || cli.start_emulator.is_some()) {
        connected.push(virtuoso::android::start_emulator(cli.start_emulator.as_deref(), Duration::from_secs(300))?);
    }
    let chosen: Vec<String> = match &cli.device {
        Some(list) => list.split(',').map(|s| s.trim().to_string()).collect(),
        None => connected.iter().take(want.max(1)).cloned().collect(),
    };
    if chosen.is_empty() {
        bail!("{}", if ios { "no iOS simulator is booted (xcrun simctl list devices booted lists none)" } else { "no Android device is connected (adb devices lists none)" });
    }
    for d in &chosen {
        if !connected.contains(d) {
            bail!("device {d} is not connected ({}: {})", if ios { "booted simulators" } else { "adb devices" }, if connected.is_empty() { "none".into() } else { connected.join(", ") });
        }
    }
    if chosen.len() < want {
        bail!("{want} devices were asked for, {} are connected", chosen.len());
    }
    Ok(chosen)
}

fn mcp(cli: &Cli) -> Result<u8> {
    use std::io::BufRead;
    let device = cli.device.clone();
    let platform = cli.platform.clone();
    let mut server = virtuoso::mcp::McpServer::new(Box::new(move || {
        // The devices listed only when none is named.
        let serial = match &device {
            Some(d) => d.clone(),
            None => {
                let connected = if platform == "ios" { virtuoso::ios::devices()? } else { devices()? };
                connected.first().cloned().ok_or_else(|| anyhow!("no {} is connected", if platform == "ios" { "iOS simulator" } else { "Android device" }))?
            }
        };
        connect(&platform, &serial, true)
    }));
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(0)
}

fn screenshot(cli: &Cli, path: PathBuf) -> Result<u8> {
    let serial = pick_devices(cli, 1)?.remove(0);
    let mut d = connect(&cli.platform, &serial, false)?;
    std::fs::write(&path, d.screenshot()?)?;
    println!("{}", path.display());
    Ok(0)
}

fn hierarchy(cli: &Cli, all: bool) -> Result<u8> {
    let serial = pick_devices(cli, 1)?.remove(0);
    let mut d = connect(&cli.platform, &serial, false)?;
    let s = d.screen()?;
    for (i, e) in s.elements.iter().enumerate() {
        if !all && !s.matchable(i) {
            continue;
        }
        let b = e.bounds;
        println!(
            "{:indent$}{} [{},{} {}x{}]{}{}{}",
            "",
            e.class.rsplit('.').next().unwrap_or(""),
            b.left,
            b.top,
            b.width(),
            b.height(),
            if e.label().is_empty() { String::new() } else { format!(" \"{}\"", e.label()) },
            if e.id.is_empty() { String::new() } else { format!(" #{}", e.id.rsplit('/').next().unwrap_or("")) },
            if e.clickable() { " clickable" } else { "" },
            indent = e.depth as usize * 2
        );
    }
    Ok(0)
}

// ---- lint ----

fn lint(a: &LintArgs) -> Result<u8> {
    if a.paths.is_empty() {
        bail!("give flow files or folders to check");
    }
    let mut problems = Vec::new();
    let mut checked = 0;
    for p in &a.paths {
        if !p.exists() {
            bail!("{} does not exist", p.display());
        }
        match collect(std::slice::from_ref(p), &[], &[], None) {
            Ok(sel) => {
                for f in &sel.flows {
                    checked += 1;
                    let mut seen = HashSet::new();
                    lint_flow(f, &mut seen, &mut problems);
                }
            }
            Err(e) => problems.push(format!("{e:#}")),
        }
    }
    if problems.is_empty() {
        println!("{checked} flows OK");
        Ok(0)
    } else {
        for p in &problems {
            println!("✗ {p}");
        }
        println!("{} problem{} in {checked} flows", problems.len(), if problems.len() == 1 { "" } else { "s" });
        Ok(1)
    }
}

/// Checks the files a flow refers to exist and parse, following runFlow without looping.
fn lint_flow(f: &Flow, seen: &mut HashSet<PathBuf>, problems: &mut Vec<String>) {
    let Some(path) = &f.path else { return };
    let canon = path.canonicalize().unwrap_or(path.clone());
    if !seen.insert(canon.clone()) {
        problems.push(format!("{}: runFlow calls itself in a loop", path.display()));
        return;
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut files = Vec::new();
    let all: Vec<&Step> = f.config.on_flow_start.iter().chain(&f.config.on_flow_complete).chain(&f.commands).collect();
    for s in all {
        refs(&s.cmd, &mut files);
    }
    for (kind, file) in files {
        if file.contains("${") {
            continue;
        }
        let p = dir.join(&file);
        if !p.exists() {
            problems.push(format!("{}: {kind} {file}: no such file", path.display()));
            continue;
        }
        if kind == "runFlow" {
            match std::fs::read_to_string(&p).map_err(anyhow::Error::from).and_then(|t| Ok(virtuoso::flow::parse_flow(&t, Some(&p))?)) {
                Ok(sub) => lint_flow(&sub, seen, problems),
                Err(e) => problems.push(format!("{e:#}")),
            }
        }
    }
    seen.remove(&canon);
}

fn refs(c: &Cmd, out: &mut Vec<(&'static str, String)>) {
    let nested: &[Step] = match c {
        Cmd::RunFlow(r) => {
            if let Some(f) = &r.file {
                out.push(("runFlow", f.clone()));
            }
            &r.commands
        }
        Cmd::Retry(r) => {
            if let Some(f) = &r.file {
                out.push(("runFlow", f.clone()));
            }
            &r.commands
        }
        Cmd::Repeat { commands, .. } => commands,
        Cmd::RunScript { file, .. } => {
            out.push(("runScript", file.clone()));
            &[]
        }
        _ => &[],
    };
    for s in nested {
        refs(&s.cmd, out);
    }
}

// ---- test ----

struct Ui {
    ansi: bool,
}

impl Ui {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.ansi {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn step(&self, shard: Option<usize>, s: &StepResult) {
        let (word, code) = match s.status {
            StepStatus::Passed => ("COMPLETED", "32"),
            StepStatus::Failed => ("FAILED", "31"),
            StepStatus::Warned => ("WARNED", "33"),
            StepStatus::Skipped => ("SKIPPED", "90"),
        };
        let prefix = shard.map(|n| format!("[shard {n}] ")).unwrap_or_default();
        let mut line = format!("{prefix}{:indent$}{}... {} {}", "", s.name, self.paint(code, word), fmt_ms(s.duration), indent = 2 * s.depth.saturating_sub(1));
        if let Some(e) = &s.error {
            if s.status != StepStatus::Passed {
                line.push_str(&format!("\n{prefix}    {}", self.paint(code, e)));
            }
        }
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
    }
}

fn fmt_ms(d: Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.1} s", d.as_secs_f64())
    }
}

/// `test --continuous`: a run, then another each time the flows change, until interrupted.
fn continuous(cli: &Cli, a: &TestArgs) -> Result<u8> {
    let mut seen = virtuoso::workspace::snapshot(&a.paths);
    loop {
        if let Err(e) = test(cli, a) {
            eprintln!("virtuoso: {e:#}");
        }
        println!("watching {} for changes (Ctrl-C to stop)", a.paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "));
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let now = virtuoso::workspace::snapshot(&a.paths);
            if now != seen {
                seen = now;
                break;
            }
        }
    }
}

fn test(cli: &Cli, a: &TestArgs) -> Result<u8> {
    if a.paths.is_empty() {
        bail!("give flow files or folders to run");
    }
    for p in &a.paths {
        if !p.exists() {
            bail!("{} does not exist", p.display());
        }
    }
    for apk in &a.app_file {
        if !apk.is_file() {
            bail!("--app-file {}: no such file", apk.display());
        }
    }
    if a.analyze {
        bail!("--analyze (Maestro's AI analysis) is not supported");
    }
    let mut sel = collect(&a.paths, &a.include_tags, &a.exclude_tags, a.config.as_deref())?;
    if sel.flows.is_empty() {
        bail!("no flows to run (check the paths and tags)");
    }
    let mut env = Vec::new();
    if let Some(f) = &a.env_file {
        let text = std::fs::read_to_string(f).map_err(|e| anyhow!("--env-file {}: {e}", f.display()))?;
        env = virtuoso::envfile::parse_env_file(&text).map_err(|e| anyhow!("--env-file {}: {e}", f.display()))?;
    }
    for kv in &a.env {
        let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("-e takes KEY=VALUE, not {kv}"))?;
        env.retain(|(e, _)| e != k);
        env.push((k.to_string(), v.to_string()));
    }
    // MAESTRO_* variables from the shell reach flows, as in Maestro.
    for (k, v) in std::env::vars() {
        if k.starts_with("MAESTRO_") && !env.iter().any(|(e, _)| *e == k) {
            env.push((k, v));
        }
    }
    let shards = a.shard_split.or(a.shard_all).unwrap_or(1);
    let serials = pick_devices(cli, shards)?;
    let serials: Vec<String> = serials.into_iter().take(shards).collect();
    // Each device's installs at once with the others' (they are apart); all done before a flow.
    std::thread::scope(|s| {
        let installs: Vec<_> = serials
            .iter()
            .map(|serial| {
                s.spawn(move || -> Result<()> {
                    for app in &a.app_file {
                        if cli.platform == "ios" {
                            virtuoso::ios::install(serial, app).map_err(|e| anyhow!("--app-file {}: {e}", app.display()))?;
                        } else {
                            // -r keeps the app's data, -t allows test builds.
                            virtuoso::android::Adb::new(serial).run(&["install", "-r", "-t", &app.to_string_lossy()]).map_err(|e| anyhow!("--app-file {}: {e}", app.display()))?;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        installs.into_iter().try_for_each(|h| h.join().map_err(|_| anyhow!("installing an --app-file panicked"))?)
    })?;
    let output_root = a.debug_output.clone().or(sel.test_output_dir.clone()).unwrap_or_else(default_output_dir);
    let mut opts = RunOptions { env, video: a.video, ..Default::default() };
    if let Some(ms) = a.find_timeout {
        opts.lookup_timeout = Duration::from_millis(ms);
        opts.optional_lookup_timeout = opts.lookup_timeout.min(opts.optional_lookup_timeout);
    }
    let ui = Arc::new(Ui { ansi: !a.no_ansi && std::env::var_os("NO_COLOR").is_none() && std::env::var_os("CI").is_none() });

    let reports = if serials.len() == 1 {
        run_on_device(&cli.platform, &serials[0], None, flows_queue(std::mem::take(&mut sel.flows), a.shard_all.is_some(), 1).remove(0), &opts, &output_root, a.keep_animations, &ui, sel.continue_on_failure, a.retries)?
    } else {
        let queues = flows_queue(std::mem::take(&mut sel.flows), a.shard_all.is_some(), serials.len());
        let mut handles = Vec::new();
        for (n, (serial, queue)) in serials.iter().cloned().zip(queues).enumerate() {
            let (opts, root, ui) = (opts.clone(), output_root.join(format!("shard-{}", n + 1)), ui.clone());
            let keep = a.keep_animations;
            let cont = sel.continue_on_failure;
            let retries = a.retries;
            let platform = cli.platform.clone();
            handles.push(std::thread::spawn(move || run_on_device(&platform, &serial, Some(n + 1), queue, &opts, &root, keep, &ui, cont, retries)));
        }
        let mut all = Vec::new();
        for h in handles {
            all.extend(h.join().map_err(|_| anyhow!("a shard crashed"))??);
        }
        all
    };

    let suite = SuiteReport { name: a.test_suite_name.clone(), device: serials.join(","), flows: reports };
    let total = suite.flows.len();
    if total > 1 || a.format != "noop" {
        for f in &suite.flows {
            let mark = if f.status == FlowStatus::Passed { ui.paint("32", "[Passed]") } else { ui.paint("31", "[Failed]") };
            println!("{mark} {} ({}){}", f.name, fmt_ms(f.duration), f.error.as_ref().map(|e| format!(" ({e})")).unwrap_or_default());
        }
    }
    let failed = suite.failed();
    if failed == 0 {
        println!("{}", ui.paint("32", &format!("{total}/{total} Flow{} Passed in {}", if total == 1 { "" } else { "s" }, fmt_ms(suite.duration()))));
    } else {
        println!("{}", ui.paint("31", &format!("{failed}/{total} Flow{} Failed", if total == 1 { "" } else { "s" })));
        println!("Debug output: {}", output_root.display());
    }
    write_report(&suite, &a.format, a.output.as_deref())?;
    Ok(if failed == 0 { 0 } else { 1 })
}

/// The flows each device runs. With shard-split they share one queue (a device that finishes
/// early takes the next flow); with shard-all each device has all of them.
fn flows_queue(flows: Vec<Flow>, all: bool, devices: usize) -> Vec<Arc<Mutex<VecDeque<Arc<Flow>>>>> {
    // Shared, not cloned: a device's queue holds the same flows as the others'.
    let flows: Vec<Arc<Flow>> = flows.into_iter().map(Arc::new).collect();
    if all {
        (0..devices).map(|_| Arc::new(Mutex::new(flows.iter().cloned().collect()))).collect()
    } else {
        let q = Arc::new(Mutex::new(flows.into_iter().collect::<VecDeque<_>>()));
        (0..devices).map(|_| q.clone()).collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn run_on_device(
    platform: &str,
    serial: &str,
    shard: Option<usize>,
    queue: Arc<Mutex<VecDeque<Arc<Flow>>>>,
    opts: &RunOptions,
    output_root: &Path,
    keep_animations: bool,
    ui: &Arc<Ui>,
    continue_on_failure: bool,
    retries: u32,
) -> Result<Vec<FlowReport>> {
    let mut dev = connect(platform, serial, !keep_animations)?;
    let mut reports = Vec::new();
    loop {
        let Some(flow) = queue.lock().unwrap().pop_front() else { break };
        let name = virtuoso::runner::flow_name(&flow);
        let dir = output_root.join(sanitize(&name));
        let mut o = opts.clone();
        o.output_dir = Some(dir.clone());
        if let Some(n) = shard {
            o.env.push(("MAESTRO_SHARD_ID".into(), n.to_string()));
            o.env.push(("MAESTRO_SHARD_INDEX".into(), (n - 1).to_string()));
        }
        let mut attempt = 0;
        let result = loop {
            println!("{}", ui.paint("1", &format!("{}> Flow {name}", shard.map(|n| format!("[shard {n}] ")).unwrap_or_default())));
            let ui2 = ui.clone();
            let result = {
                let mut runner = Runner::new(dev.as_mut(), o.clone());
                runner.on_step = Some(Box::new(move |s| ui2.step(shard, s)));
                runner.run_flow(&flow)
            };
            if result.status == FlowStatus::Passed || attempt >= retries {
                break result;
            }
            attempt += 1;
            println!("{}", ui.paint("33", &format!("Retrying {name} ({attempt} of {retries})")));
        };
        if result.status == FlowStatus::Failed {
            save_failure(dev.as_mut(), &dir);
        }
        let failed = result.status == FlowStatus::Failed;
        reports.push(FlowReport {
            name: result.name,
            file: flow.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            status: result.status,
            duration: result.duration,
            error: result.error,
            steps: result.steps,
            tags: flow.config.tags.clone(),
            properties: flow.config.properties.clone(),
        });
        if failed && !continue_on_failure {
            queue.lock().unwrap().clear();
        }
    }
    Ok(reports)
}

/// A screenshot and the screen's nodes, to see what the flow saw when it failed.
fn save_failure(dev: &mut dyn Device, dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(png) = dev.screenshot() {
        let _ = std::fs::write(dir.join("failure.png"), png);
    }
    if let Ok(s) = dev.screen() {
        let lines: Vec<String> = s
            .elements
            .iter()
            .enumerate()
            .filter(|(i, e)| s.matchable(*i) && !(e.label().is_empty() && e.id.is_empty()))
            .map(|(_, e)| format!("\"{}\" id={} bounds={} package={}", e.label(), e.id, e.bounds, e.package))
            .collect();
        let _ = std::fs::write(dir.join("screen.txt"), lines.join("\n"));
    }
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn default_output_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    home.join(".virtuoso").join("tests").join(timestamp(secs))
}

/// `yyyy-MM-dd_HHmmss` in UTC.
fn timestamp(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}_{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn write_report(s: &SuiteReport, format: &str, output: Option<&Path>) -> Result<()> {
    let (text, default) = match format {
        "noop" => return Ok(()),
        "allure" => {
            // --output is the results folder Allure reads.
            let dir = output.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("allure-results"));
            std::fs::create_dir_all(&dir)?;
            let ended = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64;
            for (name, text) in virtuoso::report::allure(s, ended.saturating_sub(s.duration().as_millis() as u64)) {
                std::fs::write(dir.join(name), text)?;
            }
            return Ok(());
        }
        "junit" => (virtuoso::report::junit(s), "report.xml"),
        "html" | "html-detailed" => (virtuoso::report::html(s), "report.html"),
        "json" => (virtuoso::report::json(s), "report.json"),
        other => bail!("--format {other}: use junit, html, json, allure or noop"),
    };
    let path = output.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(default));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, text)?;
    Ok(())
}
