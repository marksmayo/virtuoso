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
