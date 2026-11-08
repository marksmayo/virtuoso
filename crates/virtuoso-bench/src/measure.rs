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
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
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
    #[cfg(unix)]
    let waited = unix::wait(&child);
    #[cfg(not(unix))]
    let waited = child.wait().map(|s| (s.code().unwrap_or(-1), None, None)).map_err(anyhow::Error::from);
    let wall = start.elapsed().as_secs_f64() * 1000.0;
    let timed_out = watchdog.finish();
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
/// resident memory, and the CPU time of it and every process it waited for.
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
        let ms = |t: libc::timeval| t.tv_sec as f64 * 1000.0 + t.tv_usec as f64 / 1000.0;
        // Linux reports the peak in KB, macOS in bytes.
        let peak = if cfg!(target_os = "macos") { usage.ru_maxrss as f64 / 1024.0 } else { usage.ru_maxrss as f64 };
        Ok((exit, Some(peak), Some(ms(usage.ru_utime) + ms(usage.ru_stime))))
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
