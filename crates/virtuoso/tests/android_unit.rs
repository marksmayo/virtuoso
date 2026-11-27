//! Android helpers that need no device.

use virtuoso::android::{android_permissions, parse_devices, runtime_permissions};

#[test]
fn maestro_permission_names_map_to_android_permissions() {
    assert_eq!(android_permissions("camera"), vec!["android.permission.CAMERA"]);
    assert!(android_permissions("location").contains(&"android.permission.ACCESS_FINE_LOCATION".to_string()));
    assert_eq!(android_permissions("android.permission.READ_SMS"), vec!["android.permission.READ_SMS"]);
    assert!(android_permissions("teleport").is_empty());
}

#[test]
fn runtime_permissions_are_read_from_dumpsys() {
    let dump = "Packages:\n  Package [com.app]\n    install permissions:\n      android.permission.INTERNET: granted=true\n    User 0: ceDataInode=1\n      runtime permissions:\n        android.permission.CAMERA: granted=false, flags=[ USER_SENSITIVE ]\n        android.permission.POST_NOTIFICATIONS: granted=true\n      enabledComponents:\n";
    assert_eq!(runtime_permissions(dump), vec!["android.permission.CAMERA", "android.permission.POST_NOTIFICATIONS"]);
}

#[test]
fn adb_devices_lists_only_ready_devices() {
    let out = "* daemon started\nList of devices attached\nemulator-5554\tdevice\nR58M\tunauthorized\n10.0.0.2:5555\tdevice\n\n";
    assert_eq!(parse_devices(out), vec!["emulator-5554", "10.0.0.2:5555"]);
}

use virtuoso::android_device::error_dialog_button;
use virtuoso::screen::{Element, Rect, ScreenTree, ENABLED, VISIBLE};
use std::time::{Duration, Instant};

fn el(depth: u16, text: &str, pkg: &str, b: [i32; 4]) -> Element {
    Element {
        depth,
        text: text.into(),
        package: pkg.into(),
        bounds: Rect { left: b[0], top: b[1], right: b[2], bottom: b[3] },
        flags: VISIBLE | ENABLED,
        ..Default::default()
    }
}

#[test]
fn an_isnt_responding_dialog_is_answered_with_wait() {
    let s = ScreenTree::new(
        vec![
            el(0, "", "android", [28, 979, 1052, 1493]),
            el(1, "System UI isn't responding", "android", [60, 1000, 1000, 1100]),
            el(1, "Close app", "android", [60, 1200, 1000, 1300]),
            el(1, "Wait", "android", [60, 1350, 1000, 1450]),
        ],
        1080,
        2400,
    );
    assert_eq!(error_dialog_button(&s), Some((530, 1400)));
}

#[test]
fn a_keeps_stopping_dialog_is_closed() {
    let s = ScreenTree::new(
        vec![el(0, "", "android", [0, 0, 1080, 800]), el(1, "Phone keeps stopping", "android", [0, 0, 1080, 100]), el(1, "Close app", "android", [0, 200, 1080, 300])],
        1080,
        2400,
    );
    assert_eq!(error_dialog_button(&s), Some((540, 250)));
}

#[test]
fn an_app_screen_mentioning_wait_is_left_alone() {
    let s = ScreenTree::new(vec![el(0, "Wait", "com.app", [0, 0, 100, 100]), el(0, "isn't responding", "com.app", [0, 100, 100, 200])], 1080, 2400);
    assert_eq!(error_dialog_button(&s), None);
}

#[test]
fn only_permissions_not_yet_granted_need_granting() {
    use virtuoso::android::ungranted_runtime_permissions;
    let dump = "      runtime permissions:\n        android.permission.CAMERA: granted=false, flags=[ USER_SENSITIVE ]\n        android.permission.POST_NOTIFICATIONS: granted=true\n        android.permission.READ_CONTACTS: granted=false\n      enabledComponents:\n";
    assert_eq!(ungranted_runtime_permissions(dump), vec!["android.permission.CAMERA", "android.permission.READ_CONTACTS"]);
}

#[test]
fn the_emulator_to_start_is_the_one_asked_for_or_the_first_listed() {
    use virtuoso::android::pick_avd;
    let listed = "INFO    | Storing crashdata in: /tmp\nPixel_7_API_34\nvirtuoso_api36\n";
    assert_eq!(pick_avd(listed, None).as_deref(), Some("Pixel_7_API_34"));
    assert_eq!(pick_avd(listed, Some("virtuoso_api36")).as_deref(), Some("virtuoso_api36"));
    assert_eq!(pick_avd(listed, Some("nope")), None);
    assert_eq!(pick_avd("", None), None);
}

#[test]
fn each_device_gets_its_own_stable_local_port() {
    use virtuoso::android::local_port_for;
    let a = local_port_for("emulator-5554");
    assert_eq!(a, local_port_for("emulator-5554"), "the same every run");
    assert_ne!(a, local_port_for("emulator-5556"));
    assert!((20000..30000).contains(&a), "{a}");
}

/// When Back was pressed, if it was.
type BackedAt = std::rc::Rc<std::cell::Cell<Option<Instant>>>;

/// A keyboard read that takes `read` and shows the keyboard until `closes` after Back.
fn slow_keyboard(read: Duration, closes: Duration, backed: &BackedAt) -> impl FnMut(Option<u64>, Instant) -> anyhow::Result<(bool, Option<u64>)> {
    let backed = backed.clone();
    move |_, _| {
        std::thread::sleep(read);
        Ok((backed.get().is_none_or(|at| at.elapsed() < closes), Some(1)))
    }
}

/// Back, noting when it was pressed.
fn back(backed: &BackedAt) -> impl FnMut() -> anyhow::Result<()> {
    let backed = backed.clone();
    move || {
        backed.set(Some(Instant::now()));
        Ok(())
    }
}

#[test]
fn hide_keyboard_gives_a_slow_keyboard_its_whole_limit_from_the_back_press() {
    // A software-rendered emulator: the first read takes most of the limit, and the keyboard
    // goes well after Back.
    let backed = BackedAt::default();
    let shown = slow_keyboard(Duration::from_millis(150), Duration::from_millis(250), &backed);
    virtuoso::android_device::close_keyboard(shown, back(&backed), Duration::from_millis(300)).unwrap();
    assert!(backed.get().is_some());
}

#[test]
fn hide_keyboard_fails_on_a_keyboard_that_stays() {
    let backed = BackedAt::default();
    let shown = slow_keyboard(Duration::from_millis(10), Duration::from_secs(60), &backed);
    let e = virtuoso::android_device::close_keyboard(shown, back(&backed), Duration::from_millis(100)).unwrap_err();
    assert!(e.to_string().contains("still shown"), "{e}");
}

#[test]
fn hide_keyboard_without_a_keyboard_presses_nothing() {
    let pressed = std::cell::Cell::new(false);
    let none = |_: Option<u64>, _: Instant| Ok((false, Some(1)));
    let back = || {
        pressed.set(true);
        Ok(())
    };
    virtuoso::android_device::close_keyboard(none, back, Duration::from_millis(100)).unwrap();
    assert!(!pressed.get());
}

fn launch(clear_state: bool) -> String {
    let o = virtuoso::device::LaunchOptions { clear_state, stop_app: true, ..Default::default() };
    virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &o)
}

#[test]
fn a_launch_grants_the_permissions_side_by_side_and_starts_once_all_are_done() {
    // One after another, nine grants took ~270 ms on the emulator; side by side ~110 ms.
    for script in [launch(false), launch(true)] {
        let grant = script.find(" grant com.app $p 2>/dev/null & done; wait; ").unwrap_or_else(|| panic!("{script}"));
        let start = script.find(" start -W ").unwrap();
        assert!(grant < start, "{script}");
    }
}

#[test]
fn a_clear_state_launch_lists_the_permissions_while_the_data_is_cleared() {
    // pm clear (it stops the app and wipes its data) and dumpsys took ~440 ms one after the
    // other on the emulator, ~290 ms side by side. The clear's failure still stops the launch.
    let script = launch(true);
    let clear = script.find("cmd package clear com.app >/dev/null & c=$!; ").unwrap_or_else(|| panic!("{script}"));
    let dump = script.find("dumpsys package com.app").unwrap();
    let joined = script.find("wait $c || { echo 'clearing com.app failed'; exit 1; }; ").unwrap_or_else(|| panic!("{script}"));
    let grant = script.find(" grant com.app ").unwrap();
    assert!(clear < dump && dump < joined && joined < grant, "{script}");
    // Every permission listed is granted: the clear revoked them all, whatever the list said.
    assert!(!script.contains("granted=false"), "{script}");
}

#[test]
fn a_stop_launch_lists_the_ungranted_permissions_while_the_app_is_stopped() {
    // The force-stop and dumpsys ran one after the other, as the clear and dumpsys once did; the
    // grants still wait for the stop. A stop that finds no app running is no failure, as before.
    let script = launch(false);
    let stop = script.find("{ pidof com.app >/dev/null && cmd activity force-stop com.app; } & s=$!; ").unwrap_or_else(|| panic!("{script}"));
    let list = script.find("ps=$(dumpsys package com.app").unwrap_or_else(|| panic!("{script}"));
    let joined = script.find("wait $s; ").unwrap_or_else(|| panic!("{script}"));
    let grant = script.find("for p in $ps; do cmd package grant com.app $p").unwrap_or_else(|| panic!("{script}"));
    assert!(stop < list && list < joined && joined < grant, "{script}");
    assert!(script.contains("granted=false"), "only those not granted yet: {script}");
    // Without a stop, the list is taken where it is used, as before.
    let o = virtuoso::device::LaunchOptions { stop_app: false, ..Default::default() };
    let plain = virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &o);
    assert!(!plain.contains("force-stop") && plain.contains("for p in $(dumpsys package com.app"), "{plain}");
}

#[test]
fn a_launch_calls_the_services_without_the_am_and_pm_scripts() {
    // /system/bin/am and pm are shell scripts that run `cmd activity` / `cmd package`: a shell
    // started for each clear, stop, grant and start.
    let stop = virtuoso::device::LaunchOptions { stop_app: true, permissions: vec![("camera".into(), "allow".into())], ..Default::default() };
    let given = virtuoso::device::LaunchOptions { clear_state: true, permissions: vec![("camera".into(), "allow".into())], ..Default::default() };
    let scripts = [launch(false), launch(true), virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &stop), virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &given)];
    for script in scripts {
        let words: Vec<&str> = script.split(|c: char| c.is_whitespace() || ";&|(){}$".contains(c)).collect();
        assert!(!words.contains(&"am") && !words.contains(&"pm"), "{script}");
        assert!(script.contains("cmd activity start -W -n com.app/.Main"), "{script}");
    }
}

#[test]
fn a_launch_lists_the_permissions_with_one_sed() {
    // dumpsys | grep | sed | grep | sed: four processes only to narrow text one sed narrows.
    for script in [launch(false), launch(true)] {
        let list = &script[script.find("dumpsys package com.app").unwrap()..];
        let list = &list[..list.find(')').unwrap()];
        assert_eq!(list.matches('|').count(), 1, "{list}");
        assert!(list.contains("| sed -n "), "{list}");
    }
}

#[test]
fn a_first_launch_finds_the_apps_launcher_activity_in_the_same_script() {
    // Each run is a new process with nothing cached: the lookup was an exec of its own.
    let o = virtuoso::device::LaunchOptions { clear_state: true, ..Default::default() };
    let script = virtuoso::android_device::launch_script("com.app", None, &o);
    let resolve = script.find("set -- $(cmd package resolve-activity --brief -c android.intent.category.LAUNCHER com.app); eval a=\\${$#}; ").unwrap_or_else(|| panic!("{script}"));
    let missing = script.find("case $a in */*) ;; *) echo 'com.app is not installed, or has no launcher activity'; exit 3;; esac; echo \"launcher $a\"; ").unwrap_or_else(|| panic!("{script}"));
    let clear = script.find("cmd package clear").unwrap();
    let start = script.find("cmd activity start -W -n $a ").unwrap_or_else(|| panic!("{script}"));
    assert!(resolve < missing && missing < clear && clear < start, "{script}");
    // Known already: no lookup.
    let known = virtuoso::android_device::launch_script("com.app", Some("com.app/.Main"), &o);
    assert!(!known.contains("resolve-activity") && known.contains("-n com.app/.Main "), "{known}");
}

#[test]
fn the_agent_starts_without_waiting_on_the_old_jars_cleanup() {
    // touch and find (~20 ms on the emulator) only keep other builds' jars from piling up.
    let script = virtuoso::android::agent_start_script("/data/local/tmp/virtuoso-agent-1.jar", "", "tok", true);
    let missing = script.find("[ -f /data/local/tmp/virtuoso-agent-1.jar ] || { echo missing; exit 0; }; ").unwrap_or_else(|| panic!("{script}"));
    // This build's jar is touched before find can take it for an old one.
    let housekeeping = script.find("(touch /data/local/tmp/virtuoso-agent-1.jar; find /data/local/tmp -maxdepth 1 -name 'virtuoso-agent-*.jar' -mtime +7 -delete) & ").unwrap_or_else(|| panic!("{script}"));
    let start = script.find("exec app_process").unwrap();
    assert!(missing < housekeeping && housekeeping < start, "{script}");
    assert_eq!(script.matches("find ").count(), 1, "{script}");
}

/// Runs the agent's start script in `sh` (None when there is no `sh`) with a jar that is not
/// there (so it stops at saying `missing`), and `pidof`, `kill` and `sleep` faked: an old agent
/// that exits `term_polls` polls after a SIGTERM (never, with None), and at once on a SIGKILL.
/// What the script printed, and the calls it made, one a line.
fn run_stop(term_polls: Option<u32>) -> Option<String> {
    let script = virtuoso::android::agent_start_script("/nonexistent/virtuoso-agent-1.jar", "", "tok", true);
    let after_term = term_polls.map_or("1000000".to_string(), |n| n.to_string());
    let fakes = format!(
        "alive=1; termed=; polls=0; \
         pidof() {{ [ $alive = 1 ] && {{ echo 4242; [ -n \"$termed\" ] && polls=$((polls+1)) && [ $polls -ge {after_term} ] && alive=0; return 0; }}; return 1; }}; \
         kill() {{ echo \"kill $*\"; case $1 in -9|-KILL) alive=0;; *) termed=1;; esac; }}; \
         sleep() {{ echo \"sleep $1\"; }}; "
    );
    let out = std::process::Command::new("sh").arg("-c").arg(fakes + &script).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn the_agent_starts_only_once_the_old_one_has_exited() {
    // The old agent's UiAutomation is only let go when its process is gone: a new agent started
    // beside it is refused one. The stop gave it 2 s and started the new agent anyway.
    let Some(out) = run_stop(None) else { return };
    let kills: Vec<&str> = out.lines().filter(|l| l.starts_with("kill")).collect();
    assert_eq!(kills, ["kill 4242", "kill -9 4242"], "{out}");
    // A SIGTERM first, and time to act on it (2 s of 0.1 s polls), before the SIGKILL.
    let term = out.find("kill 4242").unwrap();
    let hard = out.find("kill -9 4242").unwrap();
    assert_eq!(out[term..hard].matches("sleep 0.1").count(), 20, "{out}");
    assert!(out.trim_end().ends_with("missing"), "{out}");
}

#[test]
fn an_old_agent_that_exits_on_sigterm_is_not_killed_hard_or_waited_for_longer() {
    let Some(out) = run_stop(Some(3)) else { return };
    assert!(!out.contains("kill -9"), "{out}");
    assert!(out.matches("sleep 0.1").count() <= 3, "{out}");
    assert!(out.trim_end().ends_with("missing"), "{out}");
}

#[test]
fn with_no_old_agent_the_stop_costs_one_pidof() {
    let script = virtuoso::android::agent_start_script("/nonexistent/virtuoso-agent-1.jar", "", "tok", true);
    let fakes = "pidof() { echo pidof >&2; return 1; }; kill() { echo kill; }; sleep() { echo sleep; }; ";
    let Ok(out) = std::process::Command::new("sh").arg("-c").arg(format!("{fakes}{script}")).output() else { return };
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "missing");
    assert_eq!(String::from_utf8_lossy(&out.stderr).matches("pidof").count(), 1);
}

#[test]
fn the_host_waits_for_the_agent_longer_than_the_agent_waits_for_its_uiautomation() {
    // On the Windows runner's emulator the system took up to ~20 s to grant the UiAutomation
    // after a boot (P02: ready 5.4-16.1 s after a launch script), and missed the agent's 20 s
    // four times out of 36. The agent waits longer now, and the host must outwait it, or it
    // gives up on an agent that was about to start.
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agent/android/src/dev/virtuoso/agent/Pace.java")).unwrap();
    let ms = |name: &str| -> u64 {
        let pattern = format!("static final long {name} = ");
        let at = src.find(&pattern).unwrap_or_else(|| panic!("no {name} in Pace.java"));
        let rest = &src[at + pattern.len()..];
        rest[..rest.find(';').unwrap()].replace('_', "").trim().parse().unwrap()
    };
    let connect = ms("CONNECT_TIMEOUT_MS");
    assert!(connect >= 60_000, "the agent waits {connect} ms for its UiAutomation");
    let host = virtuoso::android::AGENT_START_TIMEOUT.as_millis() as u64;
    // Its JVM's start and the retries while an old UiAutomation is let go come on top.
    assert!(host >= connect + ms("HELD_WAIT_MS") + 5_000, "the host waits {host} ms, the agent {connect} ms");
}

#[test]
fn after_pushing_the_agent_it_starts_without_stopping_one_again() {
    // The first script stopped any agent (a pidof, ~30-50 ms on the emulator) before it said
    // the jar was missing: nothing can have started one since.
    let first = virtuoso::android::agent_start_script("/data/local/tmp/virtuoso-agent-1.jar", "", "tok", true);
    assert!(first.contains("pidof virtuoso-agent"), "{first}");
    let second = virtuoso::android::agent_start_script("/data/local/tmp/virtuoso-agent-1.jar", "", "tok", false);
    assert!(!second.contains("pidof") && !second.contains("kill"), "{second}");
    // The first exited before its housekeeping: this one does it.
    assert!(second.contains("find /data/local/tmp") && second.contains("exec app_process"), "{second}");
}

/// The animations check run in `sh` (None when there is no `sh`) on a device whose global
/// settings list is `list`: the keys it wrote, sorted, and whether it started a grep.
fn animations_written(list: &str) -> Option<(Vec<String>, bool)> {
    let fakes = "cmd() { if [ \"$2\" = list ]; then printf '%s\\n' \"$LIST\"; else echo \"put $4\"; fi; }; grep() { echo grep >&2; command grep \"$@\"; }; ";
    let script = format!("{fakes}{}", virtuoso::android::animations_off_script());
    let out = std::process::Command::new("sh").arg("-c").arg(script).env("LIST", list).output().ok()?;
    let mut put: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix("put ").map(str::to_string)).collect();
    put.sort();
    Some((put, String::from_utf8_lossy(&out.stderr).contains("grep")))
}

#[test]
fn the_animations_check_starts_no_process_but_cmd() {
    // Each of the three checks was a pipeline into grep: a fork and a process start each, twice
    // a run, beside the agent's start. The shell's own `case` gives the same answers: a scale is
    // off when a whole line of the list is `<key>=0` or `<key>=0.0`.
    let (w, t, a) = ("window_animation_scale", "transition_animation_scale", "animator_duration_scale");
    let all = vec![a.to_string(), t.to_string(), w.to_string()];
    let cases: [(String, Vec<String>); 8] = [
        (format!("adb_enabled=1\n{w}=0\n{t}=0\n{a}=0\nzen_mode=0"), vec![]),
        (format!("{w}=0.0\n{t}=0.0\n{a}=0.0"), vec![]),
        (format!("{w}=1.0\n{t}=0\n{a}=0.0"), vec![w.to_string()]),
        (format!("{w}=0\n{a}=0"), vec![t.to_string()]),
        (format!("{w}=10\n{t}=0.5\n{a}=00"), all.clone()),
        // Only a whole line counts: not a key that ends with the name, nor a value that goes on.
        (format!("x_{w}=0\n{t}=0 \n{a}=0.01"), all.clone()),
        (format!("{w}=0.0.0\n{t}=0x0\n{a}=null"), all.clone()),
        (String::new(), all.clone()),
    ];
    for (list, want) in cases {
        let Some((put, grep)) = animations_written(&list) else { return };
        assert_eq!(put, want, "list {list:?}");
        assert!(!grep, "a grep was started for list {list:?}");
    }
}

/// A first launch's script (no launcher known yet) with the animations check beside it, run in
/// `sh` (None when there is none) with the device's commands faked: the settings list takes
/// 0.3 s and says every scale is on, and `launcher` is what the lookup prints last. The order
/// of what happened (one event a line), the script's output and its exit code.
fn first_launch(launcher: &str, o: &virtuoso::device::LaunchOptions) -> Option<(Vec<String>, String, i32)> {
    let log = std::env::temp_dir().join(format!("virtuoso-first-launch-{}-{launcher}.log", std::process::id()).replace('/', "_"));
    let _ = std::fs::remove_file(&log);
    let fakes = "cmd() { case \"$1 $2\" in \
         'settings list') sleep 0.3; printf '%s\\n' window_animation_scale=1.0 transition_animation_scale=1.0 animator_duration_scale=1.0;; \
         'settings put') echo \"put $4\" >> \"$LOG\";; \
         'package resolve-activity') echo resolve >> \"$LOG\"; echo \"priority=0 $LAUNCHER\";; \
         'package clear') echo clear >> \"$LOG\";; \
         'package grant') echo grant >> \"$LOG\";; \
         'activity start') echo start >> \"$LOG\"; echo Status: ok;; \
         *) echo \"other $*\" >> \"$LOG\";; esac; }; \
         dumpsys() { printf '%s\\n' '    runtime permissions:' '      android.permission.CAMERA: granted=false, flags=[ ]'; }; \
         pidof() { return 1; }; ";
    let script = virtuoso::android_device::launch_script_beside("com.app", None, o, Some(&virtuoso::android::animations_off_script()));
    let out = std::process::Command::new("sh").arg("-c").arg(format!("{fakes}( {script} ); echo \"exit $?\"")).env("LOG", &log).env("LAUNCHER", launcher).output().ok()?;
    let events = std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_string).collect();
    let _ = std::fs::remove_file(&log);
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    let (out, code) = out.trim_end().rsplit_once("exit ").map(|(o, c)| (o.to_string(), c.parse().unwrap())).unwrap();
    Some((events, out, code))
}

#[test]
fn the_first_launchs_animations_check_runs_beside_the_script_and_ends_before_the_start() {
    // The check (a settings list, up to three puts) ran to its end before the launcher lookup,
    // the clear and the permission list began: 126 ms on the Linux runner's emulator and 607 ms
    // on the Windows runner's, of independent device commands one after another.
    for clear_state in [true, false] {
        let o = virtuoso::device::LaunchOptions { clear_state, stop_app: true, ..Default::default() };
        let Some((events, out, code)) = first_launch("com.app/.Main", &o) else { return };
        let at = |e: &str| events.iter().position(|x| x == e).unwrap_or_else(|| panic!("no {e} in {events:?}"));
        let last_put = events.iter().rposition(|x| x.starts_with("put ")).unwrap_or_else(|| panic!("{events:?}"));
        // The launch's own commands do not wait for the check...
        assert!(at("resolve") < at("put window_animation_scale"), "{events:?}");
        assert!(at("grant") < last_put, "{events:?}");
        // ... and the app starts only once every scale is off.
        assert_eq!(events.iter().filter(|x| x.starts_with("put ")).count(), 3, "{events:?}");
        assert!(last_put < at("start"), "{events:?}");
        assert_eq!(events.last().map(String::as_str), Some("start"), "{events:?}");
        assert_eq!((out.as_str(), code), ("launcher com.app/.Main\nStatus: ok\n", 0));
    }
}

#[test]
fn a_first_launch_that_stops_early_still_leaves_the_animations_off() {
    // An app with no launcher ends the script before the start, with exit 3 and its message as
    // before; the check beside it is waited for, so the scales are off when the script returns.
    let o = virtuoso::device::LaunchOptions { clear_state: true, ..Default::default() };
    let Some((events, out, code)) = first_launch("none", &o) else { return };
    assert_eq!((out.as_str(), code), ("com.app is not installed, or has no launcher activity\n", 3));
    assert_eq!(events.iter().filter(|x| x.starts_with("put ")).count(), 3, "{events:?}");
    assert!(!events.iter().any(|x| x == "clear" || x == "start"), "{events:?}");
}

#[test]
fn a_launch_with_nothing_beside_it_is_the_script_it_was() {
    // Later launches go through the agent with no check: their script has no job to wait for.
    let o = virtuoso::device::LaunchOptions { clear_state: true, ..Default::default() };
    let plain = virtuoso::android_device::launch_script("com.app", None, &o);
    assert_eq!(plain, virtuoso::android_device::launch_script_beside("com.app", None, &o, None));
    assert!(!plain.contains("$A") && !plain.contains("settings"), "{plain}");
}
