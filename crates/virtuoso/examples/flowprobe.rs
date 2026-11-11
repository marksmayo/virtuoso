//! Development probe: runs a flow file on a device with timings, then prints the screen.
//! `cargo run --example flowprobe -- <serial> <flow.yaml>`

use virtuoso::android::AndroidDevice;
use virtuoso::device::Device;
use virtuoso::flow::parse_flow;
use virtuoso::runner::{RunOptions, Runner};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let t = Instant::now();
    let mut dev = AndroidDevice::connect(&args[0])?;
    dev.disable_animations()?;
    println!("connect: {:?}", t.elapsed());
    let text = std::fs::read_to_string(&args[1])?;
    let flow = parse_flow(&text, Some(std::path::Path::new(&args[1])))?;
    let mut runner = Runner::new(&mut dev, RunOptions::default());
    runner.on_step = Some(Box::new(|s| println!("{:?} {:>8.0?} {} {}", s.status, s.duration, s.name, s.error.clone().unwrap_or_default())));
    let r = runner.run_flow(&flow);
    println!("flow: {:?} in {:?}", r.status, r.duration);
    drop(runner);
    let screen = dev.screen()?;
    for e in screen.elements.iter() {
        if screen.elements.iter().position(|x| std::ptr::eq(x, e)).is_some_and(|i| screen.matchable(i)) && !e.label().is_empty() {
            println!("  {:?} {:?} [{}] {}", e.label(), e.id, e.package, if e.clickable() { "clickable" } else { "" });
        }
    }
    Ok(())
}
