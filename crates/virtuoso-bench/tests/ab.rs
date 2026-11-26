//! The A/B reading of one benchmark job that ran two virtuoso builds in turn on one runner (the
//! web field's `ab_base` mode): per flow and for the suite, wall time, CPU and peak memory of
//! the PR build against the base, and in how many rounds the PR build did better.

use std::collections::BTreeMap;
use virtuoso_bench::ab::{command, compare};
use virtuoso_bench::metrics::RunRecord;
use virtuoso_bench::orchestrate::Results;

fn run(tool: &str, scenario: &str, iteration: usize, wall: f64, cpu: f64, rss_mb: f64, passed: bool) -> RunRecord {
    RunRecord { tool: tool.into(), scenario: scenario.into(), iteration, wall_ms: wall, cpu_ms: Some(cpu), peak_rss_kb: Some(rss_mb * 1024.0), passed, ..Default::default() }
}

/// Two flows, three rounds: the PR build quicker and lighter on web/a; on web/b slower, and its
/// second round failed.
fn runs() -> Vec<RunRecord> {
    let mut v = Vec::new();
    for (i, (wall, cpu)) in [(900.0, 400.0), (950.0, 410.0), (1000.0, 420.0)].into_iter().enumerate() {
        v.push(run("virtuoso", "web/a", i, wall, cpu, 200.0, true));
        v.push(run("virtuoso-base", "web/a", i, 1000.0, 500.0, 210.0, true));
    }
    for i in 0..3 {
        v.push(run("virtuoso", "web/b", i, 2100.0, 900.0, 220.0, i != 1));
        v.push(run("virtuoso-base", "web/b", i, 2000.0, 800.0, 220.0, true));
    }
    // Another tool's runs, and the version runs, are not the A/B's.
    v.push(run("playwright", "web/a", 0, 1.0, 1.0, 1.0, true));
    v.push(run("virtuoso", "version", 0, 5.0, 5.0, 5.0, true));
    v
}

#[test]
fn each_flow_compares_the_pr_build_with_the_base_round_by_round() {
    let rows = compare(&runs(), "virtuoso", "virtuoso-base");
    let names: Vec<&str> = rows.iter().map(|r| r.scenario.as_str()).collect();
    assert_eq!(names, ["web/a", "web/b", "suite"]);

    let a = &rows[0];
    assert_eq!((a.pr.wall, a.base.wall), (Some(950.0), Some(1000.0)));
    assert_eq!((a.pr.cpu, a.base.cpu), (Some(410.0), Some(500.0)));
    assert_eq!((a.pr.rss_mb, a.base.rss_mb), (Some(200.0), Some(210.0)));
    assert_eq!((a.pr.passed, a.pr.runs, a.base.passed, a.base.runs), (3, 3, 3, 3));
    // Every round both passed is a pair; the PR build did better in each.
    assert_eq!((a.pairs, a.wall_wins, a.cpu_wins, a.rss_wins), (3, 3, 3, 3));

    let b = &rows[1];
    assert_eq!((b.pr.passed, b.pr.runs), (2, 3));
    // Round 1 is no pair: the PR build failed it.
    assert_eq!((b.pairs, b.wall_wins, b.cpu_wins, b.rss_wins), (2, 0, 0, 0));
}

#[test]
fn the_suite_row_totals_each_round_over_the_flows_both_builds_passed_in_it() {
    let rows = compare(&runs(), "virtuoso", "virtuoso-base");
    let suite = rows.last().unwrap();
    // Wall and CPU: the flows' medians summed; memory: the highest flow's.
    assert_eq!((suite.pr.wall, suite.base.wall), (Some(950.0 + 2100.0), Some(1000.0 + 2000.0)));
    assert_eq!((suite.pr.cpu, suite.base.cpu), (Some(410.0 + 900.0), Some(500.0 + 800.0)));
    assert_eq!((suite.pr.rss_mb, suite.base.rss_mb), (Some(220.0), Some(220.0)));
    // A round's totals over the flows both passed in it: round 0 PR 3000 vs 3000 (not better),
    // round 1 only web/a (950 vs 1000), round 2 3100 vs 3000.
    assert_eq!((suite.pairs, suite.wall_wins), (3, 1));
    // CPU: 1300 vs 1300, 410 vs 500, 1320 vs 1300.
    assert_eq!(suite.cpu_wins, 1);
}

#[test]
fn the_table_reads_the_results_files_and_says_which_build_is_which() {
    let d = tempfile::tempdir().unwrap();
    let all = runs();
    // One file per job (a group of flows each), as the workflow downloads them.
    let (a, b): (Vec<RunRecord>, Vec<RunRecord>) = all.into_iter().partition(|r| r.scenario != "web/b");
    let mut files = Vec::new();
    for (k, runs) in [a, b].into_iter().enumerate() {
        let r = Results { meta: BTreeMap::new(), tools: vec!["virtuoso".into(), "virtuoso-base".into()], statics: BTreeMap::new(), runs };
        let f = d.path().join(format!("results-{k}.json"));
        std::fs::write(&f, serde_json::to_string(&r).unwrap()).unwrap();
        files.push(f.display().to_string());
    }
    let table = command(&files).unwrap();
    assert!(table.contains("PR = virtuoso, base = virtuoso-base"), "{table}");
    let line = |name: &str| table.lines().find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("no {name} row: {table}")).to_string();
    let a = line("web/a");
    for want in ["1000", "950", "-5.0%", "3/3", "500", "410", "-18.0%", "210.0", "200.0", "-4.8%"] {
        assert!(a.contains(want), "{want} in {a}");
    }
    assert!(line("web/b").contains("2/3"), "{table}");
    assert!(line("suite").contains("1/3"), "{table}");

    // Other builds by name.
    let mut args = files.clone();
    args.extend(["--pr".to_string(), "virtuoso-base".to_string(), "--base".to_string(), "virtuoso".to_string()]);
    assert!(command(&args).unwrap().contains("PR = virtuoso-base, base = virtuoso"));
    assert!(command(&[]).is_err(), "no results file");
    assert!(command(&[d.path().join("missing.json").display().to_string()]).is_err());
}
