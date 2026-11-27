//! CPU time from /proc (Linux).

use std::path::Path;

/// CPU time used so far, in ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sample {
    pub own_ms: u64,
    pub tree_ms: u64,
}

pub fn sample_at(_proc: &Path, _pid: u32, _ticks_per_second: u64) -> Option<Sample> {
    None
}

pub fn sample() -> Option<Sample> {
    None
}
