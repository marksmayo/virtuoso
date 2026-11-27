//! Turning raw runs into medal metrics.

use virtuoso_bench::metrics::{compute, RunRecord, StaticInfo, DEVICE_SCENARIOS};
use std::collections::BTreeMap;

fn rec(tool: &str, scenario: &str, wall: f64, passed: bool) -> RunRecord {
    RunRecord {
        tool: tool.into(),
        scenario: scenario.into(),
        iteration: 0,
        wall_ms: wall,
        peak_rss_kb: Some(wall / 10.0),
        cpu_ms: Some(wall / 100.0),
        passed,
        agent_rss_kb: Some(1000.0),
        error: None,
        ..Default::default()
    }
}

/// Three runs of one scenario, recorded in batch `batch` at `version`.
fn batch(tool: &str, scenario: &str, batch: &str, version: &str, wall: f64, rss: f64) -> Vec<RunRecord> {
    (0..3)
        .map(|i| RunRecord {
            iteration: i,
            peak_rss_kb: Some(rss),
            batch: Some(batch.into()),
            version: Some(version.into()),
            ..rec(tool, scenario, wall, true)
        })
        .collect()
}

fn all_scenarios(tool: &str, wall: f64) -> Vec<RunRecord> {
    let mut v = Vec::new();
    for s in DEVICE_SCENARIOS {
        for i in 0..3 {
            v.push(rec(tool, s, wall + i as f64, true));
        }
    }
    v
}

fn statics() -> BTreeMap<String, StaticInfo> {
    let mut m = BTreeMap::new();
    m.insert("a".to_string(), StaticInfo { install_bytes: Some(4_000_000.0), version: "1".into() });
    m.insert("b".to_string(), StaticInfo { install_bytes: Some(30_000_000.0), version: "2".into() });
    m
}

fn metric<'a>(ms: &'a [virtuoso_bench::medals::MetricResult], key: &str) -> &'a virtuoso_bench::medals::MetricResult {
    ms.iter().find(|m| m.key == key).unwrap_or_else(|| panic!("no metric {key}: {:?}", ms.iter().map(|m| &m.key).collect::<Vec<_>>()))
}

#[test]
fn each_device_scenario_is_a_metric_of_median_wall_time_in_seconds() {
    let mut runs = all_scenarios("a", 1000.0);
    runs.extend(all_scenarios("b", 3000.0));
    let ms = compute(&runs, &statics(), &["a".into(), "b".into()]);
    let nav = metric(&ms, "exec-navigate");
    assert_eq!(nav.group, "Execution");
    assert_eq!(nav.values["a"], Some(1.001));
    assert_eq!(nav.values["b"], Some(3.001));
}

#[test]
fn a_scenario_a_tool_mostly_fails_has_no_value_for_it() {
    let mut runs = all_scenarios("a", 1000.0);
    for r in runs.iter_mut().filter(|r| r.scenario == "scroll").take(2) {
        r.passed = false;
    }
    let ms = compute(&runs, &statics(), &["a".into()]);
    assert_eq!(metric(&ms, "exec-scroll").values["a"], None);
    assert_eq!(metric(&ms, "exec-total").values["a"], None, "no suite total without every scenario");
}

#[test]
fn the_suite_total_sums_scenario_medians() {
    let runs = all_scenarios("a", 1000.0);
    let ms = compute(&runs, &statics(), &["a".into()]);
    let n = DEVICE_SCENARIOS.len() as f64;
    assert!((metric(&ms, "exec-total").values["a"].unwrap() - 1.001 * n).abs() < 1e-9);
}

#[test]
fn reliability_counts_passing_runs_and_consistency_uses_variation() {
    let mut runs = all_scenarios("a", 1000.0);
    runs[0].passed = false;
    let ms = compute(&runs, &statics(), &["a".into()]);
    let total = (DEVICE_SCENARIOS.len() * 3) as f64;
    assert!((metric(&ms, "pass-rate").values["a"].unwrap() - (total - 1.0) / total * 100.0).abs() < 1e-9);
    assert!(metric(&ms, "consistency").values["a"].unwrap() < 1.0);
}

#[test]
fn startup_metrics_come_from_their_scenarios_and_static_info() {
    let mut runs = all_scenarios("a", 1000.0);
    runs.push(rec("a", "version", 12.0, true));
    runs.push(rec("a", "version", 14.0, true));
    runs.push(rec("a", "trivial", 2500.0, true));
    runs.push(rec("a", "invalid", 300.0, true));
    let ms = compute(&runs, &statics(), &["a".into()]);
    assert_eq!(metric(&ms, "cli-startup").values["a"], Some(13.0));
    assert_eq!(metric(&ms, "first-command").values["a"], Some(2.5));
    assert_eq!(metric(&ms, "error-detection").values["a"], Some(300.0));
    assert_eq!(metric(&ms, "install-size").values["a"], Some(4.0));
}

#[test]
fn resource_metrics_are_medians_over_device_runs() {
    let runs = all_scenarios("a", 1000.0);
    let ms = compute(&runs, &statics(), &["a".into()]);
    assert_eq!(metric(&ms, "host-memory").group, "Resources");
    assert!(metric(&ms, "host-memory").values["a"].unwrap() > 0.0);
    assert_eq!(metric(&ms, "agent-memory").values["a"], Some(1000.0 / 1024.0));
    assert!(metric(&ms, "host-cpu").values["a"].is_some());
}

#[test]
fn a_competitor_keeps_its_fastest_batch_of_the_current_version() {
    let mut runs = batch("b", "navigate", "100", "2", 3000.0, 9000.0);
    runs.extend(batch("b", "navigate", "200", "2", 4000.0, 9000.0));
    let ms = compute(&runs, &statics(), &["b".into()]);
    assert_eq!(metric(&ms, "exec-navigate").values["b"], Some(3.0));
}

#[test]
fn a_competitor_keeps_its_lowest_memory_even_from_a_slower_batch() {
    let mut runs = batch("b", "navigate", "100", "2", 3000.0, 20480.0);
    runs.extend(batch("b", "navigate", "200", "2", 4000.0, 10240.0));
    let ms = compute(&runs, &statics(), &["b".into()]);
    assert_eq!(metric(&ms, "exec-navigate").values["b"], Some(3.0));
    assert_eq!(metric(&ms, "host-memory").values["b"], Some(10.0));
}

#[test]
fn a_competitor_batch_of_another_version_is_ignored() {
    let mut runs = batch("b", "navigate", "100", "1", 1000.0, 1024.0);
    runs.extend(batch("b", "navigate", "200", "2", 4000.0, 10240.0));
    let ms = compute(&runs, &statics(), &["b".into()]);
    assert_eq!(metric(&ms, "exec-navigate").values["b"], Some(4.0));
    assert_eq!(metric(&ms, "host-memory").values["b"], Some(10.0));
}

#[test]
fn virtuoso_counts_only_its_latest_batch() {
    let mut s = statics();
    s.insert("virtuoso".into(), StaticInfo { install_bytes: None, version: "3".into() });
    let mut runs = batch("virtuoso", "navigate", "100", "3", 1000.0, 1024.0);
    runs.extend(batch("virtuoso", "navigate", "200", "3", 2000.0, 2048.0));
    let ms = compute(&runs, &s, &["virtuoso".into()]);
    assert_eq!(metric(&ms, "exec-navigate").values["virtuoso"], Some(2.0));
    assert_eq!(metric(&ms, "host-memory").values["virtuoso"], Some(2.0));
}

#[test]
fn tester_armys_mobile_benchmark_is_a_suite_of_its_own() {
    let runs = vec![rec("a", "tam/login_form", 3_000.0, true), rec("a", "app/chat", 10_000.0, true), rec("b", "tam/login_form", 4_000.0, false)];
    let ms = compute(&runs, &statics(), &["a".into(), "b".into()]);
    let login = metric(&ms, "tam-login_form");
    assert_eq!(login.group, "Tester Army mobile benchmark");
    assert_eq!(login.values["a"], Some(3.0));
    assert_eq!(metric(&ms, "tam-total").values["a"], Some(3.0));
    assert_eq!(metric(&ms, "tam-passed").values["b"], Some(0.0));
    // Each suite totals its own flows.
    assert_eq!(metric(&ms, "app-total").values["a"], Some(10.0));
}

#[test]
fn each_bench_app_flow_is_a_metric_with_a_suite_total_and_a_pass_rate() {
    let mut runs = vec![rec("a", "app/chat", 10_000.0, true), rec("a", "app/cart", 20_000.0, true)];
    runs.extend([rec("b", "app/chat", 40_000.0, true), rec("b", "app/cart", 5_000.0, false)]);
    let ms = compute(&runs, &statics(), &["a".into(), "b".into(), "c".into()]);
    let chat = metric(&ms, "app-chat");
    assert_eq!(chat.group, "Speedrun Bench app");
    assert_eq!(chat.values["a"], Some(10.0));
    assert_eq!(chat.values["b"], Some(40.0));
    assert_eq!(metric(&ms, "app-cart").values["b"], None, "a failed flow has no time");
    assert_eq!(metric(&ms, "app-total").values["a"], Some(30.0));
    assert_eq!(metric(&ms, "app-total").values["b"], None, "no suite total without every flow");
    assert_eq!(metric(&ms, "app-passed").values["a"], Some(100.0));
    assert_eq!(metric(&ms, "app-passed").values["b"], Some(50.0));
    assert_eq!(metric(&ms, "app-passed").values["c"], None, "a tool that cannot run the flows");
    // The Settings metrics leave the app's runs out.
    assert_eq!(metric(&ms, "pass-rate").values["b"], None);
}

#[test]
fn without_bench_app_runs_there_are_no_bench_app_metrics() {
    let ms = compute(&all_scenarios("a", 1000.0), &statics(), &["a".into()]);
    assert!(ms.iter().all(|m| m.group != "Speedrun Bench app"));
}

#[test]
fn the_web_flows_are_a_suite_with_a_time_each_and_the_whole_suites_resources() {
    // A web field has only web/<flow> runs: a time per flow, the suite's total and share passed,
    // and memory, CPU, runs passed and variation over those runs (no device scenarios there).
    let mut runs = Vec::new();
    for (t, wall) in [("a", 1000.0), ("b", 3000.0)] {
        for f in ["login_form", "div_soup"] {
            for i in 0..3 {
                runs.push(rec(t, &format!("web/{f}"), wall + i as f64, true));
            }
        }
    }
    let ms = compute(&runs, &statics(), &["a".to_string(), "b".to_string()]);
    assert_eq!(metric(&ms, "web-login_form").values["a"], Some(1.001));
    assert_eq!(metric(&ms, "web-total").values["b"], Some(6.002));
    assert_eq!(metric(&ms, "web-passed").values["a"], Some(100.0));
    assert_eq!(metric(&ms, "web-login_form").group, "Tester Army web benchmark");
    assert!(metric(&ms, "host-memory").values["a"].is_some(), "memory from the web runs");
    assert_eq!(metric(&ms, "pass-rate").values["b"], Some(100.0));
    assert!(metric(&ms, "host-cpu").values["a"].is_some());
    assert!(metric(&ms, "consistency").values["a"].is_some());
}

/// Three passing runs of a web flow at these wall times.
fn three(tool: &str, flow: &str, walls: [f64; 3]) -> Vec<RunRecord> {
    walls.iter().map(|w| rec(tool, &format!("web/{flow}"), *w, true)).collect()
}

#[test]
fn variation_is_averaged_over_the_scenarios_every_tool_passes_three_times() {
    // a passes both flows three times; b passes only the steady one. a's unsteady flow would
    // count against a alone, so both are scored on the steady flow only.
    let mut runs = three("a", "steady", [1000.0, 1000.0, 1000.0]);
    runs.extend(three("a", "unsteady", [1000.0, 2000.0, 3000.0]));
    runs.extend(three("b", "steady", [900.0, 1000.0, 1100.0]));
    runs.extend([rec("b", "web/unsteady", 1000.0, true), rec("b", "web/unsteady", 1000.0, false), rec("b", "web/unsteady", 1000.0, false)]);
    let ms = compute(&runs, &statics(), &["a".into(), "b".into()]);
    let variation = metric(&ms, "consistency");
    assert_eq!(variation.values["a"], Some(0.0), "a's unsteady flow is left out");
    // Sample standard deviation 100 ms over a mean of 1000 ms.
    let b = variation.values["b"].unwrap();
    assert!((b - 10.0).abs() < 1e-9, "b's steady flow only: {b}");
}

#[test]
fn variation_in_ms_is_each_tools_mean_sd_over_the_same_scenarios_as_its_cv() {
    // As above: both tools are scored on the steady flow only.
    let mut runs = three("a", "steady", [1000.0, 1000.0, 1000.0]);
    runs.extend(three("a", "unsteady", [1000.0, 2000.0, 3000.0]));
    runs.extend(three("b", "steady", [900.0, 1000.0, 1100.0]));
    runs.extend(three("b", "slow", [9900.0, 10_000.0, 10_100.0]));
    runs.extend([rec("b", "web/unsteady", 1000.0, true), rec("b", "web/unsteady", 1000.0, false), rec("b", "web/unsteady", 1000.0, false)]);
    let sd = virtuoso_bench::metrics::variation_sd_ms(&runs, &statics(), &["a".into(), "b".into()]);
    assert_eq!(sd["a"], 0.0);
    assert!((sd["b"] - 100.0).abs() < 1e-9, "b's steady flow only: {sd:?}");
}

#[test]
fn without_a_variation_row_there_is_no_variation_in_ms() {
    let mut runs = three("a", "one", [1000.0, 1100.0, 1200.0]);
    runs.extend(three("b", "two", [1000.0, 1100.0, 1200.0]));
    assert!(virtuoso_bench::metrics::variation_sd_ms(&runs, &statics(), &["a".into(), "b".into()]).is_empty());
}

#[test]
fn without_a_scenario_every_tool_passes_three_times_there_is_no_variation_row() {
    let mut runs = three("a", "one", [1000.0, 1100.0, 1200.0]);
    runs.extend(three("b", "two", [1000.0, 1100.0, 1200.0]));
    runs.push(rec("b", "web/one", 1000.0, false));
    runs.push(rec("a", "web/two", 1000.0, false));
    let ms = compute(&runs, &statics(), &["a".into(), "b".into()]);
    assert!(ms.iter().all(|m| m.key != "consistency"), "{:?}", ms.iter().map(|m| &m.key).collect::<Vec<_>>());
}

#[test]
fn a_field_that_ran_no_device_scenario_has_no_rows_for_them() {
    // The web fields run only web/<flow>: rows for the device scenarios, the invalid flow, the
    // one-command flow and the device agent would be blank in every column.
    let runs: Vec<RunRecord> = (0..3).map(|i| RunRecord { agent_rss_kb: None, ..rec("a", "web/login_form", 1000.0 + i as f64, true) }).collect();
    let ms = compute(&runs, &statics(), &["a".to_string()]);
    let keys: Vec<&str> = ms.iter().map(|m| m.key.as_str()).collect();
    for gone in ["exec-launch", "exec-taps", "exec-total", "error-detection", "first-command", "agent-memory"] {
        assert!(!keys.contains(&gone), "{gone} in {keys:?}");
    }
    for kept in ["install-size", "host-memory", "pass-rate", "consistency", "web-login_form"] {
        assert!(keys.contains(&kept), "{kept} missing from {keys:?}");
    }
}
