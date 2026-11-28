//! Play services' location readiness, read from `dumpsys location`, and the wait for it made
//! before an Android field's runs. The excerpts are from round 4's Windows run 45939978769
//! (app/device's device.txt dumps).

use std::cell::{Cell, RefCell};
use std::time::Duration;
use virtuoso_bench::location::{fused_state, wait_for_fused, FusedState, Outcome, Wait};

const HEAD: &str = "Location Manager State:
  User Info:
    current users: u[0]
  Location Settings:
    Location Setting: true
  Location Providers:
    passive provider:
      service: registered
      listeners:
        10146/com.google.android.gms[fused_location_provider]/68ED9C4F Request[PASSIVE, minUpdateInterval=0, hiddenFromAppOps, WorkSource{10146 com.google.android.gms}]
      enabled=true
      allowed=true
      identity=1000/android[LocationService]
";

const GPS_AND_LOG: &str = "    gps provider [mock]:
      service: ProviderRequest[OFF]
      enabled=true
      allowed=true
      identity=2000/android[LocationService]
  Historical Aggregate Location Provider Data:
    passive:
      10146/com.google.android.gms[fused_location_provider]: min/max interval = passive/passive, locations = 0
  Event Log:
    11-27 04:38:55.725: fused provider [u0] enabled
    11-27 04:39:22.225: fused provider [u0] disabled
";

/// 04:43, the native warm-up whose location wait failed: Play services' fused provider is off.
const NOT_CONNECTED: &str = "    fused provider:
      service: ProviderRequest[OFF]
      last location=null
      enabled=false
      allowed=false
      target service=10146/com.google.android.gms/com.google.android.location.fused.FusedLocationService@1
      connected=false
";

/// 04:47:52, a run that passed: the fused provider enabled, allowed and bound to Play services.
const READY: &str = "    fused provider:
      service: ProviderRequest[OFF]
      last location=Location[fused 45.508800,-73.554000 hAcc=100.0 et=+15m6s684ms mock]
      enabled=true
      allowed=true
      identity=10146/com.google.android.gms[fused_location_provider]
      properties=ProviderProperties[powerUsage=Low, accuracy=Fine, supports=[bearing,speed,altitude]]
      target service=10146/com.google.android.gms/com.google.android.location.fused.FusedLocationService@1
      connected=true
";

fn dump(fused: &str) -> String {
    format!("{HEAD}{fused}{GPS_AND_LOG}")
}

#[test]
fn a_disabled_disconnected_fused_provider_is_not_ready() {
    match fused_state(&dump(NOT_CONNECTED)) {
        FusedState::NotReady(why) => assert_eq!(why, "enabled=false, allowed=false, connected=false"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_enabled_allowed_connected_fused_provider_is_ready() {
    assert_eq!(fused_state(&dump(READY)), FusedState::Ready);
}

#[test]
fn enabled_but_not_yet_connected_is_not_ready() {
    let half = READY.replace("connected=true", "connected=false");
    assert_eq!(fused_state(&dump(&half)), FusedState::NotReady("enabled=true, allowed=true, connected=false".into()));
    let unallowed = READY.replace("allowed=true", "allowed=false");
    assert_eq!(fused_state(&dump(&unallowed)), FusedState::NotReady("enabled=true, allowed=false, connected=true".into()));
}

#[test]
fn a_fused_block_without_its_fields_is_not_ready() {
    // A dump cut short after the header: nothing says the provider is up.
    assert_eq!(fused_state(&format!("{HEAD}    fused provider:\n")), FusedState::NotReady("enabled=?, allowed=?, connected=?".into()));
}

#[test]
fn a_device_without_a_fused_provider_has_nothing_to_wait_for() {
    // No Play services (an AOSP image): the providers are listed, and fused is not among them.
    assert_eq!(fused_state(&dump("")), FusedState::Absent);
}

#[test]
fn no_dumpsys_output_is_not_ready() {
    // adb failed or the location service is not up yet: no provider list at all.
    assert_eq!(fused_state(""), FusedState::NotReady("no location providers listed".into()));
    assert_eq!(fused_state("Can't find service: location\n"), FusedState::NotReady("no location providers listed".into()));
}

#[test]
fn the_passive_providers_fused_listener_and_the_event_log_are_not_the_fused_provider() {
    // The excerpt's passive listener and event log name the fused provider; neither is its block.
    let text = dump("");
    assert!(text.contains("[fused_location_provider]") && text.contains("fused provider [u0] enabled"));
    assert_eq!(fused_state(&text), FusedState::Absent);
}

/// A fake device: each read returns the next dump (the last one again once they run out), and
/// the clock moves only when the wait sleeps.
struct Fake {
    dumps: Vec<String>,
    reads: Cell<usize>,
    now: Cell<Duration>,
    log: RefCell<Vec<String>>,
}

impl Fake {
    fn new(dumps: Vec<String>) -> Self {
        Fake { dumps, reads: Cell::new(0), now: Cell::new(Duration::ZERO), log: RefCell::new(vec![]) }
    }
    fn run(&self, w: &Wait) -> Outcome {
        let read = || {
            let i = self.reads.get();
            self.reads.set(i + 1);
            self.dumps[i.min(self.dumps.len() - 1)].clone()
        };
        let now = || self.now.get();
        let sleep = |d: Duration| self.now.set(self.now.get() + d);
        let log = |l: &str| self.log.borrow_mut().push(l.to_string());
        wait_for_fused(w, &read, &now, &sleep, &log)
    }
}

const SECS: fn(u64) -> Duration = Duration::from_secs;

fn wait() -> Wait {
    Wait { timeout: SECS(600), hold: SECS(60), poll: SECS(5) }
}

#[test]
fn the_wait_ends_once_the_provider_has_stayed_ready_for_the_hold() {
    // Not ready for the first 3 polls (0, 5, 10 s), ready from 15 s: ready since 15 s, held 60 s at 75 s.
    let mut dumps = vec![dump(NOT_CONNECTED); 3];
    dumps.push(dump(READY));
    let fake = Fake::new(dumps);
    assert_eq!(fake.run(&wait()), Outcome::Ready { after: SECS(75) });
    let log = fake.log.borrow();
    assert!(log.iter().any(|l| l.contains("not ready") && l.contains("enabled=false")), "{log:?}");
    assert!(log.last().unwrap().contains("ready after 75 s"), "{log:?}");
}

#[test]
fn a_provider_that_drops_again_restarts_the_hold() {
    // After boot the fused provider came up for 27 s and went down again for minutes
    // (04:38:55 to 04:39:22): ready at 0-25 s, down at 30 s, ready again from 35 s.
    let mut dumps = vec![dump(READY); 6];
    dumps.push(dump(NOT_CONNECTED));
    dumps.push(dump(READY));
    let fake = Fake::new(dumps);
    assert_eq!(fake.run(&wait()), Outcome::Ready { after: SECS(95) });
}

#[test]
fn an_already_ready_device_waits_only_the_hold() {
    let fake = Fake::new(vec![dump(READY)]);
    assert_eq!(fake.run(&wait()), Outcome::Ready { after: SECS(60) });
}

#[test]
fn the_wait_gives_up_at_its_timeout_and_says_why() {
    let fake = Fake::new(vec![dump(NOT_CONNECTED)]);
    let out = fake.run(&wait());
    assert_eq!(out, Outcome::TimedOut { after: SECS(600), last: "enabled=false, allowed=false, connected=false".into() });
    let log = fake.log.borrow();
    assert!(log.last().unwrap().contains("not ready after 600 s"), "{log:?}");
    assert!(log.last().unwrap().contains("running anyway"), "{log:?}");
}

#[test]
fn a_hold_cut_off_by_the_timeout_is_a_timeout() {
    // Ready only from 580 s: 20 s of the 60 s hold by the 600 s timeout.
    let mut dumps = vec![dump(NOT_CONNECTED); 116];
    dumps.push(dump(READY));
    let fake = Fake::new(dumps);
    match fake.run(&wait()) {
        Outcome::TimedOut { after, last } => {
            assert_eq!(after, SECS(600));
            assert_eq!(last, "ready for 20 s of the 60 s hold");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_fused_provider_means_no_wait() {
    let fake = Fake::new(vec![dump("")]);
    assert_eq!(fake.run(&wait()), Outcome::Absent);
    assert_eq!(fake.reads.get(), 1);
    assert!(fake.log.borrow()[0].contains("no fused provider"), "{:?}", fake.log.borrow());
}

#[test]
fn a_fused_provider_that_vanishes_mid_wait_is_not_ready() {
    // Listed at first, so there is Play services to wait for; gone from a later dump.
    let fake = Fake::new(vec![dump(NOT_CONNECTED), dump("")]);
    let w = Wait { timeout: SECS(10), ..wait() };
    assert_eq!(fake.run(&w), Outcome::TimedOut { after: SECS(10), last: "no fused provider listed".into() });
}

#[test]
fn the_log_names_each_change_once_not_every_poll() {
    let mut dumps = vec![dump(NOT_CONNECTED); 10];
    dumps.push(dump(READY));
    let fake = Fake::new(dumps);
    fake.run(&wait());
    let log = fake.log.borrow();
    // The start, one "not ready", one "ready since", the end.
    assert_eq!(log.len(), 4, "{log:?}");
}

#[test]
fn a_device_adb_cannot_reach_times_out_without_hanging() {
    use virtuoso_bench::location::wait_with_adb;
    let missing = std::path::Path::new("/no/such/platform-tools/adb");
    let log = RefCell::new(vec![]);
    let w = Wait { timeout: Duration::ZERO, hold: SECS(60), poll: Duration::from_millis(1) };
    let out = wait_with_adb(missing, "emulator-5554", &w, &|l: &str| log.borrow_mut().push(l.to_string()));
    match out {
        Outcome::TimedOut { last, .. } => assert_eq!(last, "no location providers listed"),
        other => panic!("{other:?}"),
    }
    assert!(log.borrow().last().unwrap().contains("running anyway"), "{:?}", log.borrow());
}

#[test]
fn the_field_wait_is_ten_minutes_with_a_minute_held() {
    let w = Wait::field();
    assert_eq!((w.timeout, w.hold, w.poll), (SECS(600), SECS(60), SECS(5)));
}
