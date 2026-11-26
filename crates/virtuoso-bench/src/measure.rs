//! Running one tool invocation and measuring it: wall time, peak memory and CPU time.
//!
//! Native Windows processes are measured through their process handle (peak working set,
//! kernel + user time, the CPU of the whole process tree through a job). A process inside WSL is
//! timed from inside Linux by a small Python timer (WSL_TIMER): wsl.exe's own start-up is left out.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// How long one run may take before it is stopped and counted as failed: a tool that hangs
/// would otherwise hold up the whole benchmark.
pub const RUN_LIMIT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Measurement {
    pub wall_ms: f64,
    pub peak_rss_kb: Option<f64>,
    pub cpu_ms: Option<f64>,
    pub exit: i32,
    pub output: String,
    /// How many processes ran, the tool's own included (native runs, through the job).
    #[serde(default)]
    pub processes: Option<u32>,
}

/// Reads the timer's line: wall seconds, peak RSS KB, user and system seconds, exit status.
pub fn parse_time_line(output: &str) -> Option<(f64, f64, f64, i32)> {
    let line = output.lines().rev().find(|l| l.contains("VIRTUOSO_BENCH_TIME"))?;
    let f: Vec<&str> = line.split("VIRTUOSO_BENCH_TIME").nth(1)?.split_whitespace().collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok());
    Some((num(0)? * 1000.0, num(1)?, (num(2)? + num(3)?) * 1000.0, num(4)? as i32))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Runner {
    /// A Windows program, measured through its process handle.
    Native,
    /// A Linux program in this WSL distribution, measured by GNU time inside it.
    Wsl { distro: String },
}

/// Runs `argv` (with `env` and `cwd`) and measures it, within RUN_LIMIT.
pub fn run(runner: &Runner, argv: &[String], env: &[(String, String)], cwd: &str) -> Result<Measurement> {
    run_for(runner, argv, env, cwd, RUN_LIMIT)
}

/// Runs and measures `argv`; at `limit` it and everything it started are stopped, and the run
/// fails with "timed out after N s" in its output.
pub fn run_for(runner: &Runner, argv: &[String], env: &[(String, String)], cwd: &str, limit: Duration) -> Result<Measurement> {
    match runner {
        Runner::Native => run_native(argv, env, cwd, limit),
        Runner::Wsl { distro } => {
            let mut full = vec!["wsl".to_string(), "-d".into(), distro.clone(), "-u".into(), "root".into(), "--".into()];
            full.extend(["python3".into(), "-c".into(), WSL_TIMER.into(), cwd.into(), limit.as_secs().to_string()]);
            full.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
            full.push("--".into());
            full.extend(argv.iter().cloned());
            let start = Instant::now();
            let out = Command::new(&full[0]).args(&full[1..]).stdin(Stdio::null()).output().context("running wsl")?;
            let outer = start.elapsed().as_secs_f64() * 1000.0;
            let text = clean(&format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
            let (wall, rss, cpu, exit) = parse_time_line(&text).ok_or_else(|| anyhow!("no timing from the WSL timer (outer {outer:.0} ms): {text}"))?;
            Ok(Measurement { wall_ms: wall, peak_rss_kb: Some(rss), cpu_ms: Some(cpu), exit, output: text, processes: None })
        }
    }
}

/// Times a command inside Linux to the microsecond, as GNU time would but finer (GNU time's
/// wall clock has 10 ms steps): wall time, the peak RSS of the largest process in the tree,
/// and the CPU time of the command and every child it waited for.
/// At the limit the command's whole session is killed.
/// argv: cwd, limit seconds, KEY=VALUE..., --, command...
pub const WSL_TIMER: &str = r#"
import os, re, resource, signal, subprocess, sys, time
a = sys.argv[1:]
cwd = a.pop(0)
limit = int(a.pop(0))
env = dict(os.environ)
while a and a[0] != "--":
    k, v = a.pop(0).split("=", 1)
    env[k] = v
a.pop(0)
# GNU time reports the peak RSS: it forks from a ~2 MB C program, where a fork of this
# Python process would count Python's own memory.
t = time.perf_counter()
p = subprocess.Popen(["/usr/bin/time", "-f", "VIRTUOSO_RSS %M"] + a, cwd=cwd, env=env, stderr=subprocess.PIPE, start_new_session=True)
try:
    _, err = p.communicate(timeout=limit)
    code = p.returncode
except subprocess.TimeoutExpired:
    os.killpg(p.pid, signal.SIGKILL)
    _, err = p.communicate()
    err += b"\ntimed out after %d s\n" % limit
    code = -1
wall = time.perf_counter() - t
r = resource.getrusage(resource.RUSAGE_CHILDREN)
err = err.decode("utf-8", "replace")
m = re.findall(r"VIRTUOSO_RSS (\d+)", err)
sys.stdout.flush()
sys.stderr.write(re.sub(r"VIRTUOSO_RSS \d+\s*", "", err))
print("VIRTUOSO_BENCH_TIME %.4f %s %.4f %.4f %d" % (wall, m[-1] if m else "0", r.ru_utime, r.ru_stime, code), file=sys.stderr)
"#;

/// WSL prints UTF-16 notices and systemd warnings; keep only the tool's text.
fn clean(s: &str) -> String {
    s.replace('\0', "")
        .lines()
        .filter(|l| !l.contains("systemd") && !l.contains("networkingMode"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_native(argv: &[String], env: &[(String, String)], cwd: &str, limit: Duration) -> Result<Measurement> {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    // Its own process group, so that at the limit everything it started goes with it.
    #[cfg(all(unix, not(target_os = "linux")))]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    // On Linux its own session (and so its own process group), and a tag in its environment:
    // what identifies the processes it starts once they are orphaned (see `linux`).
    #[cfg(target_os = "linux")]
    let tag = linux::prepare(&mut cmd);
    let start = Instant::now();
    let mut child = cmd.spawn().with_context(|| format!("starting {}", argv[0]))?;
    // Both pipes drain on their own threads: a tool that fills one would otherwise block.
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    // A duplicate handle keeps the process's numbers readable after it exits.
    #[cfg(windows)]
    let handle = win::Handle::of(&child);
    // A job collects the CPU time of every process the tool starts (adb, its agent installer...),
    // as GNU time does for a Linux process and the children it waits for.
    #[cfg(windows)]
    let job = win::Job::around(&child);
    #[cfg(windows)]
    let stop = {
        let (job, pid) = (job.as_ref().map(|j| j.raw()), child.id());
        move || win::stop(job, pid)
    };
    #[cfg(unix)]
    let stop = {
        let pid = child.id() as libc::pid_t;
        // SAFETY: a signal to our own child's group; the child is not reaped before the watchdog finishes.
        move || unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    };
    #[cfg(not(any(windows, unix)))]
    let stop = || {};
    let watchdog = Watchdog::start(limit, stop);
    #[cfg(all(unix, not(target_os = "linux")))]
    let waited = unix::wait(&child);
    // Linux: the tool's exit, its process left unreaped (so its process ID, and with it its
    // session's and group's, cannot be reused while its orphans are collected).
    #[cfg(target_os = "linux")]
    let waited = linux::exited(child.id() as libc::pid_t);
    #[cfg(not(unix))]
    let waited = child.wait().map(|s| (s.code().unwrap_or(-1), None, None)).map_err(anyhow::Error::from);
    let wall = start.elapsed().as_secs_f64() * 1000.0;
    let timed_out = watchdog.finish();
    // Then the processes it left behind, counted for its run, and its own numbers.
    #[cfg(target_os = "linux")]
    let waited = waited.and_then(|()| {
        let orphans = linux::collect(child.id() as libc::pid_t, &tag, if timed_out { Duration::ZERO } else { linux::ORPHAN_GRACE });
        let (exit, peak, cpu) = unix::wait(&child)?;
        Ok((exit, peak.map(|p| p.max(orphans.peak_kb)), cpu.map(|c| c + orphans.cpu_ms)))
    });
    let (exit, peak, cpu) = waited?;
    #[cfg(windows)]
    let (peak, cpu) = handle.map(|h| (h.peak_working_set_kb(), job.as_ref().and_then(|j| j.cpu_ms()).or_else(|| h.cpu_ms()))).unwrap_or((peak, cpu));
    #[cfg(windows)]
    let processes = job.as_ref().and_then(|j| j.processes());
    #[cfg(not(windows))]
    let processes = None;
    let mut text = format!("{}{}", String::from_utf8_lossy(&out.join().unwrap_or_default()), String::from_utf8_lossy(&err.join().unwrap_or_default()));
    let exit = if timed_out {
        text.push_str(&format!("\ntimed out after {} s\n", limit.as_secs()));
        -1
    } else {
        exit
    };
    Ok(Measurement { wall_ms: wall, peak_rss_kb: peak, cpu_ms: cpu, exit, output: text, processes })
}

/// A pipe read to its end on its own thread.
fn drain(r: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut buf);
        }
        buf
    })
}

/// Stops a run at its limit unless it finishes first.
struct Watchdog {
    done: mpsc::Sender<()>,
    fired: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl Watchdog {
    fn start(limit: Duration, stop: impl FnOnce() + Send + 'static) -> Watchdog {
        let (done, rx) = mpsc::channel::<()>();
        let fired = Arc::new(AtomicBool::new(false));
        let flag = fired.clone();
        let thread = std::thread::spawn(move || {
            if rx.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
                flag.store(true, Ordering::SeqCst);
                stop();
            }
        });
        Watchdog { done, fired, thread }
    }

    /// Whether it stopped the run. Once it returns, it no longer touches the run.
    fn finish(self) -> bool {
        let _ = self.done.send(());
        let _ = self.thread.join();
        self.fired.load(Ordering::SeqCst)
    }
}

/// Linux and macOS: the child's own resource usage from wait4, as GNU time reads it: its peak
/// resident memory, and the CPU time of it and every process it waited for. On Linux the
/// processes it orphaned are added (see `linux`). macOS has no child subreaper
/// (PR_SET_CHILD_SUBREAPER), so there an orphan (Selenium's Chrome, left by its driver) goes to
/// launchd and is not counted.
#[cfg(unix)]
mod unix {
    use anyhow::Result;
    use std::process::Child;

    /// The child's exit status, peak memory (KB) and CPU time (ms).
    pub fn wait(child: &Child) -> Result<(i32, Option<f64>, Option<f64>)> {
        let mut status = 0;
        // SAFETY: rusage is plain data; wait4 fills it for our own child, which nothing else waits on.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let pid = unsafe { libc::wait4(child.id() as libc::pid_t, &mut status, 0, &mut usage) };
        anyhow::ensure!(pid > 0, "waiting for the tool: {}", std::io::Error::last_os_error());
        let exit = if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { -1 };
        // Linux reports the peak in KB, macOS in bytes.
        let peak = if cfg!(target_os = "macos") { usage.ru_maxrss as f64 / 1024.0 } else { usage.ru_maxrss as f64 };
        Ok((exit, Some(peak), Some(cpu_ms(&usage))))
    }

    /// User + system time, in ms.
    pub fn cpu_ms(usage: &libc::rusage) -> f64 {
        let ms = |t: libc::timeval| t.tv_sec as f64 * 1000.0 + t.tv_usec as f64 / 1000.0;
        ms(usage.ru_utime) + ms(usage.ru_stime)
    }
}

/// Linux: the processes a tool orphans count for its run.
///
/// wait4 counts a process and the descendants it waited for. A process whose parent exits
/// without waiting for it (Selenium's chromedriver leaves Chrome so) is reparented and counted
/// for no one. So the harness makes itself a child subreaper (PR_SET_CHILD_SUBREAPER): such a
/// process is reparented to the harness rather than to init. Once the tool exits, the harness
/// collects every process of the tool's: those in its session or process group (each tool runs
/// as a session of its own), or with its run's tag in their environment (one that started a
/// session or group of its own; Puppeteer and ChromeDriver start Chrome so). Each is reaped
/// with wait4 when it ends: its CPU time (with that of the processes it waited for) is added to
/// the run's, and its peak memory counts when larger than the tool's, as the metric is the
/// largest single process. One still running ORPHAN_GRACE after the tool exited (at once when
/// the run timed out) is killed, then reaped and counted the same way: a daemon a tool leaves
/// behind is the tool's cost, and no run inherits another's. The wall time stays the tool's exit.
///
/// Only processes identified as the run's are waited for, each by its ID, so runs at the same
/// time (the tests run so) and the harness's other children (adb) are never taken, and the
/// collection gives up KILL_WAIT after the kill. A process that left the session and ended
/// before the tool did is a zombie whose environment can no longer be read: it is missed.
#[cfg(target_os = "linux")]
mod linux {
    use anyhow::Result;
    use std::collections::HashSet;
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    /// How long the processes a tool left running may go on after it exits before they are killed.
    pub const ORPHAN_GRACE: Duration = Duration::from_secs(5);
    /// How long killed processes may take to end before the harness stops waiting for them.
    const KILL_WAIT: Duration = Duration::from_secs(10);
    const POLL: Duration = Duration::from_millis(20);
    const TAG: &str = "VIRTUOSO_BENCH_RUN";

    /// What the processes a tool left behind used: CPU time (ms), the largest peak memory (KB).
    #[derive(Default)]
    pub struct Orphans {
        pub cpu_ms: f64,
        pub peak_kb: f64,
    }

    /// Makes the harness a subreaper (once), and the run a tagged session of its own; returns
    /// the tag's environment entry.
    pub fn prepare(cmd: &mut Command) -> String {
        static SUBREAPER: std::sync::Once = std::sync::Once::new();
        // SAFETY: a plain prctl on our own process.
        SUBREAPER.call_once(|| unsafe {
            libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        });
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let tag = format!("{}-{}", std::process::id(), RUNS.fetch_add(1, Ordering::Relaxed));
        cmd.env(TAG, &tag);
        // SAFETY: setsid is async-signal-safe and touches nothing of the parent's.
        unsafe {
            cmd.pre_exec(|| if libc::setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
        }
        format!("{TAG}={tag}")
    }

    /// Waits for the tool to exit, leaving it to be reaped.
    pub fn exited(pid: libc::pid_t) -> Result<()> {
        loop {
            // SAFETY: siginfo_t is plain data waitid fills; WNOWAIT leaves the child unreaped.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            if unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) } == 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            anyhow::ensure!(e.kind() == std::io::ErrorKind::Interrupted, "waiting for the tool: {e}");
        }
    }

    /// Reaps the processes the tool `run` left, killing those still running after `grace`.
    pub fn collect(run: libc::pid_t, tag: &str, grace: Duration) -> Orphans {
        let me = std::process::id() as libc::pid_t;
        let mut got = Orphans::default();
        let mut known = HashSet::new();
        let kill_at = Instant::now() + grace;
        let give_up = kill_at + KILL_WAIT;
        loop {
            let mut left = 0;
            for p in members(run, tag.as_bytes(), &mut known) {
                if p.ppid == me {
                    if let Some((cpu, kb)) = reap(p.pid) {
                        got.cpu_ms += cpu;
                        got.peak_kb = got.peak_kb.max(kb);
                        known.remove(&(p.pid, p.start));
                        continue;
                    }
                }
                left += 1;
                if Instant::now() >= kill_at {
                    // SAFETY: a signal to one of the run's own processes.
                    unsafe { libc::kill(p.pid, libc::SIGKILL) };
                }
            }
            if left == 0 || Instant::now() >= give_up {
                return got;
            }
            std::thread::sleep(POLL);
        }
    }

    struct Proc {
        pid: libc::pid_t,
        ppid: libc::pid_t,
        start: u64,
    }

    /// The run's processes, the tool's own left out. `known` remembers each one found (by ID and
    /// start time, against a reused ID): a zombie no longer shows its environment.
    fn members(run: libc::pid_t, tag: &[u8], known: &mut HashSet<(libc::pid_t, u64)>) -> Vec<Proc> {
        let mut out = Vec::new();
        for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<libc::pid_t>().ok()) else { continue };
            let Some((ppid, group, session, start)) = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().and_then(|s| stat(&s)) else { continue };
            if pid != run && (session == run || group == run || known.contains(&(pid, start)) || tagged(pid, tag)) {
                known.insert((pid, start));
                out.push(Proc { pid, ppid, start });
            }
        }
        out
    }

    /// From /proc/<pid>/stat: parent, process group, session and start time.
    fn stat(s: &str) -> Option<(libc::pid_t, libc::pid_t, libc::pid_t, u64)> {
        // After the parenthesised name: state, ppid, pgrp, session, ..., starttime (the 20th).
        let f: Vec<&str> = s.get(s.rfind(')')? + 1..)?.split_whitespace().collect();
        Some((f.get(1)?.parse().ok()?, f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?, f.get(19)?.parse().ok()?))
    }

    fn tagged(pid: libc::pid_t, tag: &[u8]) -> bool {
        std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|env| env.split(|b| *b == 0).any(|v| v == tag))
    }

    /// The CPU time (ms) and peak memory (KB) of one of our children, once it has ended.
    fn reap(pid: libc::pid_t) -> Option<(f64, f64)> {
        let mut status = 0;
        // SAFETY: rusage is plain data; pid is our child and the run's, which nothing else waits on.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        (unsafe { libc::wait4(pid, &mut status, libc::WNOHANG, &mut usage) } > 0).then(|| (super::unix::cpu_ms(&usage), usage.ru_maxrss as f64))
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, FILETIME, HANDLE};
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    /// A duplicate of a child's process handle, valid after the child exits.
    pub struct Handle(HANDLE);

    impl Handle {
        pub fn of(child: &std::process::Child) -> Option<Handle> {
            let mut dup: HANDLE = std::ptr::null_mut();
            // SAFETY: the source handle is the live child's; the duplicate is closed in Drop.
            let ok = unsafe {
                DuplicateHandle(GetCurrentProcess(), child.as_raw_handle() as HANDLE, GetCurrentProcess(), &mut dup, 0, 0, DUPLICATE_SAME_ACCESS)
            };
            (ok != 0).then_some(Handle(dup))
        }

        pub fn peak_working_set_kb(&self) -> Option<f64> {
            let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            // SAFETY: c is a properly sized PROCESS_MEMORY_COUNTERS.
            let ok = unsafe { GetProcessMemoryInfo(self.0, &mut c, c.cb) };
            (ok != 0).then(|| c.PeakWorkingSetSize as f64 / 1024.0)
        }

        pub fn cpu_ms(&self) -> Option<f64> {
            let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut a, mut b, mut k, mut u) = (z, z, z, z);
            // SAFETY: all four out-pointers are valid FILETIMEs.
            let ok = unsafe { GetProcessTimes(self.0, &mut a, &mut b, &mut k, &mut u) };
            let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 10_000.0;
            (ok != 0).then(|| t(k) + t(u))
        }
    }

    /// A job object holding the tool and everything it starts.
    pub struct Job(HANDLE);

    impl Job {
        pub fn around(child: &std::process::Child) -> Option<Job> {
            use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
            // SAFETY: plain Win32 calls; the job handle is closed in Drop.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                    CloseHandle(job);
                    return None;
                }
                Some(Job(job))
            }
        }

        /// The handle as a plain value, for the watchdog's thread.
        pub fn raw(&self) -> usize {
            self.0 as usize
        }

        pub fn cpu_ms(&self) -> Option<f64> {
            self.accounting().map(|info| (info.TotalUserTime + info.TotalKernelTime) as f64 / 10_000.0)
        }

        pub fn processes(&self) -> Option<u32> {
            self.accounting().map(|info| info.TotalProcesses)
        }

        fn accounting(&self) -> Option<windows_sys::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION> {
            use windows_sys::Win32::System::JobObjects::{JobObjectBasicAccountingInformation, QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION};
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: info is the struct this information class fills.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then_some(info)
        }
    }

    /// Stops the tool and everything it started: its job, or its process tree without one.
    pub fn stop(job: Option<usize>, pid: u32) {
        match job {
            // SAFETY: the job's handle stays open until the watchdog has finished.
            Some(job) => unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(job as HANDLE, 1);
            },
            None => {
                let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).output();
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: we own the job handle.
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: we own this duplicated handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}
