//! `virtuoso latency`: how long agent round trips take on a device.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
}

impl Summary {
    /// Nearest-rank percentiles of `samples`; none for no samples.
    pub fn of(samples: &[Duration]) -> Option<Summary> {
        let mut s = samples.to_vec();
        s.sort_unstable();
        let rank = |p: f64| s[((p * s.len() as f64).ceil() as usize).clamp(1, s.len()) - 1];
        (!s.is_empty()).then(|| Summary { p50: rank(0.50), p95: rank(0.95), max: s[s.len() - 1] })
    }
}
