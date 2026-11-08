//! The iOS agent outlives a run (needs VIRTUOSO_IOS_DEVICE): starting one through xcodebuild
//! takes most of half a minute, so the next run reconnects to the one already serving.

use std::time::{Duration, Instant};
use virtuoso::device::Device;
use virtuoso::ios::IosDevice;

#[test]
fn the_next_run_reconnects_to_the_running_agent() {
    let Ok(udid) = std::env::var("VIRTUOSO_IOS_DEVICE") else { return };
    {
        let mut first = IosDevice::connect(&udid).expect("the first run");
        first.screen().expect("a screen read");
    }
    let start = Instant::now();
    let mut next = IosDevice::connect(&udid).expect("the next run");
    let took = start.elapsed();
    next.screen().expect("a screen read on the same agent");
    assert!(took < Duration::from_secs(3), "the next run took {took:?} to connect");
}
