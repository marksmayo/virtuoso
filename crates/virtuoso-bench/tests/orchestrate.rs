//! Pieces of the benchmark run that need no device.

use virtuoso_bench::measure::Measurement;
use virtuoso_bench::orchestrate::{install_bytes, passed, rss_of};

#[test]
fn agent_memory_sums_the_named_processes() {
    let ps = "     RSS NAME\n  123456 system_server\n   51200 virtuoso-agent\n   10240 dev.devicelab.driver.android\n    2048 sh\n";
    assert_eq!(rss_of(ps, &["virtuoso-agent".into()]), Some(51200.0));
    assert_eq!(rss_of(ps, &["dev.devicelab.driver.android".into(), "virtuoso-agent".into()]), Some(61440.0));
    assert_eq!(rss_of(ps, &["speedrun-agent".into()]), None);
}

#[test]
fn an_invalid_flow_passes_when_it_is_rejected() {
    let m = |exit| Measurement { exit, ..Default::default() };
    assert!(passed("invalid", &m(1)));
    assert!(!passed("invalid", &m(0)));
    assert!(passed("navigate", &m(0)));
    assert!(!passed("navigate", &m(1)));
}

#[test]
fn install_size_adds_files_and_folders() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("bin"), vec![0u8; 1000]).unwrap();
    std::fs::create_dir(d.path().join("drivers")).unwrap();
    std::fs::write(d.path().join("drivers/a.apk"), vec![0u8; 500]).unwrap();
    let files = vec![d.path().join("bin").display().to_string(), d.path().join("drivers").display().to_string()];
    assert_eq!(install_bytes(&files), Some(1500.0));
    assert_eq!(install_bytes(&["/no/such/file".into()]), None);
}

#[test]
fn a_run_the_harness_could_not_start_is_a_failed_run_not_an_abort() {
    use virtuoso_bench::orchestrate::measured;
    let m = measured(Err(anyhow::anyhow!("wsl: distro not found")));
    assert_eq!(m.exit, -1);
    assert!(m.output.contains("distro not found"));
    assert!(!passed("navigate", &m));
}

#[test]
fn merging_replaces_only_the_runs_of_the_tools_and_scenarios_rerun() {
    use virtuoso_bench::metrics::RunRecord;
    use virtuoso_bench::orchestrate::merge_runs;
    let r = |tool: &str, scenario: &str, wall: f64| RunRecord {
        tool: tool.into(),
        scenario: scenario.into(),
        iteration: 0,
        wall_ms: wall,
        peak_rss_kb: None,
        cpu_ms: None,
        passed: true,
        agent_rss_kb: None,
        error: None,
        ..Default::default()
    };
    let old = vec![r("virtuoso", "version", 0.0), r("b", "version", 0.0), r("virtuoso", "navigate", 5.0)];
    let new = vec![r("virtuoso", "version", 3.2)];
    let merged = merge_runs(old, new);
    assert_eq!(merged.len(), 3);
    assert!(merged.iter().any(|x| x.tool == "virtuoso" && x.scenario == "version" && x.wall_ms == 3.2));
    assert!(merged.iter().any(|x| x.tool == "b" && x.scenario == "version" && x.wall_ms == 0.0));
    assert!(merged.iter().any(|x| x.scenario == "navigate"));
}

#[test]
fn merging_keeps_a_competitors_earlier_batches_of_the_same_version() {
    use virtuoso_bench::metrics::RunRecord;
    use virtuoso_bench::orchestrate::merge_runs;
    let r = |tool: &str, batch: &str, version: &str| RunRecord {
        tool: tool.into(),
        scenario: "navigate".into(),
        batch: Some(batch.into()),
        version: Some(version.into()),
        passed: true,
        ..Default::default()
    };
    let old = vec![r("speedrun", "1", "0.2.8"), r("maestro-runner", "1", "1.0"), r("virtuoso", "1", "0.1")];
    let new = vec![r("speedrun", "2", "0.2.8"), r("maestro-runner", "2", "1.1"), r("virtuoso", "2", "0.1")];
    let merged = merge_runs(old, new);
    let batches = |tool: &str| merged.iter().filter(|x| x.tool == tool).map(|x| x.batch.clone().unwrap()).collect::<Vec<_>>();
    assert_eq!(batches("speedrun"), ["1", "2"], "same version: the earlier batch stays");
    assert_eq!(batches("maestro-runner"), ["2"], "new version: earlier batches go");
    assert_eq!(batches("virtuoso"), ["2"], "virtuoso: only the latest run counts");
}

#[test]
fn the_bench_runs_the_newest_build() {
    // A stale target/release binary once cost virtuoso 100-280 ms of CPU a run.
    let d = tempfile::tempdir().unwrap();
    let put = |dir: &str| {
        let p = d.path().join(dir);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("virtuoso.exe"), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    put("target/release");
    put("target/dev3/release");
    let newest = virtuoso_bench::orchestrate::newest_build(d.path(), "virtuoso.exe").unwrap();
    assert!(newest.ends_with("target/dev3/release/virtuoso.exe"), "{}", newest.display());
    assert!(virtuoso_bench::orchestrate::newest_build(d.path(), "other.exe").is_none());
}

#[test]
fn virtuoso_builds_with_different_agents_are_refused() {
    use virtuoso_bench::orchestrate::agent_mismatch;
    let v = |n: &str, s: &str| (n.to_string(), s.to_string());
    assert_eq!(agent_mismatch(&[v("virtuoso", "virtuoso 0.1.0 (agent 00aa)"), v("virtuoso-windows", "virtuoso 0.1.0 (agent 00aa)"), v("speedrun", "speedrun v0.2.8")]), None);
    let m = agent_mismatch(&[v("virtuoso", "virtuoso 0.1.0 (agent 00aa)"), v("virtuoso-windows", "virtuoso 0.1.0 (agent 11bb)")]).unwrap();
    assert!(m.contains("00aa") && m.contains("11bb"), "{m}");
}

fn tool(app_flows: Option<&str>, skips: &[&str]) -> virtuoso_bench::orchestrate::Tool {
    virtuoso_bench::orchestrate::Tool {
        name: "t".into(),
        runner: virtuoso_bench::measure::Runner::Native,
        test_argv: vec!["t".into(), "test".into()],
        version_argv: vec![],
        env: vec![],
        flows_dir: "/flows".into(),
        suite_dirs: app_flows.map(|d| [("app".to_string(), d.to_string())].into_iter().collect()).unwrap_or_default(),
        flow_ext: "yaml".into(),
        skips: skips.iter().map(|s| s.to_string()).collect(),
        install_files: vec![],
        agent_processes: vec![],
    }
}

#[test]
fn a_settings_scenario_runs_its_flow_from_the_flows_folder() {
    use virtuoso_bench::orchestrate::invocation;
    let (argv, cwd) = invocation(&tool(Some("/app/flows"), &[]), "navigate").unwrap();
    assert_eq!(argv, ["t", "test", "navigate.yaml"]);
    assert_eq!(cwd, "/flows");
}

#[test]
fn a_bench_app_flow_runs_from_the_app_flows_folder() {
    use virtuoso_bench::orchestrate::invocation;
    let (argv, cwd) = invocation(&tool(Some("/app/flows"), &[]), "app/chat").unwrap();
    assert_eq!(argv, ["t", "test", "chat.yaml"]);
    assert_eq!(cwd, "/app/flows");
}

#[test]
fn a_mobile_benchmark_flow_runs_from_that_suites_folder() {
    use virtuoso_bench::orchestrate::invocation;
    let mut t = tool(Some("/app/flows"), &[]);
    t.suite_dirs.insert("tam".into(), "/tam/flows".into());
    let (argv, cwd) = invocation(&t, "tam/login_form").unwrap();
    assert_eq!(argv, ["t", "test", "login_form.yaml"]);
    assert_eq!(cwd, "/tam/flows");
    // A tool with no tests for a suite is not run on it.
    assert!(invocation(&tool(Some("/app/flows"), &[]), "tam/login_form").is_none());
}

#[test]
fn a_tool_whose_tests_are_not_yaml_names_them_by_its_own_extension() {
    use virtuoso_bench::orchestrate::invocation;
    let mut t = tool(Some("/app/flows"), &[]);
    t.flow_ext = "e2e.ts".into();
    assert_eq!(invocation(&t, "navigate").unwrap().0, ["t", "test", "navigate.e2e.ts"]);
    assert_eq!(invocation(&t, "app/chat").unwrap().0, ["t", "test", "chat.e2e.ts"]);
}

#[test]
fn a_tool_runs_on_the_device_the_benchmark_was_given() {
    let mut t = tool(None, &[]);
    t.test_argv = vec!["t".into(), "--device".into(), "{device}".into(), "test".into()];
    t.env = vec![("DEVICE".into(), "{device}".into()), ("OTHER".into(), "1".into())];
    let t = t.on_device("emulator-5556");
    assert_eq!(t.test_argv, ["t", "--device", "emulator-5556", "test"]);
    assert_eq!(t.env, [("DEVICE".to_string(), "emulator-5556".to_string()), ("OTHER".into(), "1".into())]);
}

#[test]
fn a_tool_is_not_run_on_what_it_has_no_test_for() {
    use virtuoso_bench::orchestrate::invocation;
    assert!(invocation(&tool(Some("/app/flows"), &["invalid"]), "invalid").is_none());
    assert!(invocation(&tool(None, &[]), "app/chat").is_none());
}

#[test]
fn the_reset_stops_every_tools_agent_processes_and_apps() {
    use virtuoso_bench::orchestrate::reset_script;
    let mut a = tool(None, &[]);
    a.agent_processes = vec!["virtuoso-agent".into()];
    let mut b = tool(None, &[]);
    b.agent_processes = vec!["io.appium.uiautomator2.server".into(), "dev.mobile.maestro".into()];
    let script = reset_script(&[a, b]);
    assert!(script.contains("pidof virtuoso-agent"), "{script}");
    assert!(script.contains("am force-stop io.appium.uiautomator2.server;"), "{script}");
    assert!(script.contains("am force-stop dev.mobile.maestro;"), "{script}");
    // The bench app starts from no saved state too, like Settings.
    assert!(script.contains("pm clear com.android.settings"), "{script}");
    assert!(script.contains("pm clear dev.speedrun.bench"), "{script}");
    assert!(script.contains("pm clear dev.e2e.benchmark"), "{script}");
    // A stylus-like tap (Argent's) made Gboard open its handwriting tutorial over the app.
    assert!(script.contains("settings put secure stylus_handwriting_enabled 0"), "{script}");
}

#[test]
fn a_run_of_some_tools_still_stops_the_agents_the_others_left_behind() {
    use virtuoso_bench::orchestrate::Plan;
    let mut speedrun = tool(None, &[]);
    speedrun.agent_processes = vec!["speedrun-agent".into()];
    let mut argent = tool(None, &[]);
    argent.agent_processes = vec!["com.argent.androiddevtools".into()];
    let chosen = [speedrun.clone()];
    let known = [speedrun, argent];
    let plan = Plan {
        serial: "emulator-5554",
        tools: &chosen,
        known: &known,
        scenarios: &[],
        iterations: 0,
        version_runs: 0,
        progress: &|_| {},
        checkpoint: &|_| {},
    };
    // Argent's devtools app, still holding the UiAutomation connection from an earlier run.
    let script = plan.reset_script();
    assert!(script.contains("am force-stop com.argent.androiddevtools;"), "{script}");
    assert!(script.contains("pidof speedrun-agent"), "{script}");
}

#[test]
fn the_bench_app_scenarios_are_its_flow_files_without_the_suite_config_or_subflows() {
    use virtuoso_bench::orchestrate::app_scenarios;
    let d = tempfile::tempdir().unwrap();
    for f in ["chat.yaml", "cart.yaml", "config.yaml", "notes.md"] {
        std::fs::write(d.path().join(f), "").unwrap();
    }
    std::fs::create_dir(d.path().join("_shared")).unwrap();
    std::fs::write(d.path().join("_shared/guest.yaml"), "").unwrap();
    assert_eq!(app_scenarios(d.path(), "app"), ["app/cart", "app/chat"]);
    assert_eq!(app_scenarios(d.path(), "tam"), ["tam/cart", "tam/chat"]);
}

#[test]
fn merging_adds_new_tools_to_the_results_after_the_old_ones() {
    use virtuoso_bench::orchestrate::merge_tools;
    let old = vec!["maestro-runner".to_string(), "speedrun".into(), "virtuoso".into()];
    let new = vec!["speedrun".to_string(), "virtuoso".into(), "maestro".into(), "appium".into()];
    assert_eq!(merge_tools(&old, &new), ["maestro-runner", "speedrun", "virtuoso", "maestro", "appium"]);
}

#[test]
fn the_real_java_is_found_from_its_settings_not_the_path_redirector() {
    use virtuoso_bench::orchestrate::java_home;
    let out = "Property settings:\n    file.encoding = UTF-8\n    java.home = C:\\Program Files\\Java\\jdk-17\n    java.vendor = Oracle\n";
    assert_eq!(java_home(out).as_deref(), Some(r"C:\Program Files\Java\jdk-17"));
    assert_eq!(java_home("no java here"), None);
}

#[test]
fn results_from_separate_jobs_combine_into_one() {
    use std::collections::BTreeMap;
    use virtuoso_bench::metrics::{RunRecord, StaticInfo};
    use virtuoso_bench::orchestrate::{combine_results, Results};
    let run = |tool: &str, scenario: &str| RunRecord { tool: tool.into(), scenario: scenario.into(), passed: true, ..Default::default() };
    let job = |scenario: &str, tools: &[&str]| Results {
        meta: BTreeMap::from([("Device".to_string(), "emulator-5554".to_string()), ("Job".to_string(), scenario.to_string())]),
        tools: tools.iter().map(|t| t.to_string()).collect(),
        statics: tools.iter().map(|t| (t.to_string(), StaticInfo { install_bytes: Some(1.0), version: format!("{t} 1") })).collect(),
        runs: tools.iter().map(|t| run(t, scenario)).collect(),
    };
    let all = combine_results(vec![job("launch", &["virtuoso", "maestro"]), job("tam/login_form", &["virtuoso", "maestro", "argent"])]);
    assert_eq!(all.runs.len(), 5, "every job's runs");
    assert_eq!(all.tools, vec!["virtuoso", "maestro", "argent"]);
    assert_eq!(all.statics.len(), 3);
    assert_eq!(all.meta["Device"], "emulator-5554");
}
