//! An A/B job's reading (web-bench's `ab_base` mode).

use crate::metrics::RunRecord;
use anyhow::Result;

/// One build's figures for a flow.
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
    pub pairs: usize,
    pub wall_wins: usize,
    pub cpu_wins: usize,
    pub rss_wins: usize,
}

pub fn compare(_runs: &[RunRecord], _pr: &str, _base: &str) -> Vec<Row> {
    Vec::new()
}

pub fn command(_args: &[String]) -> Result<String> {
    Ok(String::new())
}
