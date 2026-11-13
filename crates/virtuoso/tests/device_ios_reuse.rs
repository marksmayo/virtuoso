//! The iOS agent outlives a run (needs VIRTUOSO_IOS_DEVICE): starting one through xcodebuild
//! takes most of half a minute, so the next run reconnects to the one already serving.

use std::sync::Mutex;
use std::time::{Duration, Instant};
use virtuoso::device::Device;
use virtuoso::ios::IosDevice;

/// The agent serves one connection at a time: these tests take turns.
static ONE: Mutex<()> = Mutex::new(());

#[test]
fn the_next_run_reconnects_to_the_running_agent() {
    let Ok(udid) = std::env::var("VIRTUOSO_IOS_DEVICE") else { return };
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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

#[test]
fn a_host_gone_mid_reply_leaves_the_agent_serving() {
    // A write to a connection the host closed (a timeout, Ctrl-C) is an error the agent drops
    // the connection on, not a SIGPIPE that ends its process (and the next run's xcodebuild
    // start, most of half a minute).
    use std::net::{SocketAddr, TcpStream};
    use virtuoso::agent::{write_frame, AgentClient};
    let Ok(udid) = std::env::var("VIRTUOSO_IOS_DEVICE") else { return };
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    // An agent serving, and no run on it.
    IosDevice::connect(&udid).expect("an agent").screen().expect("a screen read");
    let state = std::env::temp_dir().join(format!("virtuoso-ios-agent-{udid}.json"));
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&state).expect("the agent's state")).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], saved["port"].as_u64().unwrap() as u16));
    let token = saved["token"].as_str().unwrap();
    for _ in 0..3 {
        // A screenshot's reply is megabytes: its writes go on after the host has gone.
        let mut gone = TcpStream::connect(addr).unwrap();
        write_frame(&mut gone, &serde_json::json!({ "token": token })).unwrap();
        write_frame(&mut gone, &serde_json::json!({ "op": "screenshot" })).unwrap();
        drop(gone);
        std::thread::sleep(Duration::from_millis(500));
    }
    let mut client = AgentClient::connect(addr, token).expect("the agent still listens");
    client.set_timeout(Some(Duration::from_secs(10))).unwrap();
    client.ping().expect("the agent still answers");
}
