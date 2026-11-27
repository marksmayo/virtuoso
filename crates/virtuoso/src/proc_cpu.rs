//! CPU time from /proc (Linux), for VIRTUOSO_TIMINGS' per-step line: what this process has used,
//! and what its whole tree has (virtuoso and the browser it started, every process under it), as
//! the benchmark counts a run's CPU.

use std::collections::HashMap;
use std::path::Path;

/// CPU time used so far, user and system, in ms: this process's own, and its tree's (its own,
/// every live descendant's, and what each of them reaped of its children).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sample {
    pub own_ms: u64,
    pub tree_ms: u64,
}

impl Sample {
    /// The time used from `before` to this sample.
    pub fn since(self, before: Sample) -> Sample {
        Sample { own_ms: self.own_ms.saturating_sub(before.own_ms), tree_ms: self.tree_ms.saturating_sub(before.tree_ms) }
    }
}

/// A `/proc/<pid>/stat`'s parent and CPU ticks: its own (utime + stime) and its reaped
/// children's (cutime + cstime). The name, in parentheses, may hold spaces and parentheses:
/// the fields are counted from the last `)`.
fn parse_stat(text: &str) -> Option<(u32, u64, u64)> {
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // After the name: state (field 3), ppid (4), ..., utime (14), stime, cutime, cstime (17).
    let n = |k: usize| f.get(k - 3)?.parse::<u64>().ok();
    let own = n(14)? + n(15)?;
    let children = n(16)? + n(17)?;
    Some((u32::try_from(n(4)?).ok()?, own, children))
}

/// `pid`'s sample from the /proc at `proc`, its clock `ticks_per_second`; `None` when it has
/// no readable stat there.
pub fn sample_at(proc: &Path, pid: u32, ticks_per_second: u64) -> Option<Sample> {
    let stat = |p: u32| std::fs::read_to_string(proc.join(p.to_string()).join("stat")).ok().and_then(|t| parse_stat(&t));
    let (_, own, own_children) = stat(pid)?;
    // Every process's parent and ticks, to walk down from `pid`.
    let mut children: HashMap<u32, Vec<(u32, u64)>> = HashMap::new();
    for entry in std::fs::read_dir(proc).ok()?.flatten() {
        let Some(p) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        if let Some((parent, ticks, reaped)) = stat(p).filter(|&(parent, _, _)| parent != p) {
            children.entry(parent).or_default().push((p, ticks + reaped));
        }
    }
    let mut tree = own + own_children;
    let mut todo = vec![pid];
    while let Some(p) = todo.pop() {
        for &(child, ticks) in children.get(&p).into_iter().flatten() {
            tree += ticks;
            todo.push(child);
        }
    }
    let ms = |ticks: u64| ticks * 1000 / ticks_per_second.max(1);
    Some(Sample { own_ms: ms(own), tree_ms: ms(tree) })
}

/// This process's sample, from /proc; `None` where there is none (not Linux).
#[cfg(target_os = "linux")]
pub fn sample() -> Option<Sample> {
    // SAFETY: sysconf reads a constant of the system.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    sample_at(Path::new("/proc"), std::process::id(), u64::try_from(ticks).ok().filter(|&t| t > 0)?)
}

#[cfg(not(target_os = "linux"))]
pub fn sample() -> Option<Sample> {
    None
}
