//! `virtuoso-bench run` measures the tools and writes results JSON; `virtuoso-bench report` turns
//! results (and bench/features.json) into the HTML medal report.
//!
//! The tool set is this machine's: virtuoso (Linux build) and speedrun (POSIX only) in the WSL
//! distribution VIRTUOSO_BENCH_WSL (default VirtuosoUbuntu), so they compare like for like;
//! maestro-runner natively on Windows, where its port forwarding works; and virtuoso-windows, the
//! native Windows build, as a supplement.

use anyhow::{bail, Context, Result};
use virtuoso_bench::measure::Runner;
use virtuoso_bench::metrics::{StaticInfo, DEVICE_SCENARIOS};
use virtuoso_bench::orchestrate::{agent_mismatch, execute, install_bytes, newest_build, version_text, Plan, Results, Tool};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

fn tools(repo: &Path) -> Vec<Tool> {
    let distro = env_or("VIRTUOSO_BENCH_WSL", "VirtuosoUbuntu");
    let wsl = Runner::Wsl { distro: distro.clone() };
    let flows = repo.join("bench").join("flows");
    let flows_s = flows.display().to_string();
    let to_wsl = |p: &Path| {
        let s = p.display().to_string().replace('\\', "/");
        match s.split_once(":/") {
            Some((drive, rest)) => format!("/mnt/{}/{rest}", drive.to_lowercase()),
            None => s,
        }
    };
    let unc = |linux: &str| format!(r"\\wsl.localhost\{distro}{}", linux.replace('/', r"\"));
    // The newest build, whichever target dir it went to: a stale one measures old code.
    let virtuoso = env_or(
        "VIRTUOSO_WINDOWS_BIN",
        &newest_build(repo, "virtuoso.exe").unwrap_or_else(|| repo.join("target").join("release").join("virtuoso.exe")).display().to_string(),
    );
    let mr = repo.join("bench").join("tools").join("maestro-runner.exe").display().to_string();
    let mr_home = env_or("MAESTRO_RUNNER_HOME", &repo.parent().unwrap().join("maestro-runner").display().to_string());
    let mr_out = std::env::temp_dir().join("virtuoso-bench-maestro-runner").display().to_string();
    let speedrun = env_or("SPEEDRUN_BIN", "/opt/bench/speedrun/speedrun-v0.2.8-linux-x86_64/speedrun");
    let virtuoso_linux = env_or("VIRTUOSO_LINUX_BIN", "/opt/bench/virtuoso");
    vec![
        Tool {
            name: "virtuoso-windows".into(),
            runner: Runner::Native,
            test_argv: vec![virtuoso.clone(), "test".into(), "--no-ansi".into()],
            version_argv: vec![virtuoso.clone(), "--version".into()],
            env: vec![],
            flows_dir: flows_s.clone(),
            install_files: vec![virtuoso],
            agent_processes: vec!["virtuoso-agent".into()],
        },
        Tool {
            name: "maestro-runner".into(),
            runner: Runner::Native,
            test_argv: vec![mr.clone(), "test".into(), "--output".into(), mr_out],
            version_argv: vec![mr.clone(), "--version".into()],
            env: vec![
                ("MAESTRO_RUNNER_HOME".into(), mr_home.clone()),
                ("MAESTRO_RUNNER_NO_TELEMETRY".into(), "1".into()),
                ("MAESTRO_RUNNER_NO_UPDATE_CHECK".into(), "1".into()),
            ],
            flows_dir: flows_s.clone(),
            install_files: vec![mr, Path::new(&mr_home).join("drivers").join("android").display().to_string()],
            agent_processes: vec!["dev.devicelab.driver.android".into()],
        },
        Tool {
            name: "speedrun".into(),
            runner: wsl.clone(),
            test_argv: vec![speedrun.clone(), "test".into(), "--no-ansi".into()],
            version_argv: vec![speedrun.clone(), "--version".into()],
            env: vec![("SPEEDRUN_NO_UPDATE_CHECK".into(), "1".into()), ("CI".into(), "1".into())],
            flows_dir: to_wsl(&flows),
            install_files: vec![unc(&speedrun)],
            agent_processes: vec!["speedrun-agent".into()],
        },
        Tool {
            name: "virtuoso".into(),
            runner: wsl,
            test_argv: vec![virtuoso_linux.clone(), "test".into(), "--no-ansi".into()],
            version_argv: vec![virtuoso_linux.clone(), "--version".into()],
            env: vec![],
            flows_dir: to_wsl(&flows),
            install_files: vec![unc(&virtuoso_linux)],
            agent_processes: vec!["virtuoso-agent".into()],
        },
    ]
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let repo = std::env::current_dir()?;
    match args.first().map(String::as_str) {
        Some("run") => {
            let all = tools(&repo);
            let names: Vec<String> = arg(&args, "--tools")
                .map(|t| t.split(',').map(String::from).collect())
                .unwrap_or_else(|| all.iter().map(|t| t.name.clone()).collect());
            let chosen: Vec<Tool> = all.into_iter().filter(|t| names.contains(&t.name)).collect();
            let scenarios: Vec<String> = arg(&args, "--scenarios")
                .map(|s| s.split(',').filter(|s| !s.is_empty()).map(String::from).collect())
                .unwrap_or_else(|| DEVICE_SCENARIOS.iter().map(|s| s.to_string()).chain(["trivial".into(), "invalid".into()]).collect());
            let version_runs: usize = arg(&args, "--version-runs").map(|s| s.parse()).transpose()?.unwrap_or(10);
            let iterations: usize = arg(&args, "--iterations").map(|s| s.parse()).transpose()?.unwrap_or(5);
            let serial = arg(&args, "--device").unwrap_or_else(|| "emulator-5554".into());
            let out = PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "bench/results/results.json".into()));

            let mut statics = BTreeMap::new();
            for t in &chosen {
                // The version run starts WSL first, so its files can be read after it.
                let version = version_text(t);
                statics.insert(t.name.clone(), StaticInfo { install_bytes: install_bytes(&t.install_files), version });
            }
            let versions: Vec<(String, String)> = statics.iter().map(|(t, s)| (t.clone(), s.version.clone())).collect();
            if let Some(why) = agent_mismatch(&versions) {
                bail!("{why}");
            }
            for (t, v) in &versions {
                println!("{t}: {v}");
            }
            let progress = |line: &str| println!("{line}");
            let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            let mut meta = BTreeMap::new();
            meta.insert("Device".into(), device_description(&serial));
            meta.insert("Host".into(), env_or("VIRTUOSO_BENCH_HOST", "Windows 11, 20 logical CPUs"));
            meta.insert("Iterations".into(), format!("{iterations} recorded runs per tool and scenario, after one warm-up"));
            meta.insert("Started (Unix time)".into(), started.to_string());
            meta.insert(
                "Where each tool ran".into(),
                "virtuoso and speedrun (which runs on macOS/Linux only) in WSL 2 (Ubuntu 24.04); maestro-runner natively on Windows (its Linux build cannot forward ports through WSL to the Windows adb server); virtuoso-windows natively on Windows as a supplement. All drive the same emulator through the same Windows adb server.".into(),
            );
            let primary: Vec<String> = chosen.iter().map(|t| t.name.clone()).filter(|n| n != "virtuoso-windows").collect();
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let save = |runs: &[virtuoso_bench::metrics::RunRecord]| {
                let results = Results { meta: meta.clone(), tools: primary.clone(), statics: statics.clone(), runs: runs.to_vec() };
                if let Ok(text) = serde_json::to_string_pretty(&results) {
                    let _ = std::fs::write(&out, text);
                }
            };
            let plan = Plan { serial: &serial, tools: &chosen, scenarios: &scenarios, iterations, version_runs, progress: &progress, checkpoint: &save };
            let mut runs = execute(&plan)?;
            for r in &mut runs {
                r.batch = Some(started.to_string());
                r.version = statics.get(&r.tool).map(|s| s.version.clone());
            }
            if let Some(base) = arg(&args, "--merge-into") {
                // Rerun some tools or scenarios into an existing results file.
                let mut old: Results = serde_json::from_str(&std::fs::read_to_string(&base)?)?;
                for r in old.runs.iter_mut().filter(|r| r.version.is_none()) {
                    r.version = old.statics.get(&r.tool).map(|s| s.version.clone());
                }
                runs = virtuoso_bench::orchestrate::merge_runs(old.runs, runs);
                old.statics.extend(statics.clone());
                let merged = Results { runs: runs.clone(), statics: old.statics, ..old };
                std::fs::write(&base, serde_json::to_string_pretty(&merged)?)?;
                println!("merged into {base}");
            }
            save(&runs);
            println!("wrote {}", out.display());
        }
        Some("report") => {
            let input = args.get(1).context("virtuoso-bench report <results.json> [--out report.html]")?;
            let results: Results = serde_json::from_str(&std::fs::read_to_string(input)?)?;
            let features = serde_json::from_str(&std::fs::read_to_string(repo.join("bench").join("features.json"))?)?;
            let out = PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "report/index.html".into()));
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let html = if args.iter().any(|a| a == "--fragment") {
                virtuoso_bench::report::render_fragment(&results, &features)
            } else {
                virtuoso_bench::report::render(&results, &features)
            };
            std::fs::write(&out, html)?;
            println!("wrote {}", out.display());
        }
        _ => bail!("usage: virtuoso-bench run [--iterations N] [--tools a,b] [--scenarios a,b] [--device serial] [--out file]\n       virtuoso-bench report <results.json> [--out report.html]"),
    }
    Ok(())
}

fn device_description(serial: &str) -> String {
    let prop = |p: &str| {
        std::process::Command::new("adb")
            .args(["-s", serial, "shell", "getprop", p])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    format!("{serial}: {} Android {} (API {}), x86_64 emulator, host GPU", prop("ro.product.model"), prop("ro.build.version.release"), prop("ro.build.version.sdk"))
}
