//! From raw runs to the metrics medals are awarded for.
//!
//! - Start-up: install size, CLI start (`--version`), time to report an invalid flow, and the
//!   whole run of a one-command flow (connecting to the device included).
//! - Execution: per scenario, the median wall time of a tool's passing runs (no value when
//!   fewer than half passed), and the suite total (only with every scenario).
//! - Resources: peak host memory and host CPU per run, peak memory of the on-device agent.
//! - Reliability: share of runs that passed, and run-to-run variation (lower is steadier).
//!
//! Runs are kept in batches (one benchmark invocation each). The competition (speedrun,
//! maestro-runner) is scored on its best batch at its current version, per scenario and per
//! measure; virtuoso is scored on its latest batch only.

use crate::medals::{Better, MetricResult};
use crate::stats::{cv, mean, median};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The scenarios that drive the device, in report order.
pub const DEVICE_SCENARIOS: &[&str] = &["launch", "navigate", "scroll", "input", "assertions", "taps"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunRecord {
    pub tool: String,
    pub scenario: String,
    pub iteration: usize,
    pub wall_ms: f64,
    pub peak_rss_kb: Option<f64>,
    pub cpu_ms: Option<f64>,
    /// Processes the run started, its own included (native runs only): a virtuoso run should
    /// start none but itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processes: Option<u32>,
    pub passed: bool,
    pub agent_rss_kb: Option<f64>,
    #[serde(default)]
    pub error: Option<String>,
    /// The benchmark invocation that recorded this run (its start, Unix seconds); none for runs
    /// recorded before batches were kept.
    #[serde(default)]
    pub batch: Option<String>,
    /// The tool's version when this run was recorded.
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StaticInfo {
    pub install_bytes: Option<f64>,
    pub version: String,
}

pub fn scenario_label(s: &str) -> &'static str {
    match s {
        "launch" => "App launch",
        "navigate" => "Navigation",
        "scroll" => "Scroll to element",
        "input" => "Text input",
        "assertions" => "Assertion burst",
        "taps" => "Repeated taps",
        _ => "Scenario",
    }
}

fn metric(key: &str, label: &str, group: &str, unit: &str, better: Better, tools: &[String], f: impl Fn(&str) -> Option<f64>) -> MetricResult {
    MetricResult {
        key: key.into(),
        label: label.into(),
        group: group.into(),
        unit: unit.into(),
        better,
        values: tools.iter().map(|t| (t.clone(), f(t))).collect(),
    }
}

/// Whether a tool is scored on its best batch at its current version rather than its latest.
pub fn keeps_best(tool: &str) -> bool {
    !tool.starts_with("virtuoso")
}

fn batch_order(r: &RunRecord) -> u64 {
    r.batch.as_deref().and_then(|b| b.parse().ok()).unwrap_or(0)
}

/// A batch's time for its scenario: the median of passing runs, worst when fewer than half passed.
fn batch_time(rs: &[&RunRecord]) -> f64 {
    let ok: Vec<f64> = rs.iter().filter(|r| r.passed).map(|r| r.wall_ms).collect();
    if ok.len() * 2 < rs.len() {
        return f64::INFINITY;
    }
    median(&ok).unwrap_or(f64::INFINITY)
}

fn lowest_median<'a, 'b>(batches: &'b [Vec<&'a RunRecord>], f: impl Fn(&RunRecord) -> Option<f64>) -> Option<&'b [&'a RunRecord]> {
    batches
        .iter()
        .filter_map(|b| median(&b.iter().filter_map(|r| f(r)).collect::<Vec<_>>()).map(|m| (m, b)))
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, b)| b.as_slice())
}

/// The runs the metrics are computed from: per tool and scenario, virtuoso's latest batch, or for
/// the competition the batch of its current version with the best time, its memory and CPU
/// figures taken from the batches where they were lowest.
pub fn scored_runs(runs: &[RunRecord], statics: &BTreeMap<String, StaticInfo>) -> Vec<RunRecord> {
    let mut groups: BTreeMap<(&str, &str), BTreeMap<u64, Vec<&RunRecord>>> = BTreeMap::new();
    for r in runs {
        let current = statics.get(&r.tool).map(|s| s.version.as_str());
        if r.version.is_some() && current.is_some() && r.version.as_deref() != current {
            continue;
        }
        groups.entry((&r.tool, &r.scenario)).or_default().entry(batch_order(r)).or_default().push(r);
    }
    let mut out = Vec::new();
    for ((tool, _), by_batch) in groups {
        let batches: Vec<Vec<&RunRecord>> = by_batch.into_values().collect();
        if !keeps_best(tool) {
            out.extend(batches.last().into_iter().flatten().map(|r| (*r).clone()));
            continue;
        }
        let Some(fastest) = batches.iter().min_by(|a, b| batch_time(a).total_cmp(&batch_time(b))) else { continue };
        let mut best: Vec<RunRecord> = fastest.iter().map(|r| (*r).clone()).collect();
        let take = |best: &mut Vec<RunRecord>, f: fn(&RunRecord) -> Option<f64>, set: fn(&mut RunRecord, Option<f64>)| {
            if let Some(from) = lowest_median(&batches, f) {
                for (i, r) in best.iter_mut().enumerate() {
                    set(r, from.get(i).and_then(|x| f(x)));
                }
            }
        };
        take(&mut best, |r| r.peak_rss_kb, |r, v| r.peak_rss_kb = v);
        take(&mut best, |r| r.cpu_ms, |r, v| r.cpu_ms = v);
        take(&mut best, |r| r.agent_rss_kb, |r, v| r.agent_rss_kb = v);
        out.extend(best);
    }
    out
}

pub fn compute(runs: &[RunRecord], statics: &BTreeMap<String, StaticInfo>, tools: &[String]) -> Vec<MetricResult> {
    let scored = scored_runs(runs, statics);
    let runs = scored.as_slice();
    let of = |tool: &str, scenario: &str| -> Vec<&RunRecord> { runs.iter().filter(|r| r.tool == tool && r.scenario == scenario).collect() };
    let device_runs = |tool: &str| -> Vec<&RunRecord> { runs.iter().filter(|r| r.tool == tool && DEVICE_SCENARIOS.contains(&r.scenario.as_str())).collect() };
    let scenario_secs = |tool: &str, s: &str| -> Option<f64> {
        let rs = of(tool, s);
        let ok: Vec<f64> = rs.iter().filter(|r| r.passed).map(|r| r.wall_ms).collect();
        if rs.is_empty() || ok.len() * 2 < rs.len() {
            return None;
        }
        median(&ok).map(|ms| ms / 1000.0)
    };

    let mut out = vec![
        metric("install-size", "Install size", "Start-up", "MB", Better::Lower, tools, |t| {
            statics.get(t).and_then(|s| s.install_bytes).map(|b| b / 1_000_000.0)
        }),
        metric("cli-startup", "CLI start (--version)", "Start-up", "ms", Better::Lower, tools, |t| {
            median(&of(t, "version").iter().map(|r| r.wall_ms).collect::<Vec<_>>())
        }),
        metric("error-detection", "Invalid flow reported", "Start-up", "ms", Better::Lower, tools, |t| {
            median(&of(t, "invalid").iter().filter(|r| r.passed).map(|r| r.wall_ms).collect::<Vec<_>>())
        }),
        metric("first-command", "One-command flow (connect + run)", "Start-up", "s", Better::Lower, tools, |t| {
            median(&of(t, "trivial").iter().filter(|r| r.passed).map(|r| r.wall_ms).collect::<Vec<_>>()).map(|ms| ms / 1000.0)
        }),
    ];
    for s in DEVICE_SCENARIOS {
        out.push(metric(&format!("exec-{s}"), scenario_label(s), "Execution", "s", Better::Lower, tools, |t| scenario_secs(t, s)));
    }
    out.push(metric("exec-total", "Whole suite (sum of medians)", "Execution", "s", Better::Lower, tools, |t| {
        DEVICE_SCENARIOS.iter().map(|s| scenario_secs(t, s)).sum::<Option<f64>>()
    }));
    out.push(metric("host-memory", "Peak host memory", "Resources", "MB", Better::Lower, tools, |t| {
        median(&device_runs(t).iter().filter_map(|r| r.peak_rss_kb).collect::<Vec<_>>()).map(|kb| kb / 1024.0)
    }));
    out.push(metric("host-cpu", "Host CPU time, whole suite", "Resources", "s", Better::Lower, tools, |t| {
        DEVICE_SCENARIOS
            .iter()
            .map(|s| median(&of(t, s).iter().filter_map(|r| r.cpu_ms).collect::<Vec<_>>()))
            .sum::<Option<f64>>()
            .map(|ms| ms / 1000.0)
    }));
    out.push(metric("agent-memory", "Peak device agent memory", "Resources", "MB", Better::Lower, tools, |t| {
        median(&device_runs(t).iter().filter_map(|r| r.agent_rss_kb).collect::<Vec<_>>()).map(|kb| kb / 1024.0)
    }));
    out.push(metric("pass-rate", "Runs passed", "Reliability", "%", Better::Higher, tools, |t| {
        let rs = device_runs(t);
        (!rs.is_empty()).then(|| rs.iter().filter(|r| r.passed).count() as f64 * 100.0 / rs.len() as f64)
    }));
    out.push(metric("consistency", "Run-to-run variation", "Reliability", "% CV", Better::Lower, tools, |t| {
        let cvs: Vec<f64> = DEVICE_SCENARIOS
            .iter()
            .filter_map(|s| {
                let ok: Vec<f64> = of(t, s).iter().filter(|r| r.passed).map(|r| r.wall_ms).collect();
                (ok.len() >= 3).then(|| cv(&ok)).flatten()
            })
            .collect();
        mean(&cvs).map(|c| c * 100.0)
    }));
    out
}
