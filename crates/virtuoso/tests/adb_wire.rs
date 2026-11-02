//! The adb server protocol, against a fake server.

use virtuoso::adb_wire::AdbWire;
use std::io::{Read, Write};
use std::net::TcpListener;

/// A fake adb server: each connection reads requests and answers from `script` (request
/// prefix, answer bytes). Returns its port and the requests it saw.
fn fake_server(script: Vec<(&'static str, Vec<u8>)>, connections: usize) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..connections {
            let (mut s, _) = l.accept().unwrap();
            loop {
                let mut len = [0u8; 4];
                if s.read_exact(&mut len).is_err() {
                    break;
                }
                let n = usize::from_str_radix(std::str::from_utf8(&len).unwrap(), 16).unwrap();
                let mut req = vec![0u8; n];
                s.read_exact(&mut req).unwrap();
                let req = String::from_utf8(req).unwrap();
                let answer = script.iter().find(|(p, _)| req.starts_with(p)).map(|(_, a)| a.clone()).unwrap_or_else(|| b"FAIL0007unknown".to_vec());
                seen.push(req.clone());
                s.write_all(&answer).unwrap();
                // A service that streams (shell) or a one-shot host request ends the connection.
                if !req.starts_with("host:transport:") {
                    break;
                }
            }
        }
        seen
    });
    (port, h)
}

#[test]
fn devices_lists_the_serials_in_device_state() {
    let payload = "emulator-5554\tdevice\nR58M\tunauthorized\n";
    let (port, server) = fake_server(vec![("host:devices", format!("OKAY{:04x}{payload}", payload.len()).into_bytes())], 1);
    assert_eq!(AdbWire::at(port).devices().unwrap(), vec!["emulator-5554"]);
    assert_eq!(server.join().unwrap(), vec!["host:devices"]);
}

#[test]
fn forward_asks_the_server_for_that_device() {
    let (port, server) = fake_server(vec![("host-serial:", b"OKAYOKAY".to_vec())], 1);
    AdbWire::at(port).forward("emulator-5554", 21234, 7999).unwrap();
    assert_eq!(server.join().unwrap(), vec!["host-serial:emulator-5554:forward:tcp:21234;tcp:7999"]);
}

#[test]
fn a_refused_request_is_an_error_with_the_servers_reason() {
    let (port, _server) = fake_server(vec![("host-serial:", b"FAIL0014cannot bind listener".to_vec())], 1);
    let e = AdbWire::at(port).forward("emulator-5554", 21234, 7999).unwrap_err();
    assert!(e.to_string().contains("cannot bind"), "{e}");
}

#[test]
fn shell_streams_the_commands_output() {
    let (port, server) = fake_server(vec![("host:transport:", b"OKAY".to_vec()), ("shell:", b"OKAYhello\nworld\n".to_vec())], 1);
    let mut stream = AdbWire::at(port).shell("emulator-5554", "echo hello; echo world").unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).unwrap();
    assert_eq!(out, "hello\nworld\n");
    assert_eq!(server.join().unwrap(), vec!["host:transport:emulator-5554", "shell:echo hello; echo world"]);
}

#[test]
fn no_server_is_an_error_not_a_hang() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(AdbWire::at(port).devices().is_err());
}

#[test]
fn push_sends_bytes_from_memory_in_sync_packets() {
    // The agent jar went through a temp file and `adb push` (~50 ms of CPU on Windows).
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let req = |s: &mut std::net::TcpStream| {
            let mut len = [0u8; 4];
            s.read_exact(&mut len).unwrap();
            let mut r = vec![0u8; usize::from_str_radix(std::str::from_utf8(&len).unwrap(), 16).unwrap()];
            s.read_exact(&mut r).unwrap();
            s.write_all(b"OKAY").unwrap();
            String::from_utf8(r).unwrap()
        };
        let transport = req(&mut s);
        let sync = req(&mut s);
        let packet = |s: &mut std::net::TcpStream| {
            let mut head = [0u8; 8];
            s.read_exact(&mut head).unwrap();
            let n = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
            let id = String::from_utf8(head[..4].to_vec()).unwrap();
            if id == "DONE" {
                return (id, Vec::new());
            }
            let mut body = vec![0u8; n];
            s.read_exact(&mut body).unwrap();
            (id, body)
        };
        let (send, target) = packet(&mut s);
        let mut data = Vec::new();
        let mut chunks = 0;
        loop {
            let (id, body) = packet(&mut s);
            if id == "DONE" {
                break;
            }
            assert_eq!(id, "DATA");
            assert!(body.len() <= 64 * 1024);
            chunks += 1;
            data.extend(body);
        }
        s.write_all(b"OKAY\0\0\0\0").unwrap();
        (transport, sync, send, String::from_utf8(target).unwrap(), data, chunks)
    });
    let bytes: Vec<u8> = (0..150_000u32).map(|i| i as u8).collect();
    AdbWire::at(port).push("emulator-5554", &bytes, "/data/local/tmp/a.jar", 0o644).unwrap();
    let (transport, sync, send, target, data, chunks) = server.join().unwrap();
    assert_eq!(transport, "host:transport:emulator-5554");
    assert_eq!(sync, "sync:");
    assert_eq!(send, "SEND");
    assert_eq!(target, "/data/local/tmp/a.jar,33188");
    assert_eq!(chunks, 3);
    assert_eq!(data, bytes);
}
