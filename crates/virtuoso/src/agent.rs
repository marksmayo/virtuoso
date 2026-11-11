//! The connection to the on-device agent.
//!
//! Wire format, both directions: `u32` big-endian length, then that many bytes of UTF-8 JSON.
//! The first frame the host sends is `{"token": "<hex>"}`; the agent drops a connection whose
//! token is wrong. Every reply carries `ok`, and `error` when `ok` is false. A reply with
//! `blob: n` is followed by one more frame of `n` raw bytes (a screenshot's PNG).

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Frames over 64 MiB are refused: no agent reply is that big, so it is a corrupt stream.
const MAX_FRAME: usize = 64 << 20;

pub fn write_frame(w: &mut impl Write, v: &Value) -> Result<()> {
    w.write_all(&frame(v)?)?;
    w.flush()?;
    Ok(())
}

/// A value framed: written once into one buffer, its length put in front after.
fn frame(v: &Value) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; 4];
    serde_json::to_writer(&mut buf, v)?;
    let len = (buf.len() - 4) as u32;
    buf[..4].copy_from_slice(&len.to_be_bytes());
    Ok(buf)
}

pub fn read_frame(r: &mut impl Read) -> Result<Value> {
    Ok(serde_json::from_slice(&read_body(r)?)?)
}

/// One frame's bytes.
pub fn read_body(r: &mut impl Read) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        bail!("agent frame of {len} bytes is too large");
    }
    // Read into the buffer as it grows: no zeroing of it first.
    let mut body = Vec::with_capacity(len);
    r.take(len as u64).read_to_end(&mut body)?;
    if body.len() < len {
        bail!("the agent closed the connection {} bytes into a {len}-byte frame", body.len());
    }
    Ok(body)
}

pub struct AgentClient {
    stream: TcpStream,
    /// Replies are read through a buffer: a frame's length and body in one read.
    reader: std::io::BufReader<TcpStream>,
    addr: SocketAddr,
    token: String,
    timeout: Option<Duration>,
    /// A request on this connection went unanswered (timed out, or broke off): its answer may
    /// still come, and would be read as the next one's. The next request goes on a new one.
    broken: bool,
    /// The token frame, sent with the first request (one write, not two).
    hello: Option<Vec<u8>>,
}

impl AgentClient {
    pub fn connect(addr: SocketAddr, token: &str) -> Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .with_context(|| format!("connecting to the agent at {addr}"))?;
        stream.set_nodelay(true)?;
        let reader = std::io::BufReader::with_capacity(1 << 16, stream.try_clone()?);
        let hello = Some(frame(&json!({ "token": token }))?);
        Ok(AgentClient { stream, reader, addr, token: token.to_string(), timeout: None, broken: false, hello })
    }

    /// A new connection in place of a broken one.
    fn reconnect(&mut self) -> Result<()> {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        let mut fresh = AgentClient::connect(self.addr, &self.token)?;
        fresh.set_timeout(self.timeout)?;
        (self.stream, self.reader, self.broken, self.hello) = (fresh.stream, fresh.reader, false, fresh.hello);
        Ok(())
    }

    /// Sends one request and returns the reply, or the agent's error.
    pub fn call(&mut self, req: Value) -> Result<Value> {
        let reply: Value = serde_json::from_slice(&self.call_raw(&req)?)?;
        if reply["ok"] == true {
            Ok(reply)
        } else {
            Err(anyhow!(
                "agent: {}",
                reply["error"].as_str().unwrap_or("request failed")
            ))
        }
    }

    /// Sends one request whose reply brings raw bytes in a second frame.
    pub fn call_with_blob(&mut self, req: Value) -> Result<(Value, Vec<u8>)> {
        let reply = self.call(req)?;
        let blob = if reply["blob"].is_u64() {
            read_body(&mut self.reader).inspect_err(|_| self.broken = true)?
        } else {
            Vec::new()
        };
        Ok((reply, blob))
    }

    /// Sends one request and returns the reply's bytes, unchecked: for a reply big enough that
    /// the caller parses it straight into its own types.
    pub fn call_raw(&mut self, req: &Value) -> Result<Vec<u8>> {
        if self.broken {
            self.reconnect().with_context(|| format!("reconnecting to the agent for {}", req["op"]))?;
        }
        let r = self
            .send(req)
            .and_then(|_| read_body(&mut self.reader))
            .with_context(|| format!("the agent closed the connection during {}", req["op"]));
        self.broken = r.is_err();
        r
    }

    /// A request, after the token frame if it has not gone yet.
    fn send(&mut self, req: &Value) -> Result<()> {
        let mut out = self.hello.take().unwrap_or_default();
        out.extend_from_slice(&frame(req)?);
        self.stream.write_all(&out)?;
        Ok(())
    }

    /// Bounds how long a single reply may take; `None` waits forever.
    pub fn set_timeout(&mut self, t: Option<Duration>) -> Result<()> {
        // On the handle replies are read through: on Windows a cloned socket handle has its own
        // (on Unix the two share the socket, and its option).
        if cfg!(windows) {
            self.reader.get_ref().set_read_timeout(t)?;
        }
        self.stream.set_read_timeout(t)?;
        self.timeout = t;
        Ok(())
    }

    /// Tells the agent to exit, without waiting for it.
    pub fn quit(&mut self) {
        let _ = self.send(&json!({ "op": "quit" }));
    }

    pub fn ping(&mut self) -> Result<()> {
        self.call(json!({ "op": "ping" })).map(|_| ())
    }
}
