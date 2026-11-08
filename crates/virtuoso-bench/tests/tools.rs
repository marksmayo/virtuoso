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
