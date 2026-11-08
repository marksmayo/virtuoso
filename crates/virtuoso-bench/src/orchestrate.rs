//! Runs the benchmark: every scenario, every tool, several times, with the device reset to the
//! same state before each run.
//!
//! Tools take turns within each iteration (a, b, c, a, b, c...) so a slow drift of the
//! emulator over time spreads across all of them instead of landing on one.

use crate::measure::{run, Measurement, Runner};
use crate::metrics::{keeps_best, RunRecord, StaticInfo};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub runner: Runner,
    /// The program and the arguments before the flow file.
    pub test_argv: Vec<String>,
    pub version_argv: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The folder flows are run from (as the tool sees it: a Linux path for WSL tools).
    pub flows_dir: String,
    /// Each app suite's flows folder (as the tool sees it), by the suite's scenario prefix:
    /// `app` for the Speedrun Bench app's `app/<flow>`, `tam` for Tester Army's mobile
    /// benchmark's. A suite the tool has no tests for is not there.
    #[serde(default)]
    pub suite_dirs: BTreeMap<String, String>,
    /// The extension of its test files: `yaml` for flows, `e2e.ts` for e2e's tests.
    pub flow_ext: String,
    /// Scenarios the tool has no test for (Appium has no flow file to reject).
    #[serde(default)]
    pub skips: Vec<String>,
    /// Files whose size is the tool's install footprint (as the harness sees them).
    pub install_files: Vec<String>,
    /// Names of the tool's processes on the device, for the agent memory sample.
    pub agent_processes: Vec<String>,
}

/// Stands for the device's serial in a tool's test command and environment: each tool names
/// the device its own way, and one that is not told picks a device itself (or gives up when
/// two are connected).
pub const DEVICE: &str = "{device}";

impl Tool {
    /// The tool with its test command and environment naming `serial` as the device.
    pub fn on_device(&self, serial: &str) -> Tool {
        let fill = |s: &String| s.replace(DEVICE, serial);
        Tool {
            test_argv: self.test_argv.iter().map(fill).collect(),
            env: self.env.iter().map(|(k, v)| (k.clone(), fill(v))).collect(),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Results {
    pub meta: BTreeMap<String, String>,
    pub tools: Vec<String>,
    pub statics: BTreeMap<String, StaticInfo>,
    pub runs: Vec<RunRecord>,
}

/// Puts the device back where every run starts: no test agents or forwards, Settings and the
/// bench app with no saved state, animations at their defaults, the home screen.
pub fn reset_device(serial: &str, tools: &[Tool]) {
    let _ = Command::new("adb").args(["-s", serial, "forward", "--remove-all"]).output();
    let _ = Command::new("adb").args(["-s", serial, "shell", &reset_script(tools)]).output();
}

/// The shell script of the reset: every tool's agent stopped, a shell process by `kill`, an app
/// (a name with a dot: maestro-runner's, Maestro's and Appium's drivers) by `am force-stop`.
/// The apps every iOS run starts afresh: Settings (it reopens where it was left, list scrolled),
/// speedrun's benchmark app and Tester Army's.
const IOS_APPS: [&str; 3] = ["com.apple.Preferences", "dev.speedrun.bench", "dev.e2e.benchmark"];

/// What puts an iOS simulator back where every run starts, as `reset_script` does an Android
/// device: the benchmark apps stopped with no saved state, light mode. (sh, on the Mac.)
pub fn ios_reset_script(udid: &str) -> String {
    let mut s = String::new();
    for app in IOS_APPS {
        s += &format!("xcrun simctl terminate {udid} {app} 2>/dev/null; ");
        s += &format!(
            "d=$(xcrun simctl get_app_container {udid} {app} data 2>/dev/null) && [ -n \"$d\" ] && rm -rf \"$d/Documents\"/* \"$d/Library\"/* \"$d/tmp\"/* 2>/dev/null; "
        );
    }
    s + &format!("xcrun simctl ui {udid} appearance light")
}

pub fn reset_script(tools: &[Tool]) -> String {
    let mut s = String::new();
    for name in tools.iter().flat_map(|t| &t.agent_processes) {
        if name.contains('.') {
            s += &format!("am force-stop {name}; am force-stop {name}.test; ");
        } else {
            s += &format!("p=$(pidof {name}) && kill $p; ");
        }
    }
    s + "pm clear com.android.settings >/dev/null; pm clear dev.speedrun.bench >/dev/null 2>&1; pm clear dev.e2e.benchmark >/dev/null 2>&1;         for k in window_animation_scale transition_animation_scale animator_duration_scale; do settings put global $k 1; done;         settings put system accelerometer_rotation 0; settings put system user_rotation 0; settings put secure stylus_handwriting_enabled 0; cmd uimode night no >/dev/null;         input keyevent KEYCODE_HOME; sleep 1"
}

/// Samples the resident memory of the tool's device processes every half second while a run
/// goes on, and keeps the highest total.
pub struct AgentSampler {
    stop: Arc<AtomicBool>,
    peak: Arc<Mutex<Option<f64>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// The total RSS (KB) of the named processes in `ps -A -o RSS,NAME` output.
pub fn rss_of(ps: &str, names: &[String]) -> Option<f64> {
    let mut total = 0.0;
    let mut found = false;
    for line in ps.lines() {
        let mut f = line.split_whitespace();
        let (Some(rss), Some(name)) = (f.next(), f.next()) else { continue };
        if names.iter().any(|n| n == name) {
            if let Ok(kb) = rss.parse::<f64>() {
                total += kb;
                found = true;
            }
        }
    }
    found.then_some(total)
}

impl AgentSampler {
    pub fn start(serial: &str, names: Vec<String>) -> AgentSampler {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(Mutex::new(None));
        let (s, p, serial) = (stop.clone(), peak.clone(), serial.to_string());
        let handle = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                if let Ok(out) = Command::new("adb").args(["-s", &serial, "shell", "ps -A -o RSS,NAME"]).output() {
                    if let Some(kb) = rss_of(&String::from_utf8_lossy(&out.stdout), &names) {
                        let mut g = p.lock().unwrap();
                        *g = Some(g.map_or(kb, |old: f64| old.max(kb)));
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        AgentSampler { stop, peak, handle: Some(handle) }
    }

    pub fn finish(mut self) -> Option<f64> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        *self.peak.lock().unwrap()
    }
}

pub fn install_bytes(files: &[String]) -> Option<f64> {
    let mut total = 0u64;
    for f in files {
        total += dir_size(std::path::Path::new(f))?;
    }
    Some(total as f64)
}

fn dir_size(p: &std::path::Path) -> Option<u64> {
    let md = std::fs::metadata(p).ok()?;
    if md.is_file() {
        return Some(md.len());
    }
    let mut t = 0;
    for e in std::fs::read_dir(p).ok()?.flatten() {
        t += dir_size(&e.path()).unwrap_or(0);
    }
    Some(t)
}

/// Whether a run counts as passed: device scenarios must exit 0; the invalid flow must be
/// rejected (non-zero) without the run hanging.
pub fn passed(scenario: &str, m: &Measurement) -> bool {
    match scenario {
        "invalid" => m.exit != 0,
        _ => m.exit == 0,
    }
}

/// `old` with `new` added: virtuoso's runs of every (tool, scenario) in `new` replace its old ones; the
/// competition's old runs stay unless they are of another version.
pub fn merge_runs(old: Vec<RunRecord>, new: Vec<RunRecord>) -> Vec<RunRecord> {
    // The new version of each tool and scenario rerun.
    let rerun: std::collections::HashMap<(String, String), Option<String>> =
        new.iter().map(|r| ((r.tool.clone(), r.scenario.clone()), r.version.clone())).collect();
    old.into_iter()
        .filter(|r| match rerun.get(&(r.tool.clone(), r.scenario.clone())) {
            None => true,
            // The competition keeps earlier batches of the same version: it is scored on its best.
            Some(version) => keeps_best(&r.tool) && (r.version.is_none() || version.is_none() || &r.version == version),
        })
        .chain(new)
        .collect()
}

/// A run the harness could not even start counts as a failed run with the reason, so one
/// broken invocation does not end a benchmark that has been going for hours.
pub fn measured(r: Result<Measurement>) -> Measurement {
    r.unwrap_or_else(|e| Measurement { wall_ms: 0.0, peak_rss_kb: None, cpu_ms: None, exit: -1, output: format!("harness: {e:#}"), processes: None })
}

pub fn version_text(tool: &Tool) -> String {
    match run(&tool.runner, &tool.version_argv, &tool.env, &tool.flows_dir) {
        Ok(m) => m.output.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string(),
        Err(e) => format!("unknown ({e})"),
    }
}

pub struct Plan<'a> {
    pub serial: &'a str,
    /// Android (reset over adb) or an iOS simulator (reset with simctl).
    pub platform: crate::tools::Platform,
    pub tools: &'a [Tool],
    /// Every tool the benchmark knows: the reset stops their agents too, as one that a run of
    /// other tools left behind (Argent's holds the UiAutomation connection) blocks these.
    pub known: &'a [Tool],
    pub scenarios: &'a [String],
    pub iterations: usize,
    pub version_runs: usize,
    pub progress: &'a dyn Fn(&str),
    /// Called with every run so far after each run, to save progress.
    pub checkpoint: &'a dyn Fn(&[RunRecord]),
}

impl Plan<'_> {
    pub fn reset_script(&self) -> String {
        reset_script(self.known)
    }

    /// The device back where every run starts, as its platform does it.
    pub fn reset(&self) {
        match self.platform {
            crate::tools::Platform::Android => reset_device(self.serial, self.known),
            crate::tools::Platform::Ios => {
                let _ = Command::new("sh").args(["-c", &ios_reset_script(self.serial)]).output();
            }
        }
    }
}

pub fn execute(plan: &Plan) -> Result<Vec<RunRecord>> {
    let mut runs = Vec::new();
    let tools: Vec<Tool> = plan.tools.iter().map(|t| t.on_device(plan.serial)).collect();
    for tool in &tools {
        for i in 0..plan.version_runs {
            let m = measured(run(&tool.runner, &tool.version_argv, &tool.env, &tool.flows_dir));
            runs.push(record(tool, "version", i, &m, None, m.exit == 0));
        }
    }
    for scenario in plan.scenarios {
        // One unrecorded warm-up per tool: the first run after a fresh agent install or a cold
        // file cache is not what a user sees on the second run of the day.
        for tool in &tools {
            let Some((argv, dir)) = invocation(tool, scenario) else { continue };
            plan.reset();
            let _ = run(&tool.runner, &argv, &tool.env, &dir);
        }
        for i in 0..plan.iterations {
            for tool in &tools {
                let Some((argv, dir)) = invocation(tool, scenario) else { continue };
                plan.reset();
                let sampler = (scenario != "invalid").then(|| AgentSampler::start(plan.serial, tool.agent_processes.clone()));
                let m = measured(run(&tool.runner, &argv, &tool.env, &dir));
                let agent = sampler.and_then(AgentSampler::finish);
                let ok = passed(scenario, &m);
                (plan.progress)(&format!(
                    "{scenario:<11} {:<16} #{i} {:>8.0} ms  {}{}",
                    tool.name,
                    m.wall_ms,
                    if ok { "pass".to_string() } else { format!("FAIL (exit {})", m.exit) },
                    // A virtuoso run starts no process but itself: more means a spawn crept in.
                    match m.processes {
                        Some(n) if n > 1 && tool.name.starts_with("virtuoso") => format!("  ({n} processes)"),
                        _ => String::new(),
                    }
                ));
                runs.push(record(tool, scenario, i, &m, agent, ok));
                (plan.checkpoint)(&runs);
            }
        }
    }
    Ok(runs)
}

/// The command line and the folder that run `scenario` with `tool`: a Settings scenario's flow
/// from the flows folder, an app suite's `<suite>/<flow>` from that suite's. None when the tool
/// has no test for it.
pub fn invocation(tool: &Tool, scenario: &str) -> Option<(Vec<String>, String)> {
    if tool.skips.iter().any(|s| s == scenario) {
        return None;
    }
    let (file, dir) = match scenario.split_once('/') {
        Some((suite, flow)) => (flow, tool.suite_dirs.get(suite)?.clone()),
        None => (scenario, tool.flows_dir.clone()),
    };
    let mut argv = tool.test_argv.clone();
    argv.push(format!("{file}.{}", tool.flow_ext));
    Some((argv, dir))
}

fn record(tool: &Tool, scenario: &str, i: usize, m: &Measurement, agent: Option<f64>, ok: bool) -> RunRecord {
    let error = (!ok).then(|| {
        let lines: Vec<&str> = m.output.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(6)..].join("\n")
    });
    RunRecord {
        tool: tool.name.clone(),
        scenario: scenario.into(),
        iteration: i,
        wall_ms: m.wall_ms,
        peak_rss_kb: m.peak_rss_kb,
        cpu_ms: m.cpu_ms,
        processes: m.processes,
        passed: ok,
        agent_rss_kb: agent,
        error,
        ..Default::default()
    }
}

/// The most recently built `exe` under `repo/target`: `target/release` or any
/// `target/<dir>/release` (a build with its own CARGO_TARGET_DIR).
pub fn newest_build(repo: &std::path::Path, exe: &str) -> Option<std::path::PathBuf> {
    let target = repo.join("target");
    let mut candidates = vec![target.join("release").join(exe)];
    if let Ok(dirs) = std::fs::read_dir(&target) {
        candidates.extend(dirs.flatten().map(|d| d.path().join("release").join(exe)));
    }
    candidates
        .into_iter()
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// Why the virtuoso builds being compared cannot share a device, if they cannot: each deletes
/// the other's agent and pushes its own on every run. `versions` are (tool, `--version` text).
pub fn agent_mismatch(versions: &[(String, String)]) -> Option<String> {
    let agents: Vec<(&str, &str)> = versions
        .iter()
        .filter_map(|(tool, v)| Some((tool.as_str(), v.split("(agent ").nth(1)?.trim_end_matches(')').trim())))
        .collect();
    let first = agents.first()?;
    agents.iter().any(|(_, a)| a != &first.1).then(|| {
        let list: Vec<String> = agents.iter().map(|(t, a)| format!("{t}: {a}")).collect();
        format!("the virtuoso builds embed different agents ({}); build them with the same VIRTUOSO_AGENT_JAR", list.join(", "))
    })
}

/// An app suite's flows, as `<suite>/<flow>` scenarios in name order: each `.yaml` in its flows
/// folder but the suite's `config.yaml` (subflows live in folders below it).
pub fn app_scenarios(dir: &std::path::Path, suite: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".yaml")?;
            (stem != "config" && e.path().is_file()).then(|| format!("{suite}/{stem}"))
        })
        .collect();
    out.sort();
    out
}

/// The tools of results merged into older ones: the old ones in their order, then any new.
/// The results of separate jobs (a CI run's, one per group of scenarios) as one: every job's
/// runs, the tools in the order they first appear, each tool's static figures, and the first
/// job's meta with any keys the others add.
pub fn combine_results(jobs: Vec<Results>) -> Results {
    let mut all = Results { meta: BTreeMap::new(), tools: vec![], statics: BTreeMap::new(), runs: vec![] };
    for job in jobs {
        for (k, v) in job.meta {
            all.meta.entry(k).or_insert(v);
        }
        all.tools = merge_tools(&all.tools, &job.tools);
        for (t, s) in job.statics {
            all.statics.entry(t).or_insert(s);
        }
        all.runs.extend(job.runs);
    }
    all
}

pub fn merge_tools(old: &[String], new: &[String]) -> Vec<String> {
    let mut out = old.to_vec();
    out.extend(new.iter().filter(|t| !old.contains(t)).cloned());
    out
}

/// `java.home` from `java -XshowSettings:properties -version`. The `java` on Windows' PATH is
/// often Oracle's javapath redirector, which starts the real one as a child: measured, it would
/// stand for Maestro's memory.
pub fn java_home(settings: &str) -> Option<String> {
    settings.lines().find_map(|l| l.trim().strip_prefix("java.home = ")).map(|h| h.trim().to_string())
}
