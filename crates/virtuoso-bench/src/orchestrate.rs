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
/// device: the benchmark apps stopped with no saved state, light mode (after a failed run, the
/// simulator is rebooted first: `ios_reboot_script`). (sh, on the Mac.)
pub fn ios_reset_script(udid: &str) -> String {
    // simctl found once: `xcrun simctl` looks up the SDK on every call.
    let mut s = String::from("S=$(xcrun -f simctl); ");
    // Each app stopped and cleared at once with the others, its data folder remembered between
    // resets (a reinstall moves it: one that is gone is looked up again).
    for app in IOS_APPS {
        let note = format!("${{TMPDIR:-/tmp}}/virtuoso-bench-container-{udid}-{app}");
        s += &format!(
            "( \"$S\" terminate {udid} {app} 2>/dev/null; \
             d=$(cat \"{note}\" 2>/dev/null); \
             [ -d \"$d\" ] || {{ d=$(\"$S\" get_app_container {udid} {app} data 2>/dev/null) && echo \"$d\" > \"{note}\"; }}; \
             [ -n \"$d\" ] && [ -d \"$d\" ] && rm -rf \"$d/Documents\"/* \"$d/Library\"/* \"$d/tmp\"/* 2>/dev/null ) & "
        );
    }
    s + &format!("wait; [ \"$(\"$S\" ui {udid} appearance)\" = light ] || \"$S\" ui {udid} appearance light")
}

/// The simulator shut down and booted again, ready once `bootstatus` says so: what a failed
/// run left (a system prompt over every app) gone. Restarting SpringBoard alone cleared the
/// prompt but left the simulator unable to rotate or to answer the photos prompt.
pub fn ios_reboot_script(udid: &str) -> String {
    format!("xcrun simctl shutdown {udid}; xcrun simctl boot {udid}; xcrun simctl bootstatus {udid} -b >/dev/null")
}

/// Whether the agent memory sampler can read anything for `tool`: it reads `adb shell ps`, so
/// on Android only, and only for a tool whose processes are named.
pub fn samples_agent_memory(platform: crate::tools::Platform, tool: &Tool) -> bool {
    platform == crate::tools::Platform::Android && !tool.agent_processes.is_empty()
}

/// Stops every tool's agent: the `xcodebuild test-without-building` hosts first, and waits for
/// them to exit (a host relaunches a stopped runner), then every XCTest runner on the simulator
/// (bundle ids ending `.xctrunner`). Left up, they slow whichever tool runs next. What is left
/// is listed, for the job's log.
pub fn ios_stop_agents_script(udid: &str) -> String {
    format!(
        "pkill -f 'xcodebuild.*test-without-building' 2>/dev/null; \
         for i in $(seq 1 50); do pgrep -f 'xcodebuild.*test-without-building' >/dev/null || break; sleep 0.2; done; \
         pkill -9 -f 'xcodebuild.*test-without-building' 2>/dev/null; \
         for b in $(xcrun simctl spawn {udid} launchctl list | grep -o 'UIKitApplication:[^[]*\\.xctrunner' | sed 's/^UIKitApplication://' | sort -u); do xcrun simctl terminate {udid} $b 2>/dev/null; done; \
         echo \"agents stopped; still running: $(pgrep -fl 'xcodebuild|xctrunner|WebDriverAgent' | tr '\\n' ';')\""
    )
}

/// One step of a scenario's runs: the tool at an index of the scenario's tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Every tool's agent stopped (iOS, before a tool's block).
    StopAgents,
    /// The tool's unrecorded warm-up run.
    Warmup(usize),
    /// The tool's recorded run of that iteration.
    Run(usize, usize),
}

/// The order of a scenario's runs. Android: the tools take turns each iteration, its reset
/// stopping every agent before every run. iOS: a test runner stays up between runs and an idle
/// one slows the others, so each tool runs as a block after every runner is stopped, its agent
/// started in its warm-up and kept for its runs.
pub fn schedule(platform: crate::tools::Platform, tools: &[&str], iterations: usize) -> Vec<Step> {
    let mut steps = Vec::new();
    match platform {
        crate::tools::Platform::Android => {
            steps.extend((0..tools.len()).map(Step::Warmup));
            for i in 0..iterations {
                steps.extend((0..tools.len()).map(|t| Step::Run(t, i)));
            }
        }
        crate::tools::Platform::Ios => {
            for t in 0..tools.len() {
                steps.push(Step::StopAgents);
                steps.push(Step::Warmup(t));
                steps.extend((0..iterations).map(|i| Step::Run(t, i)));
            }
        }
    }
    steps
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
    /// Where each run's whole output goes (`<scenario>/<tool>-<iteration>.txt`), if anywhere.
    pub outputs: Option<&'a std::path::Path>,
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

    /// iOS: the simulator rebooted, after a failed run.
    fn reboot(&self) {
        let _ = Command::new("sh").args(["-c", &ios_reboot_script(self.serial)]).output();
        (self.progress)("simulator rebooted after a failed run");
    }

    /// Keeps a run's whole output, when asked to.
    fn keep(&self, scenario: &str, name: &str, m: &Measurement) {
        if let Some(dir) = self.outputs {
            let dir = dir.join(scenario.replace('/', "_"));
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(format!("{name}.txt")), &m.output);
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
    let ios = plan.platform == crate::tools::Platform::Ios;
    // Whether the run before failed: on iOS the simulator is rebooted before the next run, what
    // the failure left (a system prompt) gone.
    let mut failed = false;
    // Whether the tool failed on a simulator rebooted for it: it fails on its own, and its later
    // runs of the scenario need no reboot.
    let mut fails_anyway = false;
    for scenario in plan.scenarios {
        let active: Vec<(&Tool, (Vec<String>, String))> = tools.iter().filter_map(|t| invocation(t, scenario).map(|inv| (t, inv))).collect();
        let names: Vec<&str> = active.iter().map(|(t, _)| t.name.as_str()).collect();
        for step in schedule(plan.platform, &names, plan.iterations) {
            match step {
                Step::StopAgents => {
                    if let Ok(out) = Command::new("sh").args(["-c", &ios_stop_agents_script(plan.serial)]).output() {
                        (plan.progress)(String::from_utf8_lossy(&out.stdout).trim());
                    }
                }
                // One unrecorded warm-up per tool: the first run after a fresh agent install or
                // a cold file cache is not what a user sees on the second run of the day.
                Step::Warmup(t) => {
                    let (tool, (argv, dir)) = &active[t];
                    fails_anyway = false;
                    if failed && ios {
                        plan.reboot();
                    }
                    plan.reset();
                    let m = measured(run(&tool.runner, argv, &tool.env, dir));
                    failed = !passed(scenario, &m);
                    plan.keep(scenario, &format!("{}-warmup", tool.name), &m);
                }
                Step::Run(t, i) => {
                    let (tool, (argv, dir)) = &active[t];
                    let (tool, argv, dir) = (*tool, argv.clone(), dir.clone());
                    // iOS: the reboot takes the tool's test runner with it; the tool starts it
                    // again on an unrecorded run first.
                    let rebooted = failed && ios && !fails_anyway;
                    if rebooted {
                        plan.reboot();
                        plan.reset();
                        let m = measured(run(&tool.runner, &argv, &tool.env, &dir));
                        plan.keep(scenario, &format!("{}-{i}-rewarm", tool.name), &m);
                        fails_anyway = !passed(scenario, &m);
                    }
                    plan.reset();
                    let sampler = (scenario != "invalid" && samples_agent_memory(plan.platform, tool)).then(|| AgentSampler::start(plan.serial, tool.agent_processes.clone()));
                    let m = measured(run(&tool.runner, &argv, &tool.env, &dir));
                    let agent = sampler.and_then(AgentSampler::finish);
                    let ok = passed(scenario, &m);
                    failed = !ok;
                    fails_anyway = if ok { false } else { fails_anyway || rebooted };
                    plan.keep(scenario, &format!("{}-{i}", tool.name), &m);
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
