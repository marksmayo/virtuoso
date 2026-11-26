//! Where a run's time goes outside its flows: a launched browser's start (virtuoso's own start,
//! Chrome spawned, listening, connected, ready) and its stop, and the Windows profile sweep, as
//! the `phases:` and `sweep:` lines a run prints once its device is closed.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Marks the moment virtuoso started (the first thing `main` does): the `start` phase is from
/// here. Later calls keep the first.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

/// When `mark_process_start` was called, if it was.
pub fn process_start() -> Option<Instant> {
    PROCESS_START.get().copied()
}

/// A launched browser's start, as it went.
#[derive(Debug, Clone, Copy)]
pub struct Launch {
    /// The launch began (before its profile folder is made).
    pub began: Instant,
    /// Chrome's process spawned.
    pub spawned: Instant,
    /// Its port file seen: Chrome listening.
    pub listening: Instant,
    /// Its WebSocket connected.
    pub connected: Instant,
    /// How many profile folder names `fresh_profile` tried.
    pub profile_tries: u32,
}

/// A launched browser's stop.
#[derive(Debug, Clone, Copy)]
pub struct Close {
    /// From the stop's start until Chrome was ended (and, off Windows, waited for).
    pub chrome_exit: Duration,
    /// Removing its profile; `None` when it was left for the next launch's sweep (Windows).
    pub profile_removed: Option<Duration>,
}

/// What the Windows profile sweep found and did, so far.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sweep {
    /// `virtuoso-chrome-*` folders in the temp folder.
    pub found: u32,
    /// Of those, the ones removed (at the first try or a retry).
    pub removed: u32,
    /// Those that could not be removed at the first try (their files still held).
    pub held: u32,
    /// Rounds of trying the held ones again.
    pub retries: u32,
    /// How long it took, once it is done.
    pub took: Option<Duration>,
}

/// A launched browser's start and stop, and its launch's sweep (Windows).
#[derive(Debug, Clone, Copy)]
pub struct Phases {
    pub launch: Launch,
    pub close: Close,
    pub sweep: Option<Sweep>,
    /// When the sweep began (for its time so far while it still runs).
    pub sweep_began: Option<Instant>,
}

impl Phases {
    /// The `phases:` line: `process_start` (see `mark_process_start`) to the launch, the launch's
    /// steps, `ready` (the device set up) and the close, `closed_in` in all; then a `sweep:` line
    /// when there was a sweep, its time so far measured to `now` if it still runs.
    pub fn lines(&self, process_start: Option<Instant>, ready: Instant, closed_in: Duration, now: Instant) -> String {
        let l = &self.launch;
        let mut s = String::from("phases: ");
        if let Some(p) = process_start {
            s += &format!("start {}, ", Ms(l.began.saturating_duration_since(p)));
        }
        s += &format!(
            "spawn {}, chrome listening {}, connected {}, ready {}, profile tries {} | close {} (chrome exit {}, ",
            Ms(l.spawned.saturating_duration_since(l.began)),
            Ms(l.listening.saturating_duration_since(l.spawned)),
            Ms(l.connected.saturating_duration_since(l.listening)),
            Ms(ready.saturating_duration_since(l.connected)),
            l.profile_tries,
            Ms(closed_in),
            Ms(self.close.chrome_exit),
        );
        s += &match self.close.profile_removed {
            Some(d) => format!("profile removed {})", Ms(d)),
            None => "profile left)".to_string(),
        };
        if let Some(w) = &self.sweep {
            s += &format!("\nsweep: found {}, removed {}, held {}, retries {}, ", w.found, w.removed, w.held, w.retries);
            s += &match w.took {
                Some(d) => format!("took {}", Ms(d)),
                None => format!("still running after {}", Ms(self.sweep_began.map(|b| now.saturating_duration_since(b)).unwrap_or_default())),
            };
        }
        s
    }
}

/// A duration in milliseconds to a tenth.
struct Ms(Duration);

impl std::fmt::Display for Ms {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:.1} ms", self.0.as_secs_f64() * 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1000.0)
    }

    fn launch(at: Instant) -> Launch {
        Launch { began: at + ms(12.3), spawned: at + ms(15.4), listening: at + ms(655.5), connected: at + ms(656.7), profile_tries: 1 }
    }

    #[test]
    fn the_phases_line_gives_each_phase_in_ms_to_a_tenth_and_the_close() {
        let t = Instant::now();
        let p = Phases { launch: launch(t), close: Close { chrome_exit: ms(30.0), profile_removed: Some(ms(5.0)) }, sweep: None, sweep_began: None };
        assert_eq!(
            p.lines(Some(t), t + ms(664.9), ms(35.04), t + ms(2000.0)),
            "phases: start 12.3 ms, spawn 3.1 ms, chrome listening 640.1 ms, connected 1.2 ms, ready 8.2 ms, profile tries 1 | close 35.0 ms (chrome exit 30.0 ms, profile removed 5.0 ms)"
        );
    }

    #[test]
    fn without_virtuosos_own_start_the_phases_line_begins_at_the_launch() {
        let t = Instant::now();
        let p = Phases { launch: launch(t), close: Close { chrome_exit: ms(0.2), profile_removed: None }, sweep: None, sweep_began: None };
        assert_eq!(
            p.lines(None, t + ms(664.9), ms(0.4), t),
            "phases: spawn 3.1 ms, chrome listening 640.1 ms, connected 1.2 ms, ready 8.2 ms, profile tries 1 | close 0.4 ms (chrome exit 0.2 ms, profile left)"
        );
    }

    #[test]
    fn a_sweep_is_a_line_of_its_own_with_its_time_or_its_time_so_far() {
        let t = Instant::now();
        let mut p = Phases {
            launch: Launch { profile_tries: 2, ..launch(t) },
            close: Close { chrome_exit: ms(0.2), profile_removed: None },
            sweep: Some(Sweep { found: 3, removed: 2, held: 1, retries: 4, took: Some(ms(120.5)) }),
            sweep_began: Some(t + ms(12.5)),
        };
        let lines = p.lines(Some(t), t + ms(664.9), ms(0.4), t + ms(3000.0));
        let second = lines.lines().nth(1).unwrap();
        assert!(lines.lines().next().unwrap().contains("profile tries 2"), "{lines}");
        assert_eq!(second, "sweep: found 3, removed 2, held 1, retries 4, took 120.5 ms");
        p.sweep = Some(Sweep { took: None, ..p.sweep.unwrap() });
        let lines = p.lines(Some(t), t + ms(664.9), ms(0.4), t + ms(3000.0));
        assert_eq!(lines.lines().nth(1).unwrap(), "sweep: found 3, removed 2, held 1, retries 4, still running after 2987.5 ms");
    }

    #[test]
    fn virtuosos_start_is_marked_once() {
        mark_process_start();
        let first = process_start().unwrap();
        mark_process_start();
        assert_eq!(process_start(), Some(first));
    }
}
