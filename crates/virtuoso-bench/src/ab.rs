//! An A/B job's reading (web-bench's `ab_base` mode: two virtuoso builds taking turns run by run
//! on one runner). Per flow and for the suite: each build's median wall time, CPU and peak
//! memory over its passing runs, and in how many rounds both passed the PR build did better.
//! `virtuoso-bench ab <results.json>... [--pr <tool>] [--base <tool>]`.

use crate::metrics::RunRecord;
use crate::orchestrate::{Results, AB_BASE};
use crate::stats::median;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};

/// One build's figures for a flow (medians of its passing runs), or the suite's (wall and CPU
/// the flows' medians summed, memory the highest flow's).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Side {
    pub runs: usize,
    pub passed: usize,
    pub wall: Option<f64>,
    pub cpu: Option<f64>,
    pub rss_mb: Option<f64>,
}

/// A flow's (or the suite's) A/B.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub scenario: String,
    pub pr: Side,
    pub base: Side,
    /// Rounds both builds passed (the suite's: rounds with any flow both passed).
    pub pairs: usize,
    /// Of those, the rounds the PR build took less wall time, CPU, peak memory.
    pub wall_wins: usize,
    pub cpu_wins: usize,
    pub rss_wins: usize,
}

/// A passing run's three figures.
#[derive(Clone, Copy)]
struct Fig {
    wall: f64,
    cpu: Option<f64>,
    rss_mb: Option<f64>,
}

fn fig(r: &RunRecord) -> Fig {
    Fig { wall: r.wall_ms, cpu: r.cpu_ms, rss_mb: r.peak_rss_kb.map(|k| k / 1024.0) }
}

fn side(rs: &[&RunRecord]) -> Side {
    let ok: Vec<Fig> = rs.iter().filter(|r| r.passed).map(|r| fig(r)).collect();
    Side {
        runs: rs.len(),
        passed: ok.len(),
        wall: median(&ok.iter().map(|f| f.wall).collect::<Vec<_>>()),
        cpu: median(&ok.iter().filter_map(|f| f.cpu).collect::<Vec<_>>()),
        rss_mb: median(&ok.iter().filter_map(|f| f.rss_mb).collect::<Vec<_>>()),
    }
}

fn less(a: Option<f64>, b: Option<f64>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a < b)
}

fn sum(xs: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    xs.collect::<Option<Vec<f64>>>().filter(|v| !v.is_empty()).map(|v| v.iter().sum())
}

fn max(xs: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    xs.collect::<Option<Vec<f64>>>().and_then(|v| v.into_iter().reduce(f64::max))
}

/// The A/B of `pr` against `base`: a row per flow both ran (in name order), then the suite's
/// (over the flows both builds have figures for).
pub fn compare(runs: &[RunRecord], pr: &str, base: &str) -> Vec<Row> {
    let ours = |r: &&RunRecord| (r.tool == pr || r.tool == base) && r.scenario != "version";
    let scenarios: BTreeSet<&str> = runs.iter().filter(ours).map(|r| r.scenario.as_str()).collect();
    let mut rows = Vec::new();
    // Per round, each build's passing figures per flow both passed in it: the suite's rounds.
    let mut rounds: BTreeMap<usize, Vec<(Fig, Fig)>> = BTreeMap::new();
    for s in &scenarios {
        let of = |tool: &str| runs.iter().filter(|r| r.tool == tool && r.scenario == *s).collect::<Vec<_>>();
        let (p, b) = (of(pr), of(base));
        let mut row = Row { scenario: s.to_string(), pr: side(&p), base: side(&b), ..Default::default() };
        for rp in p.iter().filter(|r| r.passed) {
            let Some(rb) = b.iter().find(|r| r.passed && r.iteration == rp.iteration) else { continue };
            let (fp, fb) = (fig(rp), fig(rb));
            row.pairs += 1;
            row.wall_wins += usize::from(fp.wall < fb.wall);
            row.cpu_wins += usize::from(less(fp.cpu, fb.cpu));
            row.rss_wins += usize::from(less(fp.rss_mb, fb.rss_mb));
            rounds.entry(rp.iteration).or_default().push((fp, fb));
        }
        rows.push(row);
    }
    let both = |f: fn(&Side) -> Option<f64>| rows.iter().filter(|r| f(&r.pr).is_some() && f(&r.base).is_some()).collect::<Vec<_>>();
    let mut suite = Row { scenario: "suite".into(), ..Default::default() };
    let wall_rows = both(|s| s.wall);
    let cpu_rows = both(|s| s.cpu);
    let rss_rows = both(|s| s.rss_mb);
    suite.pr = Side {
        runs: rows.iter().map(|r| r.pr.runs).sum(),
        passed: rows.iter().map(|r| r.pr.passed).sum(),
        wall: sum(wall_rows.iter().map(|r| r.pr.wall)),
        cpu: sum(cpu_rows.iter().map(|r| r.pr.cpu)),
        rss_mb: max(rss_rows.iter().map(|r| r.pr.rss_mb)),
    };
    suite.base = Side {
        runs: rows.iter().map(|r| r.base.runs).sum(),
        passed: rows.iter().map(|r| r.base.passed).sum(),
        wall: sum(wall_rows.iter().map(|r| r.base.wall)),
        cpu: sum(cpu_rows.iter().map(|r| r.base.cpu)),
        rss_mb: max(rss_rows.iter().map(|r| r.base.rss_mb)),
    };
    for pairs in rounds.values() {
        suite.pairs += 1;
        let wall: (f64, f64) = pairs.iter().fold((0.0, 0.0), |(p, b), (fp, fb)| (p + fp.wall, b + fb.wall));
        suite.wall_wins += usize::from(wall.0 < wall.1);
        suite.cpu_wins += usize::from(less(sum(pairs.iter().map(|(p, _)| p.cpu)), sum(pairs.iter().map(|(_, b)| b.cpu))));
        suite.rss_wins += usize::from(less(max(pairs.iter().map(|(p, _)| p.rss_mb)), max(pairs.iter().map(|(_, b)| b.rss_mb))));
    }
    rows.push(suite);
    rows
}

/// A figure, its decimals, or `-`.
fn num(x: Option<f64>, decimals: usize) -> String {
    x.map_or("-".into(), |x| format!("{x:.decimals$}"))
}

/// The PR build's change on the base, in percent.
fn change(pr: Option<f64>, base: Option<f64>) -> String {
    match (pr, base) {
        (Some(p), Some(b)) if b > 0.0 => format!("{:+.1}%", (p / b - 1.0) * 100.0),
        _ => "-".into(),
    }
}

/// The rows as a plain-text table: for each measure the base's median, the PR's, the change,
/// and the rounds the PR build did better out of the rounds both passed.
pub fn render(rows: &[Row], pr: &str, base: &str) -> String {
    let mut out = format!("A/B: PR = {pr}, base = {base}. Medians of passing runs; \"better\" = rounds the PR build did better, of the rounds both passed.\n");
    out += &format!(
        "{:<28} {:>9} {:>9} {:>7} {:>6}   {:>9} {:>9} {:>7} {:>6}   {:>8} {:>8} {:>7} {:>6}   {:>6} {:>6}\n",
        "flow", "wall base", "wall PR", "change", "better", "CPU base", "CPU PR", "change", "better", "MB base", "MB PR", "change", "better", "passed", "PR"
    );
    for r in rows {
        let better = |wins: usize| format!("{wins}/{}", r.pairs);
        out += &format!(
            "{:<28} {:>9} {:>9} {:>7} {:>6}   {:>9} {:>9} {:>7} {:>6}   {:>8} {:>8} {:>7} {:>6}   {:>6} {:>6}\n",
            r.scenario,
            num(r.base.wall, 0),
            num(r.pr.wall, 0),
            change(r.pr.wall, r.base.wall),
            better(r.wall_wins),
            num(r.base.cpu, 0),
            num(r.pr.cpu, 0),
            change(r.pr.cpu, r.base.cpu),
            better(r.cpu_wins),
            num(r.base.rss_mb, 1),
            num(r.pr.rss_mb, 1),
            change(r.pr.rss_mb, r.base.rss_mb),
            better(r.rss_wins),
            format!("{}/{}", r.base.passed, r.base.runs),
            format!("{}/{}", r.pr.passed, r.pr.runs),
        );
    }
    out
}

/// `virtuoso-bench ab <results.json>... [--pr <tool>] [--base <tool>]`: the A/B table of the
/// results files (one per job), the PR build `virtuoso` and the base `AB_BASE` unless named.
pub fn command(args: &[String]) -> Result<String> {
    let usage = "virtuoso-bench ab <results.json>... [--pr <tool>] [--base <tool>]";
    let (mut pr, mut base, mut files) = ("virtuoso".to_string(), AB_BASE.to_string(), Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pr" => pr = it.next().context(usage)?.clone(),
            "--base" => base = it.next().context(usage)?.clone(),
            f => files.push(f),
        }
    }
    if files.is_empty() {
        bail!("{usage}");
    }
    let mut runs = Vec::new();
    for f in files {
        let r: Results = serde_json::from_str(&std::fs::read_to_string(f).with_context(|| format!("reading {f}"))?).with_context(|| format!("parsing {f}"))?;
        runs.extend(r.runs);
    }
    Ok(render(&compare(&runs, &pr, &base), &pr, &base))
}
