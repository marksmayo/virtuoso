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
    let body = serde_json::to_vec(v)?;
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf)?;
    w.flush()?;
    Ok(())
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
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
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
}

impl AgentClient {
    pub fn connect(addr: SocketAddr, token: &str) -> Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .with_context(|| format!("connecting to the agent at {addr}"))?;
        stream.set_nodelay(true)?;
        let reader = std::io::BufReader::with_capacity(1 << 16, stream.try_clone()?);
        let mut client = AgentClient { stream, reader, addr, token: token.to_string(), timeout: None, broken: false };
        write_frame(&mut client.stream, &json!({ "token": token }))?;
        Ok(client)
    }

    /// A new connection in place of a broken one.
    fn reconnect(&mut self) -> Result<()> {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        let mut fresh = AgentClient::connect(self.addr, &self.token)?;
        fresh.set_timeout(self.timeout)?;
        (self.stream, self.reader, self.broken) = (fresh.stream, fresh.reader, false);
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
        let r = write_frame(&mut self.stream, req)
            .and_then(|_| read_body(&mut self.reader))
            .with_context(|| format!("the agent closed the connection during {}", req["op"]));
        self.broken = r.is_err();
        r
    }

    /// Bounds how long a single reply may take; `None` waits forever.
    pub fn set_timeout(&mut self, t: Option<Duration>) -> Result<()> {
        // On the handle replies are read through: on Windows a cloned socket handle has its own.
        self.reader.get_ref().set_read_timeout(t)?;
        self.stream.set_read_timeout(t)?;
        self.timeout = t;
        Ok(())
    }

    /// Tells the agent to exit, without waiting for it.
    pub fn quit(&mut self) {
        let _ = write_frame(&mut self.stream, &json!({ "op": "quit" }));
    }

    pub fn ping(&mut self) -> Result<()> {
        self.call(json!({ "op": "ping" })).map(|_| ())
    }
}
