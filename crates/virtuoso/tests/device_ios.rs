//! iOS behaviour on a booted simulator (needs VIRTUOSO_IOS_DEVICE, its UDID, and
//! VIRTUOSO_IOS_AGENT, the agent's .xctestrun built from agent/ios).

use virtuoso::device::{Device, LaunchOptions};
use virtuoso::ios::IosDevice;

const SETTINGS: &str = "com.apple.Preferences";

fn device() -> Option<IosDevice> {
    let udid = std::env::var("VIRTUOSO_IOS_DEVICE").ok()?;
    Some(IosDevice::connect(&udid).expect("connect to the simulator"))
}

fn shows(d: &mut IosDevice, label: &str) -> bool {
    d.screen().unwrap().elements.iter().any(|e| e.label() == label)
}

#[test]
fn reads_the_screen_of_a_launched_app() {
    let Some(mut d) = device() else { return };
    d.launch_app(SETTINGS, &LaunchOptions { stop_app: true, ..Default::default() }).unwrap();
    assert!(shows(&mut d, "General"), "Settings' main screen lists General");
}
