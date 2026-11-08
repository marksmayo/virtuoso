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
    // Agent memory is sampled through adb: nothing to sample on a simulator.
    assert!(all.iter().all(|t| t.agent_processes.is_empty()));
}
