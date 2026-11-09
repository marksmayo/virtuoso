//! Which tools the benchmark runs, and how, on each kind of host.

use std::path::Path;
use virtuoso_bench::measure::Runner;
use virtuoso_bench::tools::{tools, Host};

#[test]
fn on_linux_and_macos_every_tool_runs_natively() {
    let all = tools(Path::new("/work/virtuoso"), &Host::Unix);
    let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
    for want in ["virtuoso", "maestro-runner", "speedrun", "maestro", "appium", "e2e", "argent"] {
        assert!(names.contains(&want), "missing {want}: {names:?}");
    }
    // The native Windows build is the Windows host's supplement.
    assert!(!names.contains(&"virtuoso-windows"));
    for t in &all {
        assert!(matches!(t.runner, Runner::Native), "{} runs natively", t.name);
        assert!(!t.test_argv[0].ends_with(".exe"), "{}: {}", t.name, t.test_argv[0]);
    }
}

#[test]
fn on_windows_speedrun_and_the_linux_virtuoso_run_in_wsl() {
    let all = tools(Path::new(r"C:\work\virtuoso"), &Host::WindowsWsl { distro: "Ubuntu".into() });
    let runner = |name: &str| &all.iter().find(|t| t.name == name).unwrap().runner;
    assert!(matches!(runner("speedrun"), Runner::Wsl { .. }));
    assert!(matches!(runner("virtuoso"), Runner::Wsl { .. }));
    assert!(matches!(runner("maestro-runner"), Runner::Native));
    assert!(all.iter().any(|t| t.name == "virtuoso-windows"));
}

use virtuoso_bench::tools::{tools_for, Platform};

#[test]
fn on_ios_every_tool_is_told_the_platform_and_none_samples_android_agents() {
    let all = tools_for(Path::new("/work/virtuoso"), &Host::Unix, Platform::Ios);
    let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
    for want in ["virtuoso", "maestro-runner", "speedrun", "maestro", "appium", "e2e", "argent"] {
        assert!(names.contains(&want), "missing {want}: {names:?}");
    }
    let argv = |name: &str| all.iter().find(|t| t.name == name).unwrap().test_argv.join(" ");
    assert!(argv("virtuoso").contains("--platform ios"), "{}", argv("virtuoso"));
    assert!(argv("speedrun").contains("-p ios"), "{}", argv("speedrun"));
    assert!(argv("maestro-runner").contains("--platform ios"), "{}", argv("maestro-runner"));
    assert!(argv("argent").contains("--platform ios"), "{}", argv("argent"));
    let env = |name: &str| all.iter().find(|t| t.name == name).unwrap().env.clone();
    assert!(env("appium").iter().any(|(k, v)| k == "VIRTUOSO_BENCH_PLATFORM" && v == "ios"));
    assert!(env("e2e").iter().any(|(k, v)| k == "VIRTUOSO_BENCH_PLATFORM" && v == "ios"));
    // The Settings scenarios as iOS's Settings app has them.
    let flows = |name: &str| all.iter().find(|t| t.name == name).unwrap().flows_dir.replace('\\', "/");
    for t in ["virtuoso", "speedrun", "maestro-runner", "maestro"] {
        assert!(flows(t).ends_with("bench/flows-ios"), "{t}: {}", flows(t));
    }
    // e2e and Argent have their own tests of iOS's Settings app (settings-ios), Appium in run.mjs:
    // every tool runs those scenarios.
    for t in ["e2e", "argent"] {
        assert!(flows(t).ends_with("settings-ios"), "{t}: {}", flows(t));
    }
    let skips = |name: &str| all.iter().find(|t| t.name == name).unwrap().skips.clone();
    for s in ["launch", "navigate", "scroll", "input", "assertions", "taps", "trivial"] {
        for t in ["e2e", "argent", "appium"] {
            assert!(!skips(t).contains(&s.to_string()), "{t} runs {s} on iOS");
        }
    }
    assert!(!skips("virtuoso").contains(&"launch".to_string()));
    // Argent's tests of five Speedrun app flows run adb scripts (dark mode, rotation, launch
    // arguments): Android's, so skipped on iOS.
    for s in ["app/appearance", "app/device", "app/inputs", "app/remember_me", "app/scripting"] {
        assert!(skips("argent").contains(&s.to_string()), "argent skips {s} on iOS");
    }
    // Agent memory is sampled through adb: nothing to sample on a simulator.
    assert!(all.iter().all(|t| t.agent_processes.is_empty()));
}

#[test]
fn appium_runs_the_speedrun_app_scenarios_on_android_and_ios() {
    // Its tests of the Speedrun Bench app's flows: bench/appium/app/scenarios.mjs, run from app/.
    for platform in [Platform::Android, Platform::Ios] {
        let all = tools_for(Path::new("/work/virtuoso"), &Host::Unix, platform);
        let appium = all.iter().find(|t| t.name == "appium").unwrap();
        let (argv, dir) = virtuoso_bench::orchestrate::invocation(appium, "app/cart").expect("appium has a test of app/cart");
        assert!(dir.replace('\\', "/").ends_with("bench/appium/app"), "{dir}");
        assert_eq!(argv.last().unwrap(), "cart.yaml");
        assert!(virtuoso_bench::orchestrate::invocation(appium, "app/appearance").is_some());
    }
}

#[test]
fn appium_runs_the_settings_scenarios_on_ios() {
    // Its Settings tests have iOS versions (run.mjs, XCUITest on iOS's Settings app): it skips
    // only the invalid flow, as on Android.
    let all = tools_for(Path::new("/work/virtuoso"), &Host::Unix, Platform::Ios);
    let appium = all.iter().find(|t| t.name == "appium").unwrap();
    for s in ["launch", "navigate", "scroll", "input", "assertions", "taps", "trivial"] {
        let (argv, dir) = virtuoso_bench::orchestrate::invocation(appium, s).unwrap_or_else(|| panic!("appium has a test of {s} on iOS"));
        assert_eq!(argv.last().unwrap(), &format!("{s}.yaml"));
        assert!(dir.replace('\\', "/").ends_with("bench/flows-ios"), "{dir}");
    }
    assert!(virtuoso_bench::orchestrate::invocation(appium, "invalid").is_none());
}
