//! Measuring only: how Chrome's start splits up, under several ways to launch and connect to it,
//! the variants taking turns (shuffled each round). One JSON line per launch on stdout, each
//! time in ms from the spawn.
//!
//! `cargo run --release --example startbench -- <chrome> <rounds> [variants,...]`
//! Env: STARTBENCH_SHELL (a headless shell for the `shell` variant), STARTBENCH_WINDOW (`w,h`),
//! STARTBENCH_TRACE_DIR (also a few launches with Chrome's startup trace written there).

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use virtuoso::chrome::{launch_args_for, parse_active_port, Cdp, Transport};
use virtuoso::websocket::{WebSocket, LAST_CONNECT};

struct Times {
    spawned: Instant,
    at: Vec<(&'static str, f64)>,
}

impl Times {
    fn mark(&mut self, name: &'static str) {
        self.mark_at(name, Instant::now());
    }
    fn mark_at(&mut self, name: &'static str, t: Instant) {
        self.at.push((name, t.saturating_duration_since(self.spawned).as_secs_f64() * 1000.0));
    }
}

fn args_for(variant: &str, binary: &Path, profile: &Path, window: (i32, i32), trace: Option<&Path>) -> Vec<String> {
    let mut a = launch_args_for(binary, profile, true, window);
    match variant {
        "bare" => {
            // Only what a launch cannot do without: headless, no GPU, its window, a port, a
            // profile, no first-run prompts, the mock keychain, a blank tab.
            let keep = ["--headless", "--disable-gpu", "--window-size", "--remote-debugging-port", "--no-first-run", "--user-data-dir", "--use-mock-keychain", "--password-store", "about:blank"];
            a.retain(|x| keep.iter().any(|k| x.starts_with(k)));
        }
        "netproc" => a.insert(0, "--enable-features=NetworkServiceInProcess2".into()),
        "nosandbox" => a.insert(0, "--no-sandbox".into()),
        // As Playwright launches: no window at the start, the first tab made once connected.
        "nowindow" => {
            a.retain(|x| x != "about:blank");
            a.insert(0, "--no-startup-window".into());
        }
        "pipe" | "pipe-nowindow" => {
            if variant == "pipe-nowindow" {
                a.retain(|x| x != "about:blank");
                a.insert(0, "--no-startup-window".into());
            }
            for x in a.iter_mut() {
                if x.starts_with("--remote-debugging-port") {
                    *x = "--remote-debugging-pipe".into();
                }
            }
        }
        _ => {}
    }
    if let Some(dir) = trace {
        a.insert(0, "--trace-startup=-*,toplevel,startup,browser,devtools,navigation,loading,content,benchmark,disabled-by-default-devtools.timeline".into());
        a.insert(0, format!("--trace-startup-file={}", dir.display()));
        a.insert(0, "--trace-startup-duration=4".into());
        a.insert(0, "--trace-startup-format=json".into());
    }
    a
}

#[cfg(unix)]
mod pipe {
    use anyhow::{bail, Result};
    use std::io::{BufRead, Write};
    use std::os::fd::{FromRawFd, RawFd};
    use std::os::unix::process::CommandExt;
    use std::sync::mpsc;
    use std::time::Duration;

    pub struct Pipe {
        tx: std::fs::File,
        rx: mpsc::Receiver<String>,
    }

    fn cloexec_pipe() -> [RawFd; 2] {
        let mut fds = [0; 2];
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
            for fd in fds {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
        fds
    }

    fn high(fd: RawFd) -> RawFd {
        unsafe {
            let h = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 100);
            libc::close(fd);
            h
        }
    }

    /// Sets up fds 3 (Chrome reads) and 4 (Chrome writes) for the child; the parent's ends
    /// after `opened`.
    pub fn prepare(cmd: &mut std::process::Command) -> (RawFd, RawFd, RawFd, RawFd) {
        let to_chrome = cloexec_pipe();
        let from_chrome = cloexec_pipe();
        let (child_r, child_w) = (high(to_chrome[0]), high(from_chrome[1]));
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(child_r, 3) < 0 || libc::dup2(child_w, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        (child_r, child_w, to_chrome[1], from_chrome[0])
    }

    pub fn opened(fds: (RawFd, RawFd, RawFd, RawFd)) -> Pipe {
        unsafe {
            libc::close(fds.0);
            libc::close(fds.1);
        }
        let tx = unsafe { std::fs::File::from_raw_fd(fds.2) };
        let rxf = unsafe { std::fs::File::from_raw_fd(fds.3) };
        let (s, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(rxf);
            let mut buf = Vec::new();
            while r.read_until(0, &mut buf).map(|n| n > 0).unwrap_or(false) {
                if buf.last() == Some(&0) {
                    buf.pop();
                }
                if s.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                    break;
                }
                buf.clear();
            }
        });
        Pipe { tx, rx }
    }

    impl virtuoso::chrome::Transport for Pipe {
        fn send_text(&mut self, text: &str) -> Result<()> {
            let mut b = Vec::with_capacity(text.len() + 1);
            b.extend_from_slice(text.as_bytes());
            b.push(0);
            self.tx.write_all(&b)?;
            Ok(())
        }
        fn recv_text(&mut self, timeout: Duration) -> Result<Option<String>> {
            match self.rx.recv_timeout(timeout) {
                Ok(s) => Ok(Some(s)),
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(_) => bail!("the pipe closed"),
            }
        }
    }
}

/// From a connection to the first page loaded: ready (the targets known), the blank tab
/// attached, its renderer answering, a page navigated to and loaded; then Chrome closed.
fn drive<T: Transport>(cdp: &mut Cdp<T>, t: &mut Times, child: &mut Child, linger: Duration) -> Result<()> {
    let calls = [("Target.setDiscoverTargets", json!({"discover": true})), ("Target.getTargets", json!({}))];
    let targets = match cdp.calls(None, &calls, &|_| false)? {
        virtuoso::chrome::Answer::Reply(v) | virtuoso::chrome::Answer::Stopped(v) => v,
    };
    t.mark("ready");
    let at = Instant::now();
    cdp.call(None, "Browser.getVersion", json!({}))?;
    t.at.push(("getversion_rtt", at.elapsed().as_secs_f64() * 1000.0));
    let infos = targets["targetInfos"].as_array().cloned().unwrap_or_default();
    let target = match infos.iter().find(|x| x["type"] == "page" && x["url"] == "about:blank") {
        Some(x) => x["targetId"].as_str().unwrap_or_default().to_string(),
        None => {
            t.at.push(("no_blank_tab", 1.0));
            cdp.call(None, "Target.createTarget", json!({"url": "about:blank"}))?["targetId"].as_str().unwrap_or_default().to_string()
        }
    };
    let session = cdp.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}))?["sessionId"].as_str().unwrap_or_default().to_string();
    t.mark("attached");
    cdp.call(Some(&session), "Runtime.evaluate", json!({"expression": "1+1", "returnByValue": true}))?;
    t.mark("evaluated");
    cdp.call(Some(&session), "Page.enable", json!({}))?;
    cdp.call(Some(&session), "Page.navigate", json!({"url": "data:text/html,<title>x</title><p>hello</p>"}))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match cdp.next_event(Duration::from_millis(500))? {
            Some(e) if e["method"] == "Page.loadEventFired" && e["sessionId"].as_str() == Some(&session) => break,
            _ if Instant::now() > deadline => bail!("no load"),
            _ => {}
        }
    }
    t.mark("loaded");
    std::thread::sleep(linger);
    let at = Instant::now();
    cdp.call_timeout = Duration::from_secs(2);
    let _ = cdp.call(None, "Browser.close", json!({}));
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && matches!(child.try_wait(), Ok(None)) {
        std::thread::sleep(Duration::from_millis(1));
    }
    t.at.push(("close", at.elapsed().as_secs_f64() * 1000.0));
    Ok(())
}

/// When Chrome printed "DevTools listening on <url>", and the url (stderr drained for ever).
fn watch_stderr(child: &mut Child) -> (std::sync::Arc<std::sync::OnceLock<Instant>>, std::sync::mpsc::Receiver<String>) {
    let seen = std::sync::Arc::new(std::sync::OnceLock::new());
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(err) = child.stderr.take() {
        let s = seen.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut r = std::io::BufReader::new(err);
            let mut line = String::new();
            while r.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                if let Some(url) = line.trim().strip_prefix("DevTools listening on ") {
                    let _ = s.set(Instant::now());
                    let _ = tx.send(url.to_string());
                }
                line.clear();
            }
        });
    }
    (seen, rx)
}

fn once(variant: &str, binary: &Path, root: &Path, n: u32, window: (i32, i32), trace: Option<&Path>) -> Result<Vec<(&'static str, f64)>> {
    let profile = root.join(format!("p{n}"));
    std::fs::create_dir_all(&profile)?;
    let mut cmd = Command::new(binary);
    cmd.args(args_for(variant, binary, &profile, window, trace)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(unix)]
    let fds = variant.starts_with("pipe").then(|| pipe::prepare(&mut cmd));
    #[cfg(not(unix))]
    if variant.starts_with("pipe") {
        bail!("no pipe here");
    }
    let began = Instant::now();
    let mut child = cmd.spawn()?;
    let mut t = Times { spawned: Instant::now(), at: Vec::new() };
    t.at.push(("spawn_call", t.spawned.duration_since(began).as_secs_f64() * 1000.0));
    let (stderr_line, urls) = watch_stderr(&mut child);
    // A trace is written once its duration is over: Chrome kept open till then.
    let linger = if trace.is_some() { Duration::from_millis(4500) } else { Duration::ZERO };
    let r = (|| -> Result<()> {
        #[cfg(unix)]
        if let Some(fds) = fds {
            let mut cdp = Cdp::new(pipe::opened(fds));
            return drive(&mut cdp, &mut t, &mut child, linger);
        }
        if variant == "stderr" {
            let url = urls.recv_timeout(Duration::from_secs(30))?;
            t.mark("port_file");
            let ws = WebSocket::connect(&url)?;
            t.mark("connected");
            if let Some([tcp, sent, upgraded]) = *LAST_CONNECT.lock().unwrap() {
                t.mark_at("tcp", tcp);
                t.mark_at("upgrade_sent", sent);
                t.mark_at("upgrade_101", upgraded);
            }
            let mut cdp = Cdp::new(ws);
            return drive(&mut cdp, &mut t, &mut child, linger);
        }
        let file = profile.join("DevToolsActivePort");
        let deadline = Instant::now() + Duration::from_secs(30);
        let (port, path) = loop {
            if let Some(pp) = std::fs::read_to_string(&file).ok().as_deref().and_then(parse_active_port) {
                break pp;
            }
            if Instant::now() > deadline {
                bail!("no port file");
            }
            if let Ok(Some(s)) = child.try_wait() {
                bail!("Chrome exited ({s})");
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        t.mark("port_file");
        let ws = WebSocket::connect(&format!("ws://127.0.0.1:{port}{path}"))?;
        t.mark("connected");
        if let Some([tcp, sent, upgraded]) = *LAST_CONNECT.lock().unwrap() {
            t.mark_at("tcp", tcp);
            t.mark_at("upgrade_sent", sent);
            t.mark_at("upgrade_101", upgraded);
        }
        let mut cdp = Cdp::new(ws);
        drive(&mut cdp, &mut t, &mut child, linger)
    })();
    if let Some(s) = stderr_line.get() {
        t.mark_at("stderr_line", *s);
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&profile);
    r.map(|_| t.at)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let binary = PathBuf::from(args.first().ok_or_else(|| anyhow!("usage: startbench <chrome> <rounds> [variants]"))?);
    let rounds: u32 = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(10);
    let mut variants: Vec<String> = args.get(2).map_or("port,bare,netproc,nosandbox".into(), String::clone).split(',').map(String::from).collect();
    let shell = std::env::var("STARTBENCH_SHELL").ok().filter(|s| !s.is_empty()).map(PathBuf::from);
    if shell.is_none() {
        variants.retain(|v| v != "shell");
    }
    let window = std::env::var("STARTBENCH_WINDOW").ok().and_then(|s| s.split_once(',').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))).unwrap_or((1280, 800));
    let root = std::env::temp_dir().join(format!("startbench-{}", std::process::id()));
    let mut n = 0;
    let mut seed: u64 = 0x9e3779b97f4a7c15 ^ std::process::id() as u64;
    // Round 0 a warm-up, not reported.
    for round in 0..=rounds {
        let mut order = variants.clone();
        for i in (1..order.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            order.swap(i, (seed >> 33) as usize % (i + 1));
        }
        for v in &order {
            n += 1;
            let bin = if v == "shell" { shell.clone().unwrap() } else { binary.clone() };
            let v_run = if v == "shell" { "port" } else { v.as_str() };
            match once(v_run, &bin, &root, n, window, None) {
                Ok(at) if round > 0 => {
                    let m: serde_json::Map<String, Value> = at.into_iter().map(|(k, x)| (k.to_string(), json!((x * 10.0).round() / 10.0))).collect();
                    println!("{}", json!({"variant": v, "round": round, "t": m}));
                }
                Ok(_) => {}
                Err(e) => println!("{}", json!({"variant": v, "round": round, "error": e.to_string()})),
            }
        }
    }
    if let Ok(dir) = std::env::var("STARTBENCH_TRACE_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        for i in 0..3 {
            n += 1;
            let file = dir.join(format!("startup-trace-{i}.json"));
            match once("port", &binary, &root, n, window, Some(&file)) {
                Ok(at) => println!("{}", json!({"variant": "trace", "round": i, "t": at.into_iter().map(|(k, x)| (k.to_string(), json!(x))).collect::<serde_json::Map<_, _>>()})),
                Err(e) => println!("{}", json!({"variant": "trace", "error": e.to_string()})),
            }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
