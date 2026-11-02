//! Development probe: times agent operations and prints what the agent sees.
//! `cargo run --example probe -- <serial> [windows|type <text>|launch <app>]`

use virtuoso::android::{Adb, AndroidAgent};
use serde_json::json;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let serial = args.first().cloned().unwrap_or_else(|| "emulator-5554".into());
    let adb = Adb::new(&serial);
    let t = Instant::now();
    let mut agent = AndroidAgent::start(&adb)?;
    println!("agent start: {:?}", t.elapsed());
    let c = agent.client();
    match args.get(1).map(String::as_str) {
        Some("type") => {
            let t = Instant::now();
            let r = c.call(json!({"op": "type", "text": args[2]}))?;
            println!("type: {:?} {r}", t.elapsed());
        }
        Some("launch") => {
            let app = &args[2];
            let t = Instant::now();
            let r = c.call(json!({"op": "exec", "argv": ["cmd", "package", "resolve-activity", "--brief", "-c", "android.intent.category.LAUNCHER", app]}))?;
            println!("resolve: {:?} {}", t.elapsed(), r["out"]);
            let t = Instant::now();
            let r = c.call(json!({"op": "exec", "argv": ["dumpsys", "package", app]}))?;
            println!("dumpsys: {:?} {} bytes", t.elapsed(), r["out"].as_str().unwrap_or("").len());
            let t = Instant::now();
            let r = c.call(json!({"op": "exec", "argv": ["am", "force-stop", app]}))?;
            println!("force-stop: {:?} {}", t.elapsed(), r["code"]);
        }
        _ => {}
    }
    for _ in 0..3 {
        let t = Instant::now();
        let s = c.call(json!({"op": "screen"}))?;
        println!("screen: {:?}, {} nodes", t.elapsed(), s["nodes"].as_array().map(|n| n.len()).unwrap_or(0));
        for w in s["windows"].as_array().unwrap() {
            println!("  window {w}");
        }
    }
    let t = Instant::now();
    c.ping()?;
    println!("ping: {:?}", t.elapsed());
    Ok(())
}
