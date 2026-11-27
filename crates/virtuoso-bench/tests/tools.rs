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

#[test]
fn where_the_tools_ran_follows_the_host_and_platform() {
    use virtuoso_bench::tools::{where_tools_ran, Host, Platform};
    let ios = where_tools_ran(&Host::Unix, Platform::Ios);
    assert!(ios.contains("simulator") && !ios.contains("WSL") && !ios.contains("adb"), "{ios}");
    let linux = where_tools_ran(&Host::Unix, Platform::Android);
    assert!(linux.contains("natively") && !linux.contains("WSL"), "{linux}");
    let windows = where_tools_ran(&Host::WindowsWsl { distro: "Ubuntu-24.04".into() }, Platform::Android);
    assert!(windows.contains("WSL") && windows.contains("Ubuntu-24.04"), "{windows}");
}

#[test]
fn on_the_web_virtuoso_and_maestro_run_the_web_flows_in_chrome() {
    // A field of its own: Tester Army's web benchmark (bench/web-flows), each tool driving a
    // Chrome of its own natively (no device, no WSL: Chrome runs on every host).
    for host in [Host::Unix, Host::WindowsWsl { distro: "X".into() }] {
        let all = tools_for(Path::new("/work/virtuoso"), &host, Platform::Web);
        let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
        for want in ["virtuoso", "maestro"] {
            assert!(names.contains(&want), "missing {want}: {names:?}");
        }
        for gone in ["speedrun", "maestro-runner", "argent", "virtuoso-windows"] {
            assert!(!names.contains(&gone), "{gone} has no web support: {names:?}");
        }
        let virtuoso = all.iter().find(|t| t.name == "virtuoso").unwrap();
        assert!(matches!(virtuoso.runner, virtuoso_bench::measure::Runner::Native), "natively, even on Windows");
        assert!(virtuoso.test_argv.join(" ").contains("--platform web"), "{:?}", virtuoso.test_argv);
        assert!(!virtuoso.test_argv.iter().any(|a| a == "--device"), "no device: {:?}", virtuoso.test_argv);
        for tool in &all {
            let (argv, dir) = virtuoso_bench::orchestrate::invocation(tool, "web/login_form").unwrap_or_else(|| panic!("{} runs web/login_form", tool.name));
            assert!(dir.replace('\\', "/").ends_with("bench/web-flows"), "{}: {dir}", tool.name);
            assert_eq!(argv.last().unwrap(), "login_form.yaml", "{}", tool.name);
            assert!(tool.agent_processes.is_empty(), "{}: no device agent to sample", tool.name);
        }
    }
}

#[test]
fn on_the_web_the_code_tools_run_their_own_port_of_each_flow() {
    // Playwright, Puppeteer, Selenium, Cypress and e2e: each test is code, a port of the flow in
    // bench/web/<tool>, run by its run.mjs given the flow's file name.
    let all = tools_for(Path::new("/work/virtuoso"), &Host::Unix, Platform::Web);
    for name in ["playwright", "puppeteer", "selenium", "cypress", "e2e"] {
        let tool = all.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("missing {name}"));
        assert!(matches!(tool.runner, virtuoso_bench::measure::Runner::Native), "{name}");
        let (argv, dir) = virtuoso_bench::orchestrate::invocation(tool, "web/login_form").unwrap();
        let run = format!("bench/web/{name}/run.mjs");
        assert_eq!(argv[0], "node", "{name}: {argv:?}");
        assert!(argv[1].replace('\\', "/").ends_with(&run), "{name}: {argv:?}");
        assert_eq!(argv.last().unwrap(), "login_form.yaml", "{name}");
        assert!(dir.replace('\\', "/").ends_with("bench/web-flows"), "{name}: {dir}");
        assert!(tool.version_argv.last().unwrap() == "--version", "{name}: {:?}", tool.version_argv);
        assert!(tool.install_files[0].replace('\\', "/").ends_with(&format!("bench/web/{name}/node_modules")), "{name}: {:?}", tool.install_files);
    }
}

#[test]
fn maestro_gets_the_desktop_window_every_tool_has_and_a_phone_field_tells_each_tool_the_phone() {
    use virtuoso_bench::tools::web_tools_for;
    let repo = Path::new("/work/virtuoso");
    let desk = web_tools_for(repo, None);
    let maestro = desk.iter().find(|t| t.name == "maestro").unwrap();
    assert!(maestro.test_argv.join(" ").contains("--screen-size 1280x800"), "{:?}", maestro.test_argv);
    assert!(desk.iter().all(|t| !t.env.iter().any(|(k, _)| k == "VIRTUOSO_EMULATE" || k == "VIRTUOSO_BENCH_EMULATE")));

    let phone = web_tools_for(repo, Some("pixel-7"));
    let maestro = phone.iter().find(|t| t.name == "maestro").unwrap();
    assert!(maestro.test_argv.join(" ").contains("--screen-size 412x839"), "Maestro's web mode has only a window size: {:?}", maestro.test_argv);
    let virtuoso = phone.iter().find(|t| t.name == "virtuoso").unwrap();
    assert!(virtuoso.env.contains(&("VIRTUOSO_EMULATE".to_string(), "pixel-7".to_string())), "{:?}", virtuoso.env);
    for t in phone.iter().filter(|t| !["virtuoso", "maestro"].contains(&t.name.as_str())) {
        assert!(t.env.contains(&("VIRTUOSO_BENCH_EMULATE".to_string(), "pixel-7".to_string())), "{}: {:?}", t.name, t.env);
    }
}

#[test]
fn a_phone_field_runs_a_flows_phone_copy_where_there_is_one() {
    // bench/web-flows/<phone>/<flow>.yaml stands in for a flow whose screen points are the
    // desktop's; every other flow is the desktop one.
    let d = tempfile::tempdir().unwrap();
    let flows = d.path().join("bench").join("web-flows");
    std::fs::create_dir_all(flows.join("pixel-7")).unwrap();
    std::fs::write(flows.join("kanban_board.yaml"), "").unwrap();
    std::fs::write(flows.join("login_form.yaml"), "").unwrap();
    std::fs::write(flows.join("pixel-7").join("kanban_board.yaml"), "").unwrap();
    let phone = virtuoso_bench::tools::web_tools_for(d.path(), Some("pixel-7"));
    let v = phone.iter().find(|t| t.name == "virtuoso").unwrap();
    let (argv, _) = virtuoso_bench::orchestrate::invocation(v, "web/kanban_board").unwrap();
    assert_eq!(argv.last().unwrap().replace('\\', "/"), "pixel-7/kanban_board.yaml");
    let (argv, _) = virtuoso_bench::orchestrate::invocation(v, "web/login_form").unwrap();
    assert_eq!(argv.last().unwrap(), "login_form.yaml");
    let desk = virtuoso_bench::tools::web_tools_for(d.path(), None);
    let v = desk.iter().find(|t| t.name == "virtuoso").unwrap();
    assert_eq!(virtuoso_bench::orchestrate::invocation(v, "web/kanban_board").unwrap().0.last().unwrap(), "kanban_board.yaml");
}

#[test]
fn the_android_field_drives_the_devices_chrome_with_every_tool_that_can() {
    use virtuoso_bench::orchestrate::DEVICE;
    use virtuoso_bench::tools::web_android_tools;
    let all = web_android_tools(Path::new("/work/virtuoso"));
    let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["virtuoso", "maestro", "playwright", "puppeteer", "selenium", "appium"], "Cypress cannot drive a device; e2e's web engine needs browser contexts Chrome on Android lacks");
    let v = &all[0];
    assert!(v.test_argv.join(" ").contains(&format!("--device android:{DEVICE}")), "{:?}", v.test_argv);
    let m = &all[1];
    assert!(m.test_argv.iter().any(|a| a.replace('\\', "/").ends_with("bench/web/maestro-android.mjs")), "Maestro as a Chrome app flow: {:?}", m.test_argv);
    for t in &all[1..] {
        assert!(t.env.contains(&("VIRTUOSO_BENCH_ANDROID".to_string(), DEVICE.to_string())), "{}: {:?}", t.name, t.env);
    }
    for t in &all {
        // Its own copies: the phone's screen points (as the Pixel 7 field's), and what Chrome on
        // Android shows a tool that reads its accessibility tree (no data-testid).
        assert_eq!(t.flow_variant.as_deref(), Some("android"), "{}: the Android field's flow copies", t.name);
        assert!(t.suite_dirs["web"].replace('\\', "/").ends_with("bench/web-flows"));
    }
    // The field is picked by VIRTUOSO_BENCH_WEB_DEVICE=android; the serial is the run's --device.
    let tool = all.iter().find(|t| t.name == "puppeteer").unwrap().on_device("emulator-5554");
    assert!(tool.env.contains(&("VIRTUOSO_BENCH_ANDROID".to_string(), "emulator-5554".to_string())));
}

#[test]
fn every_field_runs_the_maestro_its_setup_installs() {
    use virtuoso_bench::tools::{web_android_tools, web_tools_for, MAESTRO_VERSION};
    // Its latest release (cli-2.12.1, 2026-11-02).
    assert_eq!(MAESTRO_VERSION, "2.12.1");
    let repo = Path::new("/work/virtuoso");
    let home = format!("bench/tools/maestro-{MAESTRO_VERSION}/maestro");
    let mut fields = vec![
        tools_for(repo, &Host::Unix, Platform::Android),
        tools_for(repo, &Host::Unix, Platform::Ios),
        web_tools_for(repo, None),
    ];
    fields.push(web_android_tools(repo));
    for all in &fields {
        let m = all.iter().find(|t| t.name == "maestro").unwrap();
        let named = m.install_files.iter().chain(&m.test_argv).chain(m.env.iter().map(|(_, v)| v)).any(|s| s.replace('\\', "/").contains(&home));
        assert!(named, "{:?} {:?} {:?}", m.install_files, m.test_argv, m.env);
    }
    // Every setup script and workflow fetches (and caches) that release, and no other.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in [
        "bench/ci/setup-linux.sh",
        "bench/ci/setup-macos.sh",
        "bench/ci/setup-web.sh",
        "bench/ci/setup-windows.ps1",
        ".github/workflows/android-bench.yml",
        ".github/workflows/ios-bench.yml",
        ".github/workflows/web-bench.yml",
        ".github/workflows/web-android-bench.yml",
    ] {
        let text = std::fs::read_to_string(root.join(f)).unwrap();
        assert!(text.contains(&format!("maestro-{MAESTRO_VERSION}")), "{f} names maestro-{MAESTRO_VERSION}");
        let stale: Vec<&str> = text.matches("maestro-2.").collect();
        let current = text.matches(&format!("maestro-{MAESTRO_VERSION}")).count();
        assert_eq!(stale.len(), current, "{f} names another Maestro");
        if f.starts_with("bench/ci/") {
            assert!(text.contains(&format!("releases/download/cli-{MAESTRO_VERSION}/maestro.zip")), "{f} downloads cli-{MAESTRO_VERSION}");
        }
    }
}

#[test]
fn every_windows_benchmark_offers_a_defender_exclusion_off_by_default_for_every_tool() {
    // Microsoft Defender's real-time scan of the temp folder, where every tool's browser
    // profiles and outputs are written: an input of each Windows benchmark workflow, off unless
    // asked for, taken before any tool runs, for all of them alike, and said in the host note.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in [".github/workflows/android-bench-windows.yml", ".github/workflows/web-bench.yml"] {
        let text = std::fs::read_to_string(root.join(f)).unwrap().replace("\r\n", "\n");
        let input = &text[text.find("      defender_exclusion:\n").unwrap_or_else(|| panic!("{f}: no defender_exclusion input"))..];
        let input = &input[..input.find("\n\n").unwrap_or(input.len())];
        assert!(input.contains("type: boolean") && input.contains("default: false"), "{f}: off by default: {input}");
        let step = text.find("./bench/ci/defender-windows.ps1").unwrap_or_else(|| panic!("{f}: no Defender step"));
        let run = text.find("      - name: Run\n").unwrap_or_else(|| panic!("{f}: no Run step"));
        assert!(step < run, "{f}: the exclusion is in place before any tool runs");
        let line = text[step..].lines().next().unwrap();
        assert!(line.contains("inputs.defender_exclusion"), "{f}: the step takes the input: {line}");
        assert!(!text[step.saturating_sub(400)..step].contains("matrix.tool"), "{f}: not per tool");
        let host = text.lines().find(|l| l.contains("VIRTUOSO_BENCH_HOST:")).unwrap();
        assert!(host.contains("inputs.defender_exclusion") && host.contains("Defender"), "{f}: the README says so: {host}");
    }
    let script = std::fs::read_to_string(root.join("bench/ci/defender-windows.ps1")).expect("bench/ci/defender-windows.ps1");
    assert!(script.contains("Add-MpPreference -ExclusionPath"), "{script}");
    assert!(script.contains("$env:TEMP") && script.contains("$env:RUNNER_TEMP"), "every tool's temp folders: {script}");
    assert!(script.contains("Get-MpComputerStatus"), "the state is logged either way: {script}");
}
