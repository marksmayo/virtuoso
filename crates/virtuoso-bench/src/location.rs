//! Play services' location, waited for before an Android field's runs.
//!
//! The bench app asks Play services' fused location client for its fix (app/device's
//! "Location" check), not the platform's location service. After a cold boot on a slow emulator
//! (WHPX on GitHub's Windows runners: round 4's run 45939978769), Play services' fused provider
//! is not serving for minutes: `dumpsys location` lists it `enabled=false, allowed=false,
//! connected=false`, and any tool that reaches that step then gets no location. The harness
//! waits for it once, before the first run of any tool, so every tool starts on the same device.

use std::path::Path;
use std::time::{Duration, Instant};

/// Play services' fused provider as one `dumpsys location` shows it.
#[derive(Debug, PartialEq, Eq)]
pub enum FusedState {
    /// Enabled, allowed and connected to Play services' FusedLocationService.
    Ready,
    /// Listed but not serving, or no provider list at all: why, from its fields.
    NotReady(String),
    /// The providers are listed and fused is not among them: no Play services to wait for.
    Absent,
}

/// The fused provider's state from `dumpsys location`'s text: its own block (`fused provider:`
/// under "Location Providers:"), not the passive provider's fused listener or the event log.
pub fn fused_state(dumpsys: &str) -> FusedState {
    if !dumpsys.lines().any(|l| l.trim() == "Location Providers:") {
        return FusedState::NotReady("no location providers listed".into());
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let mut lines = dumpsys.lines();
    let Some(header) = lines.find(|l| {
        let t = l.trim();
        t.starts_with("fused provider") && t.ends_with(':')
    }) else {
        return FusedState::Absent;
    };
    let depth = indent(header);
    let (mut enabled, mut allowed, mut connected) = ("?", "?", "?");
    for line in lines.take_while(|l| !l.trim().is_empty() && indent(l) > depth) {
        match line.trim().split_once('=') {
            Some(("enabled", v)) => enabled = v,
            Some(("allowed", v)) => allowed = v,
            Some(("connected", v)) => connected = v,
            _ => {}
        }
    }
    if [enabled, allowed, connected].iter().all(|v| *v == "true") {
        FusedState::Ready
    } else {
        FusedState::NotReady(format!("enabled={enabled}, allowed={allowed}, connected={connected}"))
    }
}

/// How long to wait, how long the provider must stay ready to count, and how often to look.
#[derive(Debug, Clone, Copy)]
pub struct Wait {
    pub timeout: Duration,
    pub hold: Duration,
    pub poll: Duration,
}

impl Wait {
    /// A field's: up to 10 minutes (Play services came up about 9 minutes after a WHPX boot),
    /// ready for a whole minute (after boot it came up for 27 s and went down for 8 minutes),
    /// looked at every 5 s.
    pub fn field() -> Self {
        Wait { timeout: Duration::from_secs(600), hold: Duration::from_secs(60), poll: Duration::from_secs(5) }
    }
}

/// How the wait ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Ready and held, this long after the wait began.
    Ready { after: Duration },
    /// Not ready (or not held) by the timeout: the runs go ahead anyway.
    TimedOut { after: Duration, last: String },
    /// No fused provider on the device: nothing waited for.
    Absent,
}

const WHAT: &str = "Play services location";

/// Waits until the fused provider has stayed ready for `w.hold`, or `w.timeout` passes. `read`
/// is one `dumpsys location`, `now` the time since the wait began, `sleep` waits; `log` gets the
/// start, each change of state and the end.
pub fn wait_for_fused(w: &Wait, read: &dyn Fn() -> String, now: &dyn Fn() -> Duration, sleep: &dyn Fn(Duration), log: &dyn Fn(&str)) -> Outcome {
    let start = now();
    let mut ready_since: Option<Duration> = None;
    let mut last = String::new();
    let mut first = true;
    loop {
        let t = now() - start;
        let state = fused_state(&read());
        if first {
            first = false;
            if state == FusedState::Absent {
                log(&format!("{WHAT}: no fused provider on the device (no Play services): not waiting"));
                return Outcome::Absent;
            }
            log(&format!("{WHAT}: waiting up to {} s for the fused provider to be ready for {} s", w.timeout.as_secs(), w.hold.as_secs()));
        }
        match state {
            FusedState::Ready => {
                let since = *ready_since.get_or_insert_with(|| {
                    log(&format!("{WHAT}: fused provider ready at {} s", t.as_secs()));
                    t
                });
                if t - since >= w.hold {
                    log(&format!("{WHAT}: fused provider ready after {} s", t.as_secs()));
                    return Outcome::Ready { after: t };
                }
                last = format!("ready for {} s of the {} s hold", (t - since).as_secs(), w.hold.as_secs());
            }
            not_ready => {
                let why = match not_ready {
                    FusedState::NotReady(why) => why,
                    _ => "no fused provider listed".into(),
                };
                ready_since = None;
                if last != why {
                    log(&format!("{WHAT}: fused provider not ready at {} s ({why})", t.as_secs()));
                }
                last = why;
            }
        }
        if t >= w.timeout {
            log(&format!("{WHAT}: fused provider not ready after {} s ({last}): running anyway", t.as_secs()));
            return Outcome::TimedOut { after: t, last };
        }
        sleep(w.poll);
    }
}

/// `wait_for_fused` on the device `serial`, through the adb at `adb`, on the wall clock. A
/// device adb cannot reach reads as no provider list, so the wait runs to its timeout and the
/// runs go ahead.
pub fn wait_with_adb(adb: &Path, serial: &str, w: &Wait, log: &dyn Fn(&str)) -> Outcome {
    let read = || std::process::Command::new(adb).args(["-s", serial, "shell", "dumpsys", "location"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let began = Instant::now();
    wait_for_fused(w, &read, &|| began.elapsed(), &|d| std::thread::sleep(d), log)
}
