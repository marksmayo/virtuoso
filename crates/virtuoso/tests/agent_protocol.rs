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
