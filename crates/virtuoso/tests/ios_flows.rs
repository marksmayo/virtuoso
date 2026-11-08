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
    assert!(text.contains("5/5 Flows Passed"), "{text}");
}
