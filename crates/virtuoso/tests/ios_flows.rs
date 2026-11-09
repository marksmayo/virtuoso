//! Maestro flows on an iOS simulator, through the `virtuoso` command (needs
//! VIRTUOSO_IOS_DEVICE; the fixture app installed). Each flow in
//! tests/ios_flows covers a group of commands.

use std::process::Command;

#[test]
fn the_ios_flows_pass_on_a_simulator() {
    let Ok(udid) = std::env::var("VIRTUOSO_IOS_DEVICE") else { return };
    let flows = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ios_flows");
    // Kept after the run (CI uploads it): a failed flow's screenshot and hierarchy.
    let out = std::env::temp_dir().join("ios-flows-output");
    let o = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .args(["--platform", "ios", "--device", &udid, "test", flows, "--no-ansi", "--test-output-dir"])
        .arg(&out)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success(), "{text}");
    assert!(text.contains("6/6 Flows Passed"), "{text}");
}

/// The benchmark's iOS Settings scenarios (bench/flows-ios) pass, each from a cleared Settings
/// as the benchmark's reset leaves it: they must, before they time anyone.
#[test]
fn the_ios_benchmark_flows_pass_on_a_simulator() {
    let Ok(udid) = std::env::var("VIRTUOSO_IOS_DEVICE") else { return };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/flows-ios");
    let mut failed = Vec::new();
    for flow in ["launch", "navigate", "scroll", "input", "assertions", "taps", "trivial"] {
        let clear = format!(
            "xcrun simctl terminate {udid} com.apple.Preferences; d=$(xcrun simctl get_app_container {udid} com.apple.Preferences data) && rm -rf \"$d/Library\"/* \"$d/Documents\"/* \"$d/tmp\"/*"
        );
        let _ = Command::new("sh").args(["-c", &clear]).output();
        let o = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
            .args(["--platform", "ios", "--device", &udid, "test", "--no-ansi", "--test-output-dir"])
            // Kept (CI uploads it): a failed flow's screenshot and hierarchy.
            .arg(std::env::temp_dir().join("ios-flows-output").join(format!("bench-{flow}")))
            .arg(dir.join(format!("{flow}.yaml")))
            .output()
            .unwrap();
        if !o.status.success() {
            failed.push(format!("{flow}: {}", String::from_utf8_lossy(&o.stdout).lines().filter(|l| l.contains("FAILED") || l.starts_with("    ")).collect::<Vec<_>>().join(" | ")));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The Speedrun Bench app's flows (speedrun's bench/app/flows, in VIRTUOSO_SPEEDRUN_APP_FLOWS;
/// the app installed) pass on iOS, each from the benchmark's reset, as speedrun passes them.
/// Not the two that fail for every tool on a simulator (appearance, inputs), nor device: on CI
/// the simulator's media import (`simctl addmedia`, every tool's way in) now and then never
/// answers, in some runs every try.
#[test]
fn the_speedrun_app_flows_pass_on_a_simulator() {
    let (Ok(udid), Ok(dir)) = (std::env::var("VIRTUOSO_IOS_DEVICE"), std::env::var("VIRTUOSO_SPEEDRUN_APP_FLOWS")) else { return };
    // A comma-separated subset, for a quicker try.
    let only = std::env::var("VIRTUOSO_SPEEDRUN_APP_ONLY").unwrap_or_default();
    let mut failed = Vec::new();
    for flow in [
        "animations", "cart", "chat", "deep_links", "drag", "feed", "gestures", "native", "onboarding", "popups", "profile", "remember_me",
        "scripting", "selectors", "shop_checkout", "sign_in", "timing",
    ] {
        if !only.is_empty() && !only.split(',').any(|f| f == flow) {
            continue;
        }
        let app = "dev.speedrun.bench";
        let reset = format!(
            "xcrun simctl terminate {udid} {app}; d=$(xcrun simctl get_app_container {udid} {app} data) && rm -rf \"$d/Documents\"/* \"$d/Library\"/* \"$d/tmp\"/*; xcrun simctl ui {udid} appearance light"
        );
        let _ = Command::new("sh").args(["-c", &reset]).output();
        let o = Command::new(env!("CARGO_BIN_EXE_virtuoso"))
            .args(["--platform", "ios", "--device", &udid, "test", "--no-ansi", "--test-output-dir"])
            // Kept (CI uploads it): a failed flow's screenshot and hierarchy.
            .arg(std::env::temp_dir().join("ios-flows-output").join(format!("app-{flow}")))
            .arg(std::path::Path::new(&dir).join(format!("{flow}.yaml")))
            .output()
            .unwrap();
        if !o.status.success() {
            // The steps up to the failure, with their times.
            let out = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
            failed.push(format!("{flow}:\n{}", lines[lines.len().saturating_sub(25)..].join("\n")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}
