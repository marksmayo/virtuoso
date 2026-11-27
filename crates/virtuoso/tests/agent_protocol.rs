//! The host side of the agent wire protocol, exercised against an in-process fake agent.

use virtuoso::agent::{read_frame, write_frame, AgentClient};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::thread;

/// Starts a fake agent that checks the token, then answers each request with `reply(request)`.
fn fake_agent(token: &'static str, reply: fn(&Value) -> Value) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let hello = read_frame(&mut s).unwrap();
        if hello["token"] != token {
            return;
        }
        while let Ok(req) = read_frame(&mut s) {
            write_frame(&mut s, &reply(&req)).unwrap();
        }
    });
    addr
}

#[test]
fn frames_round_trip_through_a_buffer() {
    let mut buf = Vec::new();
    write_frame(&mut buf, &json!({"op": "ping", "n": 3})).unwrap();
    assert_eq!(&buf[..4], &(buf.len() as u32 - 4).to_be_bytes());
    let back = read_frame(&mut buf.as_slice()).unwrap();
    assert_eq!(back, json!({"op": "ping", "n": 3}));
}

#[test]
fn ping_reaches_an_agent_that_accepts_the_token() {
    let addr = fake_agent("secret", |req| {
        assert_eq!(req["op"], "ping");
        json!({"ok": true, "us": 12})
    });
    let mut client = AgentClient::connect(addr, "secret").unwrap();
    client.ping().unwrap();
}

#[test]
fn an_agent_error_becomes_a_host_error_with_its_message() {
    let addr = fake_agent("t", |_| json!({"ok": false, "error": "no window"}));
    let mut client = AgentClient::connect(addr, "t").unwrap();
    let err = client.call(json!({"op": "screen"})).unwrap_err();
    assert!(err.to_string().contains("no window"), "{err}");
}

#[test]
fn a_wrong_token_fails_instead_of_hanging() {
    let addr = fake_agent("right", |_| json!({"ok": true}));
    let mut client = AgentClient::connect(addr, "wrong").unwrap();
    assert!(client.ping().is_err());
}

#[test]
fn a_raw_call_returns_the_reply_bytes_for_a_typed_parse() {
    let addr = fake_agent("t", |_| json!({"ok": true, "nodes": [{"t": "OK"}]}));
    let mut client = AgentClient::connect(addr, "t").unwrap();
    let body = client.call_raw(&json!({"op": "screen"})).unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["nodes"][0]["t"], "OK");
}

#[test]
fn a_reply_may_bring_raw_bytes_in_a_second_frame() {
    // A screenshot as base64 inside JSON cost the agent ~8 copies of the PNG.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        read_frame(&mut s).unwrap();
        read_frame(&mut s).unwrap();
        write_frame(&mut s, &json!({"ok": true, "blob": 5})).unwrap();
        use std::io::Write;
        s.write_all(&5u32.to_be_bytes()).unwrap();
        s.write_all(b"\x89PNG\x00").unwrap();
    });
    let mut client = AgentClient::connect(addr, "t").unwrap();
    let (reply, blob) = client.call_with_blob(json!({"op": "screenshot"})).unwrap();
    assert_eq!(reply["ok"], true);
    assert_eq!(blob, b"\x89PNG\x00");
}

/// A fake agent serving `connections` connections one after another, as the agent does: each
/// one's token read, then each request answered `{"ok": true, "n": <its n>}`. `drop(i, n)` says
/// whether connection `i` is closed instead of answering the request with that `n` (before the
/// token is read, when it is the first request: as the agent drops a connection that sent no
/// token in time).
fn serial_agent(connections: usize, drop: fn(usize, &Value) -> bool) -> (std::net::SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = accepted.clone();
    thread::spawn(move || {
        for i in 0..connections {
            let Ok((mut s, _)) = listener.accept() else { return };
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if read_frame(&mut s).is_err() {
                continue;
            }
            while let Ok(req) = read_frame(&mut s) {
                if drop(i, &req) {
                    break;
                }
                if write_frame(&mut s, &json!({"ok": true, "n": req["n"]})).is_err() {
                    break;
                }
            }
        }
    });
    (addr, accepted)
}

#[test]
fn a_connection_the_agent_dropped_before_its_first_request_is_made_again_not_reported_lost() {
    // The host connects as soon as the agent is listening and sends the token with its first
    // request, which waits for the first launch over adb: 8-12 s on the Windows runner's
    // emulator. The agent had dropped the connection by then (it gave a token 5 s), and the
    // request read as the agent lost: the agent was restarted (P02 traces, 6 of 36 Windows runs).
    // The agent is alive: a new connection is all it takes.
    let (addr, accepted) = serial_agent(2, |i, _| i == 0);
    let mut client = AgentClient::connect(addr, "t").unwrap();
    client.set_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let reply = client.call(json!({"op": "windows", "n": 1})).unwrap();
    assert_eq!(reply["n"], 1, "{reply}");
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 2);
    // The new connection is the one kept.
    assert_eq!(client.call(json!({"op": "windows", "n": 2})).unwrap()["n"], 2);
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn a_pair_of_requests_on_a_connection_the_agent_dropped_goes_again_on_a_new_one() {
    let (addr, _) = serial_agent(2, |i, _| i == 0);
    let mut client = AgentClient::connect(addr, "t").unwrap();
    client.set_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let (a, b) = client.call_raw_pair(&json!({"op": "tap", "n": 1}), &json!({"op": "settle", "n": 2})).unwrap();
    let (a, b): (Value, Value) = (serde_json::from_slice(&a).unwrap(), serde_json::from_slice(&b).unwrap());
    assert_eq!((a["n"].as_i64(), b["n"].as_i64()), (Some(1), Some(2)));
}

#[test]
fn an_agent_gone_mid_session_is_still_reported_lost() {
    // A connection that already carried requests broke: the agent may have died with the request
    // half done. That is for the caller (which restarts the agent), not a quiet retry here.
    let (addr, accepted) = serial_agent(2, |i, req| i == 0 && req["n"] == 2);
    let mut client = AgentClient::connect(addr, "t").unwrap();
    client.set_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    client.call(json!({"op": "windows", "n": 1})).unwrap();
    assert!(client.call(json!({"op": "windows", "n": 2})).is_err());
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 1, "no new connection was tried");
}

#[test]
fn an_agent_that_drops_the_new_connection_too_is_reported_lost() {
    // Nothing listening behind adb's forward (the agent died) looks the same: every connection is
    // closed at once. One more try, then the error.
    let (addr, accepted) = serial_agent(3, |_, _| true);
    let mut client = AgentClient::connect(addr, "t").unwrap();
    client.set_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let e = client.call(json!({"op": "windows", "n": 1})).unwrap_err();
    assert!(format!("{e:#}").contains("closed the connection"), "{e:#}");
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn after_a_reply_times_out_the_next_call_gets_its_own_reply_not_the_late_one() {
    // An accessibility call that hangs past the host's timeout is still answered, late: read
    // on the same connection, that answer would be taken for the next request's, and every
    // reply after would be one behind (a whole run of device tests failed so).
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        // One connection at a time, each request answered in turn, as the agent serves them.
        for _ in 0..2 {
            let Ok((mut s, _)) = listener.accept() else { return };
            if read_frame(&mut s).is_err() {
                continue;
            }
            while let Ok(req) = read_frame(&mut s) {
                if req["n"] == 1 {
                    thread::sleep(std::time::Duration::from_millis(400));
                }
                if write_frame(&mut s, &json!({"ok": true, "n": req["n"]})).is_err() {
                    break;
                }
            }
        }
    });
    let mut client = AgentClient::connect(addr, "t").unwrap();
    client.set_timeout(Some(std::time::Duration::from_millis(150))).unwrap();
    assert!(client.call(json!({"op": "screen", "n": 1})).is_err(), "the first reply is too late");
    thread::sleep(std::time::Duration::from_millis(400));
    client.set_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let reply = client.call(json!({"op": "screen", "n": 2})).unwrap();
    assert_eq!(reply["n"], 2, "{reply}");
}
