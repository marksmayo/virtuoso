//! Medals: each metric awards gold, silver and bronze by rank, Olympic style. Values within
//! TIE_TOLERANCE of each other tie and share a medal; the rank after a tie is skipped. A tool
//! with no value for a metric (unsupported, or every run failed) gets no medal for it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Results closer than this (relative) are a tie: run-to-run noise is about that size.
pub const TIE_TOLERANCE: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Better {
    Lower,
    Higher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Medal {
    Gold,
    Silver,
    Bronze,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricResult {
    pub key: String,
    pub label: String,
    /// Which part of the table the metric belongs to ("Start-up", "Execution", "Features"...).
    pub group: String,
    pub unit: String,
    pub better: Better,
    pub values: BTreeMap<String, Option<f64>>,
}

fn ties(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs());
    scale == 0.0 || (a - b).abs() / scale <= TIE_TOLERANCE
}

/// The medal each tool wins for one metric.
pub fn rank(m: &MetricResult) -> BTreeMap<String, Option<Medal>> {
    let mut scored: Vec<(&String, f64)> = m.values.iter().filter_map(|(t, v)| v.map(|v| (t, v))).collect();
    scored.sort_by(|a, b| {
        let o = a.1.partial_cmp(&b.1).unwrap();
        if m.better == Better::Lower {
            o
        } else {
            o.reverse()
        }
    });
    let mut out: BTreeMap<String, Option<Medal>> = m.values.keys().map(|t| (t.clone(), None)).collect();
    let mut place = 0;
    for i in 0..scored.len() {
        // A tie with the one before keeps its place; otherwise the place is the position.
        if i == 0 || !ties(scored[i].1, scored[i - 1].1) {
            place = i;
        }
        let medal = match place {
            0 => Some(Medal::Gold),
            1 => Some(Medal::Silver),
            2 => Some(Medal::Bronze),
            _ => None,
        };
        out.insert(scored[i].0.clone(), medal);
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableRow {
    pub tool: String,
    pub gold: u32,
    pub silver: u32,
    pub bronze: u32,
}

impl TableRow {
    pub fn total(&self) -> u32 {
        self.gold + self.silver + self.bronze
    }
}

/// Medal counts per tool, ordered by golds, then silvers, then bronzes.
pub fn medal_table(metrics: &[MetricResult], tools: &[String]) -> Vec<TableRow> {
    let mut rows: Vec<TableRow> = tools.iter().map(|t| TableRow { tool: t.clone(), gold: 0, silver: 0, bronze: 0 }).collect();
    for m in metrics {
        for (tool, medal) in rank(m) {
            let Some(row) = rows.iter_mut().find(|r| r.tool == tool) else { continue };
            match medal {
                Some(Medal::Gold) => row.gold += 1,
                Some(Medal::Silver) => row.silver += 1,
                Some(Medal::Bronze) => row.bronze += 1,
                None => {}
            }
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse((r.gold, r.silver, r.bronze)));
    rows
}
