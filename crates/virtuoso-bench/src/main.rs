//! `virtuoso-bench run` measures the tools and writes results JSON; `virtuoso-bench report` turns
//! results (and bench/features.json) into the HTML medal report.
//!
//! The tool set is the host's (see `tools`): on Windows, virtuoso (Linux build) and speedrun
//! (POSIX only) in the WSL distribution VIRTUOSO_BENCH_WSL, maestro-runner natively, and
//! virtuoso-windows as a supplement; on Linux and macOS, every tool natively.

use anyhow::{bail, Context, Result};
use virtuoso_bench::tools::{app_flows, env_or, tools_for, Host, Platform};
use virtuoso_bench::metrics::{StaticInfo, DEVICE_SCENARIOS};
use virtuoso_bench::orchestrate::{agent_mismatch, app_scenarios, execute, install_bytes, version_text, Plan, Results, Tool};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let repo = std::env::current_dir()?;
    match args.first().map(String::as_str) {
        Some("run") => {
            // `--platform ios`: the tools on a booted simulator (--device <udid>).
            let platform = if arg(&args, "--platform").as_deref() == Some("ios") { Platform::Ios } else { Platform::Android };
            let all = tools_for(&repo, &Host::here(), platform);
            let names: Vec<String> = arg(&args, "--tools")
                .map(|t| t.split(',').map(String::from).collect())
                .unwrap_or_else(|| all.iter().map(|t| t.name.clone()).collect());
            let chosen: Vec<Tool> = all.iter().filter(|t| names.contains(&t.name)).cloned().collect();
            let scenarios: Vec<String> = arg(&args, "--scenarios")
                // `app`: every flow of the bench app.
                .map(|s| {
                    s.split(',')
                        .filter(|s| !s.is_empty())
                        .flat_map(|s| match s {
                            "app" => app_scenarios(&app_flows(&repo), "app"),
                            "tam" => app_scenarios(&repo.join("bench").join("tam-flows"), "tam"),
                            _ => vec![s.to_string()],
                        })
                        .collect()
                })
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
            meta.insert("Device".into(), if platform == Platform::Ios { simulator_description(&serial) } else { device_description(&serial) });
            meta.insert("Host".into(), env_or("VIRTUOSO_BENCH_HOST", "Windows 11, 20 logical CPUs"));
            meta.insert("Iterations".into(), format!("{iterations} recorded runs per tool and scenario, after one warm-up"));
            meta.insert("Started (Unix time)".into(), started.to_string());
            if let Ok(why) = std::env::var("VIRTUOSO_BENCH_NOT_MEASURED") {
                meta.insert("Not measured".into(), why);
            }
            meta.insert("Where each tool ran".into(), virtuoso_bench::tools::where_tools_ran(&Host::here(), platform));
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
            // --outputs <dir>: every run's whole output, for where the time went or why it failed.
            let outputs = arg(&args, "--outputs").map(std::path::PathBuf::from);
            let plan = Plan { serial: &serial, platform, tools: &chosen, known: &all, scenarios: &scenarios, iterations, version_runs, progress: &progress, checkpoint: &save, outputs: outputs.as_deref() };
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
                let tools = virtuoso_bench::orchestrate::merge_tools(&old.tools, &primary);
                let merged = Results { runs: runs.clone(), statics: old.statics, tools, ..old };
                std::fs::write(&base, serde_json::to_string_pretty(&merged)?)?;
                println!("merged into {base}");
            }
            save(&runs);
            println!("wrote {}", out.display());
        }
        Some("merge") => {
            // The results files of separate jobs (one per group of scenarios) as one.
            let usage = "virtuoso-bench merge <out.json> <results.json>...";
            let out = args.get(1).context(usage)?;
            let jobs = args[2..]
                .iter()
                .map(|f| Ok(serde_json::from_str(&std::fs::read_to_string(f).with_context(|| format!("reading {f}"))?)?))
                .collect::<Result<Vec<Results>>>()?;
            if jobs.is_empty() {
                bail!("{usage}");
            }
            std::fs::write(out, serde_json::to_string_pretty(&virtuoso_bench::orchestrate::combine_results(jobs))?)?;
            println!("wrote {out}");
        }
        Some("readme") => {
            // The README's results from several fields: --field "Android (Linux runner)=android.json"
            // once per field; each field's chart goes to --charts (default docs/bench).
            let usage = "virtuoso-bench readme --field <name>=<results.json>... [--charts docs/bench] [--readme README.md]";
            let mut fields = Vec::new();
            for (i, a) in args.iter().enumerate() {
                if a == "--field" {
                    let spec = args.get(i + 1).context(usage)?;
                    let (name, file) = spec.split_once('=').context(usage)?;
                    let results: Results = serde_json::from_str(&std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?)?;
                    fields.push(virtuoso_bench::fields::Field { name: name.to_string(), results });
                }
            }
            if fields.is_empty() {
                bail!("{usage}");
            }
            let charts = arg(&args, "--charts").unwrap_or_else(|| "docs/bench".into());
            std::fs::create_dir_all(repo.join(&charts))?;
            for f in &fields {
                let one = std::slice::from_ref(f);
                let metrics = virtuoso_bench::fields::field_metrics(one);
                let times = virtuoso_bench::fields::relative_times(&metrics, &f.results.tools, "virtuoso");
                let path = repo.join(&charts).join(format!("{}.svg", virtuoso_bench::fields::slug(&f.name)));
                std::fs::write(&path, virtuoso_bench::fields::relative_chart_svg(&f.name, &times, "virtuoso"))?;
                println!("wrote {}", path.display());
            }
            let features = serde_json::from_str(&std::fs::read_to_string(repo.join("bench").join("features.json"))?)?;
            let readme = PathBuf::from(arg(&args, "--readme").unwrap_or_else(|| repo.join("README.md").display().to_string()));
            let text = std::fs::read_to_string(&readme).with_context(|| format!("reading {}", readme.display()))?;
            let summary = virtuoso_bench::report::readme_fields_summary(&fields, &features, &charts.replace('\\', "/"));
            std::fs::write(&readme, virtuoso_bench::report::update_readme(&text, &summary))?;
            println!("updated {}", readme.display());
        }
        Some("report") => {
            let input = args.get(1).context("virtuoso-bench report <results.json> [--out report.html] [--readme README.md | --no-readme]")?;
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
            // The README carries the medal tables too, between its benchmark markers.
            if !args.iter().any(|a| a == "--no-readme") {
                let readme = PathBuf::from(arg(&args, "--readme").unwrap_or_else(|| repo.join("README.md").display().to_string()));
                let text = std::fs::read_to_string(&readme).with_context(|| format!("reading {}", readme.display()))?;
                let summary = virtuoso_bench::report::readme_summary(&results, &features);
                std::fs::write(&readme, virtuoso_bench::report::update_readme(&text, &summary))?;
                println!("updated {}", readme.display());
            }
        }
        _ => bail!("usage: virtuoso-bench run [--iterations N] [--tools a,b] [--scenarios a,b] [--device serial] [--out file]\n       virtuoso-bench report <results.json> [--out report.html] [--readme README.md | --no-readme]"),
    }
    Ok(())
}

/// A simulator by its name and iOS version, as simctl lists it.
fn simulator_description(udid: &str) -> String {
    let out = std::process::Command::new("xcrun").args(["simctl", "list", "devices", "-j"]).output();
    let json: serde_json::Value = out.ok().and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or_default();
    for (runtime, sims) in json["devices"].as_object().into_iter().flatten() {
        for sim in sims.as_array().into_iter().flatten() {
            if sim["udid"] == udid {
                // com.apple.CoreSimulator.SimRuntime.iOS-26-2 is iOS 26.2.
                let os = runtime.rsplit('.').next().unwrap_or("").replacen('-', " ", 1).replace('-', ".");
                return format!("{} ({os} simulator)", sim["name"].as_str().unwrap_or("simulator"));
            }
        }
    }
    format!("{udid}: iOS simulator")
}

fn device_description(serial: &str) -> String {
    let prop = |p: &str| {
        std::process::Command::new("adb")
            .args(["-s", serial, "shell", "getprop", p])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    // How the emulator draws: its host's GPU here, software on a CI runner (VIRTUOSO_BENCH_GPU).
    let gpu = std::env::var("VIRTUOSO_BENCH_GPU").unwrap_or_else(|_| "host GPU".into());
    format!("{serial}: {} Android {} (API {}), x86_64 emulator, {gpu}", prop("ro.product.model"), prop("ro.build.version.release"), prop("ro.build.version.sdk"))
}
