//! A WebSocket client for Chrome's DevTools Protocol: text messages over a plain TCP connection
//! to the browser on this machine (no TLS, no extensions). Small on purpose: a library for this
//! would take most of the binary's size budget.

use anyhow::{anyhow, bail, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct WebSocket {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    /// The next frame's mask: any 4 bytes do for a client (they hide nothing on localhost).
    mask: u32,
}

impl WebSocket {
    /// Connects to `url` (`ws://host:port/path`) and makes the upgrade.
    pub fn connect(url: &str) -> Result<WebSocket> {
        let rest = url.strip_prefix("ws://").ok_or_else(|| anyhow!("not a ws:// URL: {url}"))?;
        let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
        let writer = TcpStream::connect(socket_addr(host)?)?;
        writer.set_nodelay(true)?;
        let mut reader = BufReader::with_capacity(1 << 16, writer.try_clone()?);
        let mut mask = seed();
        let key: Vec<u8> = (0..4).flat_map(|_| next_mask(&mut mask).to_be_bytes()).collect();
        let key = base64(&key);
        let mut writer = writer;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        writer.write_all(request.as_bytes())?;
        let mut status = String::new();
        reader.read_line(&mut status)?;
        if !status.starts_with("HTTP/1.1 101") {
            bail!("the browser refused the WebSocket upgrade: {}", status.trim());
        }
        // The rest of the reply's headers; the accept hash is not checked (the browser is local).
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                bail!("the connection closed during the upgrade");
            }
            if line == "\r\n" {
                break;
            }
        }
        Ok(WebSocket { reader, writer, mask })
    }

    /// No message within `t` is an error (`None`: wait for ever).
    pub fn set_read_timeout(&mut self, t: Option<Duration>) -> Result<()> {
        self.reader.get_ref().set_read_timeout(t)?;
        Ok(())
    }

    pub fn send_text(&mut self, text: &str) -> Result<()> {
        let m = next_mask(&mut self.mask).to_be_bytes();
        self.writer.write_all(&encode_frame(1, text.as_bytes(), m))?;
        Ok(())
    }

    /// The next text message; pings are answered on the way.
    pub fn recv_text(&mut self) -> Result<String> {
        read_message(&mut self.reader, &mut self.writer, &mut self.mask)
    }

    /// The next text message, or `None` when none starts within `t`. Only the wait for its first
    /// byte has the timeout: a read cut off inside a frame would lose the stream's place.
    pub fn recv_text_within(&mut self, t: Duration) -> Result<Option<String>> {
        if self.reader.buffer().is_empty() {
            self.reader.get_ref().set_read_timeout(Some(t.max(Duration::from_millis(1))))?;
            let filled = self.reader.fill_buf().map(|b| !b.is_empty());
            self.reader.get_ref().set_read_timeout(None)?;
            match filled {
                Ok(true) => {}
                Ok(false) => bail!("the browser closed the connection"),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
        self.recv_text().map(Some)
    }
}

/// One frame from a client: FIN, `opcode`, the masked payload with its length.
fn encode_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 14);
    f.push(0x80 | opcode);
    match payload.len() {
        n if n < 126 => f.push(0x80 | n as u8),
        n if n < 65536 => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// `host:port` as an address without a name resolver (the binary links none): an IP, or
/// localhost (the loopback).
fn socket_addr(host: &str) -> Result<std::net::SocketAddr> {
    let host = match host.strip_prefix("localhost:") {
        Some(port) => format!("127.0.0.1:{port}"),
        None => host.to_string(),
    };
    host.parse().map_err(|_| anyhow!("connect to a browser by its IP and port (127.0.0.1:9222), not {host}"))
}

/// The next mask (xorshift32): any bytes do, so long as they change.
fn next_mask(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// A mask seed that differs between connections.
fn seed() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() ^ d.as_secs() as u32).unwrap_or(1);
    t | 1
}

/// Base64 (the standard alphabet, padded or not) as bytes; characters outside it are skipped.
pub fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// The next message from `r`: its frames put together, a ping answered on `w` with a pong.
fn read_message<R: BufRead, W: Write>(r: &mut R, w: &mut W, mask: &mut u32) -> Result<String> {
    let mut message = Vec::new();
    loop {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)?;
        let (fin, opcode, masked) = (h[0] & 0x80 != 0, h[0] & 0x0f, h[1] & 0x80 != 0);
        let len = match h[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                r.read_exact(&mut b)?;
                u16::from_be_bytes(b) as usize
            }
            127 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b)?;
                u64::from_be_bytes(b) as usize
            }
            n => n as usize,
        };
        let mut key = [0u8; 4];
        if masked {
            r.read_exact(&mut key)?;
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        if masked {
            payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]);
        }
        match opcode {
            // Text, or a continuation of it.
            0 | 1 => {
                message.extend_from_slice(&payload);
                if fin {
                    return String::from_utf8(message).map_err(|_| anyhow!("the browser sent a message that is not UTF-8"));
                }
            }
            8 => bail!("the browser closed the connection"),
            9 => {
                // A pong with the ping's payload.
                w.write_all(&encode_frame(10, &payload, next_mask(mask).to_be_bytes()))?;
            }
            // A pong, or a binary frame (the protocol sends none): skipped.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};
    use std::net::TcpListener;

    /// A frame as Chrome sends one: unmasked.
    fn server_frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![(if fin { 0x80 } else { 0 }) | opcode];
        match payload.len() {
            n if n < 126 => f.push(n as u8),
            n if n < 65536 => {
                f.push(126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                f.push(127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(payload);
        f
    }

    fn unmask(frame: &[u8], header: usize) -> Vec<u8> {
        let key = &frame[header..header + 4];
        frame[header + 4..].iter().enumerate().map(|(i, b)| b ^ key[i % 4]).collect()
    }

    #[test]
    fn base64_decodes_what_it_encodes() {
        for b in [&b""[..], b"f", b"fo", b"foo", b"foob", &[0, 255, 128, 7, 9][..]] {
            assert_eq!(base64_decode(&base64(b)), b, "{:?}", base64(b));
        }
        assert_eq!(base64_decode("aGVs\nbG8="), b"hello", "a line break skipped");
    }

    #[test]
    fn a_short_text_frame_is_masked_and_carries_its_length() {
        let f = encode_frame(1, b"hello", [1, 2, 3, 4]);
        assert_eq!(&f[..6], &[0x81, 0x80 | 5, 1, 2, 3, 4]);
        assert_eq!(unmask(&f, 2), b"hello");
    }

    #[test]
    fn a_message_of_126_to_65535_bytes_takes_a_16_bit_length() {
        let p = vec![b'x'; 300];
        let f = encode_frame(1, &p, [9, 8, 7, 6]);
        assert_eq!(&f[..4], &[0x81, 0x80 | 126, 0x01, 0x2c]);
        assert_eq!(unmask(&f, 4), p);
    }

    #[test]
    fn a_longer_message_takes_a_64_bit_length() {
        let p = vec![b'y'; 70_000];
        let f = encode_frame(1, &p, [0, 0, 0, 0]);
        assert_eq!(&f[..2], &[0x81, 0x80 | 127]);
        assert_eq!(&f[2..10], &70_000u64.to_be_bytes());
        assert_eq!(f.len(), 10 + 4 + 70_000);
    }

    #[test]
    fn a_message_in_fragments_is_put_back_together() {
        let mut bytes = server_frame(false, 1, b"{\"id\":");
        bytes.extend(server_frame(false, 0, b"1,"));
        bytes.extend(server_frame(true, 0, b"\"result\":{}}"));
        let mut out = Vec::new();
        let m = read_message(&mut Cursor::new(bytes), &mut out, &mut 0).unwrap();
        assert_eq!(m, "{\"id\":1,\"result\":{}}");
        assert!(out.is_empty());
    }

    #[test]
    fn a_ping_is_answered_with_a_pong_and_the_message_after_it_read() {
        let mut bytes = server_frame(true, 9, b"pp");
        bytes.extend(server_frame(true, 1, b"after"));
        let mut out = Vec::new();
        let m = read_message(&mut Cursor::new(bytes), &mut out, &mut 0).unwrap();
        assert_eq!(m, "after");
        assert_eq!(out[0], 0x8a, "a pong");
        assert_eq!(unmask(&out, 2), b"pp", "with the ping's payload");
    }

    #[test]
    fn a_close_from_the_browser_is_an_error() {
        let bytes = server_frame(true, 8, &[0x03, 0xe8]);
        let e = read_message(&mut Cursor::new(bytes), &mut Vec::new(), &mut 0).unwrap_err();
        assert!(e.to_string().contains("closed"), "{e}");
    }

    #[test]
    fn a_large_message_is_read_whole() {
        let p = "z".repeat(200_000);
        let bytes = server_frame(true, 1, p.as_bytes());
        assert_eq!(read_message(&mut Cursor::new(bytes), &mut Vec::new(), &mut 0).unwrap().len(), 200_000);
    }

    /// A server on a free port that reads the upgrade request, answers `reply`, then sends
    /// `then` and echoes the first text frame it gets back as a text frame.
    fn server(reply: &'static str, then: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                request.push_str(&line);
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            s.write_all(reply.as_bytes()).unwrap();
            if !reply.starts_with("HTTP/1.1 101") {
                return request;
            }
            s.write_all(&then).unwrap();
            // Echo one client frame back unmasked.
            let mut h2 = [0u8; 2];
            if r.read_exact(&mut h2).is_ok() {
                let len = (h2[1] & 0x7f) as usize;
                let mut key = [0u8; 4];
                r.read_exact(&mut key).unwrap();
                let mut p = vec![0u8; len];
                r.read_exact(&mut p).unwrap();
                let p: Vec<u8> = p.iter().enumerate().map(|(i, b)| b ^ key[i % 4]).collect();
                s.write_all(&server_frame(true, 1, &p)).unwrap();
            }
            request
        });
        (format!("ws://127.0.0.1:{port}/devtools/browser/abc"), h)
    }

    const UPGRADED: &str = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: x\r\n\r\n";

    #[test]
    fn the_upgrade_asks_for_a_websocket_and_messages_go_both_ways() {
        let (url, h) = server(UPGRADED, server_frame(true, 1, b"hi"));
        let mut ws = WebSocket::connect(&url).unwrap();
        ws.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        assert_eq!(ws.recv_text().unwrap(), "hi");
        ws.send_text("{\"id\":1}").unwrap();
        assert_eq!(ws.recv_text().unwrap(), "{\"id\":1}");
        let request = h.join().unwrap();
        assert!(request.starts_with("GET /devtools/browser/abc HTTP/1.1\r\n"), "{request}");
        for header in ["Upgrade: websocket", "Connection: Upgrade", "Sec-WebSocket-Version: 13", "Sec-WebSocket-Key: ", "Host: 127.0.0.1:"] {
            assert!(request.contains(header), "{header} in {request}");
        }
    }

    #[test]
    fn a_wait_with_a_timeout_returns_none_when_nothing_comes_and_the_message_when_it_does() {
        let (url, h) = server(UPGRADED, vec![]);
        let mut ws = WebSocket::connect(&url).unwrap();
        let start = std::time::Instant::now();
        assert!(ws.recv_text_within(Duration::from_millis(100)).unwrap().is_none());
        assert!(start.elapsed() >= Duration::from_millis(90));
        ws.send_text("echo me").unwrap();
        assert_eq!(ws.recv_text_within(Duration::from_secs(5)).unwrap().as_deref(), Some("echo me"));
        h.join().unwrap();
    }

    #[test]
    fn a_refused_upgrade_is_an_error() {
        let (url, h) = server("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n", vec![]);
        let e = WebSocket::connect(&url).err().expect("refused");
        assert!(e.to_string().contains("404"), "{e}");
        h.join().unwrap();
    }

    #[test]
    fn localhost_is_the_loopback_and_other_names_are_refused_unresolved() {
        let (url, h) = server(UPGRADED, server_frame(true, 1, b"hi"));
        let mut ws = WebSocket::connect(&url.replace("127.0.0.1", "localhost")).unwrap();
        assert_eq!(ws.recv_text_within(Duration::from_secs(5)).unwrap().as_deref(), Some("hi"));
        ws.send_text("x").unwrap();
        h.join().unwrap();
        let e = WebSocket::connect("ws://example.com:9222/devtools/browser/x").err().expect("a name");
        assert!(e.to_string().contains("IP"), "{e}");
    }

    #[test]
    fn a_url_that_is_not_ws_is_an_error() {
        assert!(WebSocket::connect("http://127.0.0.1:1/x").is_err());
        assert!(WebSocket::connect("wss://127.0.0.1:1/x").is_err());
    }
}
