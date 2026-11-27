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
fn an_ab_base_is_a_second_virtuoso_run_from_the_base_build() {
    // web-bench's ab_base mode: the base ref's virtuoso, built beside this one, is a tool of its
    // own (virtuoso-base), told everything this field tells virtuoso.
    use virtuoso_bench::orchestrate::AB_BASE;
    use virtuoso_bench::tools::{web_tools_for, with_ab_base};
    let repo = Path::new("/work/virtuoso");
    let plain = web_tools_for(repo, Some("pixel-7"));
    assert_eq!(with_ab_base(plain.clone(), None).len(), plain.len(), "no base: the field as it was");

    let all = with_ab_base(plain, Some("/base/virtuoso"));
    let virtuoso = all.iter().find(|t| t.name == "virtuoso").unwrap();
    let base = all.iter().find(|t| t.name == AB_BASE).expect("the base build's tool");
    assert_eq!(base.test_argv[0], "/base/virtuoso");
    assert_eq!(base.test_argv[1..], virtuoso.test_argv[1..]);
    assert_eq!(base.version_argv, ["/base/virtuoso", "--version"]);
    assert_eq!(base.install_files, ["/base/virtuoso"]);
    assert_eq!(base.env, virtuoso.env, "the phone field's emulation too");
    assert_eq!(base.flow_variant, virtuoso.flow_variant);
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
fn the_android_fields_browser_is_named_with_its_package_and_version() {
    use virtuoso_bench::tools::android_browser_named;
    let dumpsys = "Packages:\n  Package [org.chromium.chrome] (d4148c3):\n    versionCode=812700079 minSdk=29 targetSdk=36\n    versionName=158.0.8127.0\n";
    assert_eq!(
        android_browser_named("Chrome on a Pixel 7 emulator (API 36, Google APIs)", "org.chromium.chrome", dumpsys),
        "Chrome on a Pixel 7 emulator (API 36, Google APIs): Chromium 158.0.8127.0 (org.chromium.chrome)"
    );
    // The image's own Chrome, a copy for each user: the first versionName is the one it runs.
    let chrome = "    versionName=133.0.6943.137\n    primaryCpuAbi=null\n    versionName=133.0.6943.137\n";
    assert_eq!(android_browser_named("Chrome on a Pixel 7 emulator", "com.android.chrome", chrome), "Chrome on a Pixel 7 emulator: Chrome 133.0.6943.137 (com.android.chrome)");
    // Not installed, or adb not answering: the label as it was.
    assert_eq!(android_browser_named("Chrome on a Pixel 7 emulator", "com.android.chrome", ""), "Chrome on a Pixel 7 emulator");
}

#[test]
fn the_android_fields_browser_is_the_package_virtuoso_android_browser_names_else_chrome() {
    use std::cell::RefCell;
    use virtuoso_bench::tools::android_field_browser;
    let asked = RefCell::new(Vec::new());
    let dumpsys = |pkg: &str| {
        asked.borrow_mut().push(pkg.to_string());
        "    versionName=158.0.8127.0\n".to_string()
    };
    assert_eq!(android_field_browser(Some("Chromium on a Pixel 7"), Some(" org.chromium.chrome "), dumpsys), "Chromium on a Pixel 7: Chromium 158.0.8127.0 (org.chromium.chrome)");
    assert_eq!(android_field_browser(None, Some(""), dumpsys), "Chrome on an Android device: Chrome 158.0.8127.0 (com.android.chrome)");
    assert_eq!(android_field_browser(None, None, dumpsys), "Chrome on an Android device: Chrome 158.0.8127.0 (com.android.chrome)");
    assert_eq!(*asked.borrow(), ["org.chromium.chrome", "com.android.chrome", "com.android.chrome"]);
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
fn from_chrome_158_the_android_field_leaves_maestro_out_and_says_why() {
    use virtuoso_bench::tools::{web_android_tools, without_unreadable};
    let all = || web_android_tools(Path::new("/work/virtuoso"));
    let names = |t: &[virtuoso_bench::orchestrate::Tool]| t.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
    // Chromium 159 (and Chrome 158): an ordinary tab builds no accessibility tree while only
    // UiAutomation is connected, and Maestro's UiAutomation suppresses every accessibility service.
    for version in ["159.0.8154.0", "158.0.8127.93"] {
        let dumpsys = format!("Packages:\n    versionName={version}\n");
        let (tools, why) = without_unreadable(all(), &dumpsys);
        assert_eq!(names(&tools), ["virtuoso", "playwright", "puppeteer", "selenium", "appium"], "{version}");
        let why = why.unwrap_or_else(|| panic!("{version}: a reason"));
        assert!(why.starts_with("maestro"), "{why}");
        for word in ["Chrome 158", "accessibility tree", "accessibility service", "UiAutomation"] {
            assert!(why.contains(word), "{word}: {why}");
        }
    }
    // Chrome 157 and earlier (the image's 133) still build it: Maestro runs.
    for dumpsys in ["    versionName=133.0.6943.137\n", "    versionName=157.0.8103.5\n"] {
        let (tools, why) = without_unreadable(all(), dumpsys);
        assert!(names(&tools).contains(&"maestro".to_string()), "{dumpsys}");
        assert_eq!(why, None);
    }
    // No version (adb not answering, no browser): every tool, as before.
    let (tools, why) = without_unreadable(all(), "");
    assert_eq!(tools.len(), all().len());
    assert_eq!(why, None);
}

#[test]
fn a_run_asks_the_devices_browser_for_its_version_only_in_the_android_web_field() {
    use std::cell::RefCell;
    use virtuoso_bench::tools::field_tools;
    let repo = Path::new("/work/virtuoso");
    let asked = RefCell::new(Vec::new());
    let dumpsys = |pkg: &str| {
        asked.borrow_mut().push(pkg.to_string());
        "    versionName=159.0.8154.0\n".to_string()
    };
    let android_web = |k: &str| match k {
        "VIRTUOSO_BENCH_WEB_DEVICE" => Some("android".to_string()),
        "VIRTUOSO_ANDROID_BROWSER" => Some(" org.chromium.chrome ".to_string()),
        _ => None,
    };
    let (tools, why) = field_tools(repo, &Host::Unix, Platform::Web, &android_web, &dumpsys);
    assert!(!tools.iter().any(|t| t.name == "maestro"));
    assert!(why.is_some_and(|w| w.starts_with("maestro")));
    assert_eq!(*asked.borrow(), ["org.chromium.chrome"]);
    // The image's Chrome when no package is named.
    let chrome = |k: &str| (k == "VIRTUOSO_BENCH_WEB_DEVICE").then(|| "android".to_string());
    field_tools(repo, &Host::Unix, Platform::Web, &chrome, &dumpsys);
    assert_eq!(asked.borrow().last().unwrap(), "com.android.chrome");
    // Elsewhere no device browser is asked about, and every tool runs.
    let none = |_: &str| None;
    for platform in [Platform::Web, Platform::Android, Platform::Ios] {
        let (tools, why) = field_tools(repo, &Host::Unix, platform, &none, &dumpsys);
        assert!(tools.iter().any(|t| t.name == "maestro"), "{platform:?}");
        assert_eq!(why, None);
    }
    assert_eq!(asked.borrow().len(), 2);
}

#[test]
fn the_web_fields_browser_is_the_labels_with_a_devices_version() {
    use virtuoso_bench::tools::web_browser_label;
    let dumpsys = |_: &str| "    versionName=159.0.8154.0\n".to_string();
    let android = |k: &str| match k {
        "VIRTUOSO_BENCH_WEB_DEVICE" => Some("android".to_string()),
        "VIRTUOSO_BENCH_BROWSER" => Some("Chromium on a Pixel 7 emulator".to_string()),
        "VIRTUOSO_ANDROID_BROWSER" => Some("org.chromium.chrome".to_string()),
        _ => None,
    };
    assert_eq!(web_browser_label(&android, &dumpsys), "Chromium on a Pixel 7 emulator: Chromium 159.0.8154.0 (org.chromium.chrome)");
    // A desktop field: the label CI gives, else headless Chrome; no device is asked.
    let never = |_: &str| -> String { panic!("no device to ask") };
    let ci = |k: &str| (k == "VIRTUOSO_BENCH_BROWSER").then(|| "Chrome for Testing 158".to_string());
    assert_eq!(web_browser_label(&ci, &never), "Chrome for Testing 158");
    assert_eq!(web_browser_label(&|_| None, &never), "headless Chrome, 1280 x 800 window");
}

#[test]
fn a_web_run_on_a_device_records_its_browser_and_what_was_not_measured() {
    // The harness itself, with no tools to run, on a device adb cannot reach: the field's label
    // as given (no version to add) and the reason CI gave for a tool left out.
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("r.json");
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso-bench"))
        .current_dir(d.path())
        .args(["run", "--platform", "web", "--device", "no-such-device", "--tools", "none", "--scenarios", "web/login_form", "--iterations", "1", "--version-runs", "1", "--out"])
        .arg(&out)
        .env("VIRTUOSO_BENCH_WEB_DEVICE", "android")
        .env("VIRTUOSO_ANDROID_BROWSER", "org.chromium.chrome")
        .env("VIRTUOSO_BENCH_BROWSER", "Chromium on a phone")
        .env("VIRTUOSO_BENCH_NOT_MEASURED", "speedrun: no WSL 2")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(r["meta"]["Device"], "Chromium on a phone");
    assert_eq!(r["meta"]["Not measured"], "speedrun: no WSL 2");
}

#[test]
fn a_fields_reasons_for_tools_not_measured_are_joined() {
    use virtuoso_bench::tools::not_measured;
    assert_eq!(not_measured(None, None), None);
    assert_eq!(not_measured(Some("speedrun: no WSL 2".into()), None).as_deref(), Some("speedrun: no WSL 2"));
    assert_eq!(not_measured(Some(" ".into()), Some("maestro: no tree".into())).as_deref(), Some("maestro: no tree"));
    assert_eq!(not_measured(Some("speedrun: no WSL 2".into()), Some("maestro: no tree".into())).as_deref(), Some("speedrun: no WSL 2; maestro: no tree"));
}

#[test]
fn the_android_field_drives_chromium_159_by_default_from_chromiums_own_snapshots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = std::fs::read_to_string(root.join("bench/ci/install-android-browser.sh")).unwrap();
    // The field's default build: the x86_64 snapshot at 159's branch point (159.0.8154.0).
    assert!(script.contains("default_position=1739839"), "{script}");
    assert!(script.contains("https://commondatastorage.googleapis.com/chromium-browser-snapshots/AndroidDesktop_x64"));
    // Only Chromium's own snapshot host is fetched from.
    let hosts: Vec<&str> = script.match_indices("https://").map(|(i, _)| script[i..].split(['/', '"', ' ']).nth(2).unwrap_or("")).collect();
    assert!(hosts.iter().all(|h| *h == "commondatastorage.googleapis.com"), "{hosts:?}");
    let wf = std::fs::read_to_string(root.join(".github/workflows/web-android-bench.yml")).unwrap();
    // Every run installs it unless asked for the image's Chrome; the step runs on a push too.
    assert!(wf.contains("install-android-browser.sh download \"${CHROMIUM:-default}\""), "the workflow downloads the default build");
    assert!(!wf.contains("if: inputs.chromium != ''"), "the Chromium step is not skipped when the input is empty");
    assert!(wf.contains("1739839"), "the input's description names the default");
}

#[test]
fn the_emulator_field_keeps_vulkan_on_and_stops_at_once_when_the_browser_cannot_start() {
    // Chromium 159's GPU process needs Vulkan: with the emulator's `-feature -Vulkan` it aborted at
    // every start (45997623615), every tool failed every run, and Playwright's 900 s waits ran
    // the jobs into their 5 h limit.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let wf = std::fs::read_to_string(root.join(".github/workflows/web-android-bench.yml")).unwrap();
    let options = wf.lines().find(|l| l.trim_start().starts_with("EMULATOR_OPTIONS:")).expect("EMULATOR_OPTIONS");
    assert!(!options.contains("-Vulkan"), "{options}");
    // Before any round, the browser is started once: no DevTools socket within its wait and the
    // job ends, saying so, instead of every tool timing out in turn.
    let script = std::fs::read_to_string(root.join("bench/ci/run-web-android.sh")).unwrap();
    let check = script.find("browser_starts ||").expect("the run checks the browser starts");
    assert!(check < script.find("for flow in").unwrap(), "the check comes before the rounds");
    assert!(script.contains("devtools_remote"), "it waits for the DevTools socket");
    assert!(script[check..].lines().next().unwrap().contains("exit 1"), "a browser that cannot start ends the job");
}

#[test]
fn the_emulator_field_keeps_what_the_device_showed_and_its_anrs() {
    // A DevTools screenshot shows only the page: a native window over the browser (an ANR
    // dialog, a first-run screen) is seen only on the device's own screen. Every 15 s the
    // focused window goes beside the memory line, and the screen to outputs/screens; at the end
    // the event log's ANRs and crashes.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = std::fs::read_to_string(root.join("bench/ci/run-web-android.sh")).unwrap();
    assert!(script.contains("mCurrentFocus"), "the focused window is recorded");
    assert!(script.contains("screencap -p > outputs/screens/"), "the device's screen is kept");
    assert!(script.contains("am_anr"), "ANRs are kept");
}

#[test]
fn a_device_adb_cannot_reach_names_no_browser_version() {
    use virtuoso_bench::tools::{android_field_browser, dumpsys_with};
    let missing = Path::new("/no/such/platform-tools/adb");
    assert_eq!(android_field_browser(Some("Chrome on a Pixel 7"), None, dumpsys_with(missing, "emulator-5554")), "Chrome on a Pixel 7");
}
