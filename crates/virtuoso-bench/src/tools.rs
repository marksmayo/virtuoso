//! The tools the benchmark runs, and how each runs on this kind of host.

use crate::measure::Runner;
use crate::orchestrate::{newest_build, Tool, DEVICE};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the benchmark runs.
#[derive(Debug, Clone)]
pub enum Host {
    /// Windows, with the POSIX-only tools in this WSL distribution.
    WindowsWsl { distro: String },
    /// Linux or macOS: every tool natively.
    Unix,
}

impl Host {
    /// This machine: Windows with VIRTUOSO_BENCH_WSL's distribution (default VirtuosoUbuntu),
    /// else Unix.
    pub fn here() -> Host {
        if cfg!(windows) {
            Host::WindowsWsl { distro: env_or("VIRTUOSO_BENCH_WSL", "VirtuosoUbuntu") }
        } else {
            Host::Unix
        }
    }
}

pub fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

pub fn tools(repo: &Path, host: &Host) -> Vec<Tool> {
    // On Windows, speedrun (POSIX only) and virtuoso's Linux build run in WSL, so they compare
    // like for like; on Linux and macOS everything runs natively.
    let (wsl, distro) = match host {
        Host::WindowsWsl { distro } => (Runner::Wsl { distro: distro.clone() }, distro.clone()),
        Host::Unix => (Runner::Native, String::new()),
    };
    let unix = matches!(host, Host::Unix);
    let flows = repo.join("bench").join("flows");
    let flows_s = flows.display().to_string();
    let to_wsl = |p: &Path| {
        if unix {
            return p.display().to_string();
        }
        let s = p.display().to_string().replace('\\', "/");
        match s.split_once(":/") {
            Some((drive, rest)) => format!("/mnt/{}/{rest}", drive.to_lowercase()),
            None => s,
        }
    };
    let unc = |linux: &str| if unix { linux.to_string() } else { format!(r"\\wsl.localhost\{distro}{}", linux.replace('/', r"\")) };
    // The newest build, whichever target dir it went to: a stale one measures old code.
    let virtuoso = env_or(
        "VIRTUOSO_WINDOWS_BIN",
        &newest_build(repo, "virtuoso.exe").unwrap_or_else(|| repo.join("target").join("release").join("virtuoso.exe")).display().to_string(),
    );
    let exe = if unix { "" } else { ".exe" };
    let mr = env_or("MAESTRO_RUNNER_BIN", &repo.join("bench").join("tools").join(format!("maestro-runner{exe}")).display().to_string());
    let mr_home = env_or("MAESTRO_RUNNER_HOME", &repo.parent().unwrap().join("maestro-runner").display().to_string());
    let mr_out = std::env::temp_dir().join("virtuoso-bench-maestro-runner").display().to_string();
    let speedrun = env_or("SPEEDRUN_BIN", "/opt/bench/speedrun/speedrun-v0.2.11-linux-x86_64/speedrun");
    // On a Unix host, the newest build in this checkout, else VIRTUOSO_LINUX_BIN.
    let local = || newest_build(repo, "virtuoso").unwrap_or_else(|| repo.join("target").join("release").join("virtuoso")).display().to_string();
    let virtuoso_linux = env_or("VIRTUOSO_LINUX_BIN", &if unix { local() } else { "/opt/bench/virtuoso".into() });
    let app_flows = app_flows(repo);
    let tam_flows = repo.join("bench").join("tam-flows");
    // The app suites' flows folders by scenario prefix, as a native tool and as a WSL one sees them.
    let suites = |dirs: &[(&str, &Path)], wsl: bool| -> BTreeMap<String, String> {
        dirs.iter().map(|(k, d)| (k.to_string(), if wsl { to_wsl(d) } else { d.display().to_string() })).collect()
    };
    let maestro_suites = [("app", app_flows.as_path()), ("tam", tam_flows.as_path())];
    // Maestro itself, launched as its maestro.bat does, but straight into the JVM so the
    // process measured is the one doing the work.
    let maestro_home = PathBuf::from(env_or("MAESTRO_HOME", &repo.join("bench").join("tools").join("maestro-2.11.0").join("maestro").display().to_string()));
    let java = real_java(unix);
    let maestro = |args: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = vec![java.clone(), "--enable-native-access=ALL-UNNAMED".into(), "-classpath".into()];
        v.push(maestro_home.join("lib").join("*").display().to_string());
        v.push("maestro.cli.AppKt".into());
        v.extend(args.iter().map(|a| a.to_string()));
        v
    };
    let appium_dir = repo.join("bench").join("appium");
    let appium_home = PathBuf::from(env_or("APPIUM_HOME", &dirs_home().join(".appium").display().to_string()));
    let appium_run = appium_dir.join("run.mjs").display().to_string();
    // Tester Army's e2e and Software Mansion's Argent: tests of their own, one folder per suite.
    let e2e_dir = repo.join("bench").join("e2e");
    let e2e_run = e2e_dir.join("run.mjs").display().to_string();
    let argent_dir = repo.join("bench").join("argent");
    let argent_cli = argent_dir.join("node_modules").join("@swmansion").join("argent").join("dist").join("cli.js").display().to_string();
    let own_suites = |dir: &Path| suites(&[("app", dir.join("app").as_path()), ("tam", dir.join("tam").as_path())], false);
    let mut all = vec![
        Tool {
            name: "virtuoso-windows".into(),
            runner: Runner::Native,
            test_argv: vec![virtuoso.clone(), "test".into(), "--no-ansi".into(), "--device".into(), DEVICE.into()],
            version_argv: vec![virtuoso.clone(), "--version".into()],
            env: vec![],
            flows_dir: flows_s.clone(),
            suite_dirs: suites(&maestro_suites, false),
            flow_ext: "yaml".into(),
            skips: vec![],
            install_files: vec![virtuoso],
            agent_processes: vec!["virtuoso-agent".into()],
        },
        Tool {
            name: "maestro-runner".into(),
            runner: Runner::Native,
            test_argv: vec![mr.clone(), "--device".into(), DEVICE.into(), "test".into(), "--output".into(), mr_out],
            version_argv: vec![mr.clone(), "--version".into()],
            env: vec![
                ("MAESTRO_RUNNER_HOME".into(), mr_home.clone()),
                ("MAESTRO_RUNNER_NO_TELEMETRY".into(), "1".into()),
                ("MAESTRO_RUNNER_NO_UPDATE_CHECK".into(), "1".into()),
            ],
            flows_dir: flows_s.clone(),
            suite_dirs: suites(&maestro_suites, false),
            flow_ext: "yaml".into(),
            skips: vec![],
            install_files: vec![mr, Path::new(&mr_home).join("drivers").join("android").display().to_string()],
            agent_processes: vec!["dev.devicelab.driver.android".into()],
        },
        Tool {
            name: "speedrun".into(),
            runner: wsl.clone(),
            test_argv: vec![speedrun.clone(), "test".into(), "--no-ansi".into(), "--device".into(), DEVICE.into()],
            version_argv: vec![speedrun.clone(), "--version".into()],
            env: vec![("SPEEDRUN_NO_UPDATE_CHECK".into(), "1".into()), ("CI".into(), "1".into())],
            flows_dir: to_wsl(&flows),
            suite_dirs: suites(&maestro_suites, true),
            flow_ext: "yaml".into(),
            skips: vec![],
            install_files: vec![unc(&speedrun)],
            agent_processes: vec!["speedrun-agent".into()],
        },
        Tool {
            name: "virtuoso".into(),
            runner: wsl,
            test_argv: vec![virtuoso_linux.clone(), "test".into(), "--no-ansi".into(), "--device".into(), DEVICE.into()],
            version_argv: vec![virtuoso_linux.clone(), "--version".into()],
            env: vec![],
            flows_dir: to_wsl(&flows),
            suite_dirs: suites(&maestro_suites, true),
            flow_ext: "yaml".into(),
            skips: vec![],
            install_files: vec![unc(&virtuoso_linux)],
            agent_processes: vec!["virtuoso-agent".into()],
        },
        Tool {
            name: "maestro".into(),
            runner: Runner::Native,
            test_argv: maestro(&["test", "--device", DEVICE]),
            version_argv: maestro(&["--version"]),
            env: vec![
                ("MAESTRO_CLI_NO_ANALYTICS".into(), "1".into()),
                ("MAESTRO_CLI_ANALYSIS_NOTIFICATION_DISABLED".into(), "true".into()),
                ("MAESTRO_DISABLE_UPDATE_CHECK".into(), "true".into()),
            ],
            flows_dir: flows_s.clone(),
            suite_dirs: suites(&maestro_suites, false),
            flow_ext: "yaml".into(),
            skips: vec![],
            // Its lib/ folder; the Java 17 runtime it needs is not counted.
            install_files: vec![maestro_home.display().to_string()],
            agent_processes: vec!["dev.mobile.maestro".into()],
        },
        Tool {
            name: "appium".into(),
            runner: Runner::Native,
            test_argv: vec!["node".into(), appium_run.clone()],
            version_argv: vec!["node".into(), appium_run, "--version".into()],
            env: vec![("APPIUM_HOME".into(), appium_home.display().to_string()), ("VIRTUOSO_BENCH_DEVICE".into(), DEVICE.into())],
            flows_dir: flows_s,
            // Its tests are code, not flow files: Tester Army's mobile benchmark has an Appium
            // version (tam/scenarios.mjs, run from tam/); the Speedrun Bench app's flows and the
            // invalid flow do not.
            suite_dirs: suites(&[("tam", appium_dir.join("tam").as_path())], false),
            flow_ext: "yaml".into(),
            skips: vec!["invalid".into()],
            // The server and its UiAutomator2 driver; the Node.js runtime is not counted.
            install_files: vec![appium_dir.join("node_modules").display().to_string(), appium_home.display().to_string()],
            agent_processes: vec!["io.appium.uiautomator2.server".into(), "io.appium.settings".into()],
        },
        Tool {
            name: "e2e".into(),
            runner: Runner::Native,
            test_argv: vec!["node".into(), e2e_run.clone()],
            version_argv: vec!["node".into(), e2e_run, "--version".into()],
            env: vec![("VIRTUOSO_BENCH_DEVICE".into(), DEVICE.into())],
            flows_dir: e2e_dir.join("settings").display().to_string(),
            suite_dirs: own_suites(&e2e_dir),
            flow_ext: "e2e.ts".into(),
            // A test is code: there is no flow file with an unknown command to reject.
            skips: vec!["invalid".into()],
            // The SDK, runner and mobile engine (agent-device); the Node.js runtime is not counted.
            install_files: vec![e2e_dir.join("node_modules").display().to_string()],
            agent_processes: vec![],
        },
        Tool {
            name: "argent".into(),
            runner: Runner::Native,
            test_argv: vec!["node".into(), argent_cli.clone(), "e2e".into(), "run".into(), "--device".into(), DEVICE.into(), "--platform".into(), "android".into()],
            version_argv: vec!["node".into(), argent_cli, "--version".into()],
            env: vec![("ARGENT_TELEMETRY".into(), "0".into())],
            flows_dir: argent_dir.join("settings").display().to_string(),
            suite_dirs: own_suites(&argent_dir),
            flow_ext: "yaml".into(),
            skips: vec![],
            // The CLI, tool-server and its bundled binaries; the Node.js runtime is not counted.
            install_files: vec![argent_dir.join("node_modules").display().to_string()],
            // Its devtools app, run as an instrumentation that holds the UiAutomation connection.
            agent_processes: vec!["com.argent.androiddevtools".into()],
        },
    ];
    // The native Windows build is a Windows host's supplement.
    if unix {
        all.retain(|t| t.name != "virtuoso-windows");
    }
    all
}

/// Speedrun's own benchmark app (github.com/hubastard/speedrun, bench/app): its flows, run from
/// that checkout next to this repository, with its prebuilt APK installed.
pub fn app_flows(repo: &Path) -> PathBuf {
    PathBuf::from(env_or(
        "VIRTUOSO_BENCH_APP_FLOWS",
        &repo.parent().unwrap().join("speedrun").join("bench").join("app").join("flows").display().to_string(),
    ))
}

/// The JVM itself, not the PATH's redirector to it (see orchestrate::java_home).
fn real_java(unix: bool) -> String {
    std::process::Command::new("java")
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .ok()
        .and_then(|o| crate::orchestrate::java_home(&String::from_utf8_lossy(&o.stderr)))
        .map(|h| Path::new(&h).join("bin").join(if unix { "java" } else { "java.exe" }).display().to_string())
        .unwrap_or_else(|| "java".into())
}

fn dirs_home() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default())
}


/// The platform the tools drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    /// A booted iOS simulator on a Mac.
    Ios,
}

/// The tools for a platform: on iOS, each told so its own way (a flag, or for the tools whose
/// tests are code, VIRTUOSO_BENCH_PLATFORM). Agent memory is sampled through adb, so on iOS no
/// tool has device processes to sample; the native Windows build has no part (iOS is a Mac's).
pub fn tools_for(repo: &Path, host: &Host, platform: Platform) -> Vec<Tool> {
    let mut all = tools(repo, host);
    if platform == Platform::Ios {
        all.retain(|t| t.name != "virtuoso-windows");
        for t in &mut all {
            t.agent_processes.clear();
            // The Settings scenarios as iOS's Settings app has them (bench/flows-ios).
            if t.flows_dir.ends_with("flows") {
                t.flows_dir.push_str("-ios");
            }
            let ios = || ["--platform".to_string(), "ios".to_string()];
            match t.name.as_str() {
                // Global flags, before the command.
                "virtuoso" | "maestro-runner" => t.test_argv.splice(1..1, ios()).for_each(drop),
                "speedrun" => {
                    let at = t.test_argv.iter().position(|a| a == "test").map_or(1, |i| i + 1);
                    t.test_argv.splice(at..at, ["-p".to_string(), "ios".to_string()]).for_each(drop);
                }
                "maestro" => {
                    let at = t.test_argv.iter().position(|a| a == "test").unwrap_or(t.test_argv.len());
                    t.test_argv.splice(at..at, ios()).for_each(drop);
                }
                "argent" => {
                    if let Some(i) = t.test_argv.iter().position(|a| a == "android") {
                        t.test_argv[i] = "ios".into();
                    }
                }
                _ => t.env.push(("VIRTUOSO_BENCH_PLATFORM".into(), "ios".into())),
            }
            // e2e's Settings tests are written against Android's Settings app: no value for
            // those scenarios on iOS until they have iOS versions, rather than failures.
            if t.name == "e2e" {
                for s in crate::metrics::DEVICE_SCENARIOS.iter().copied().chain(["trivial"]) {
                    t.skips.push(s.to_string());
                }
            }
        }
    }
    all
}
