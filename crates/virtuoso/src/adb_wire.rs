//! The adb server's own protocol over TCP, so a request costs a socket round trip instead of
//! starting `adb` (~60 ms each on Windows). Callers fall back to `adb` when this fails.
//!
//! A request is its length (4 hex digits) and text; the server answers `OKAY`, or `FAIL` with a
//! length-prefixed reason. `host:` requests answer and close; `host:transport:<serial>` points
//! the connection at a device, after which a service such as `shell:` streams its output.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub struct AdbWire {
    port: u16,
}

impl AdbWire {
    /// The adb server this machine's `adb` uses (ANDROID_ADB_SERVER_PORT, else 5037).
    pub fn local() -> AdbWire {
        let port = std::env::var("ANDROID_ADB_SERVER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(5037);
        AdbWire { port }
    }

    pub fn at(port: u16) -> AdbWire {
        AdbWire { port }
    }

    fn connect(&self) -> Result<TcpStream> {
        let addr: SocketAddr = ([127, 0, 0, 1], self.port).into();
        let s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).with_context(|| format!("no adb server on port {}", self.port))?;
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(Duration::from_secs(10)))?;
        Ok(s)
    }

    /// Serials of the devices in `device` state.
    pub fn devices(&self) -> Result<Vec<String>> {
        let mut s = self.connect()?;
        request(&mut s, "host:devices")?;
        let text = read_counted(&mut s)?;
        Ok(text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter(|(_, state)| state.trim() == "device")
            .map(|(serial, _)| serial.to_string())
            .collect())
    }

    /// Forwards local TCP `local` to the device's `remote` (rebinding a forward already there).
    pub fn forward(&self, serial: &str, local: u16, remote: u16) -> Result<()> {
        let mut s = self.connect()?;
        request(&mut s, &format!("host-serial:{serial}:forward:tcp:{local};tcp:{remote}"))?;
        // A forward answers a second time once the listener is bound.
        status(&mut s)
    }

    /// Runs `cmd` in the device's shell; its output, until the command ends.
    pub fn shell(&self, serial: &str, cmd: &str) -> Result<TcpStream> {
        let mut s = self.connect()?;
        request(&mut s, &format!("host:transport:{serial}"))?;
        request(&mut s, &format!("shell:{cmd}"))?;
        s.set_read_timeout(None)?;
        Ok(s)
    }
}

impl AdbWire {
    /// Writes `bytes` to the device's `path` with `mode`, through the sync service: `SEND`,
    /// `DATA` packets of at most 64 KiB, `DONE`; each id followed by a little-endian u32.
    pub fn push(&self, serial: &str, bytes: &[u8], path: &str, mode: u32) -> Result<()> {
        let mut s = self.connect()?;
        request(&mut s, &format!("host:transport:{serial}"))?;
        request(&mut s, "sync:")?;
        // As adb sends it: a regular file's st_mode.
        let target = format!("{path},{}", 0o100000 | mode);
        let mut out = Vec::with_capacity(bytes.len() + target.len() + 64);
        let mut packet = |id: &[u8], n: u32, body: &[u8]| {
            out.extend_from_slice(id);
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(body);
        };
        packet(b"SEND", target.len() as u32, target.as_bytes());
        for chunk in bytes.chunks(64 * 1024) {
            packet(b"DATA", chunk.len() as u32, chunk);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0);
        packet(b"DONE", now, &[]);
        s.write_all(&out)?;
        let mut head = [0u8; 8];
        s.read_exact(&mut head).context("the adb server closed the connection")?;
        let n = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
        match &head[..4] {
            b"OKAY" => {
                let _ = s.write_all(b"QUIT\0\0\0\0");
                Ok(())
            }
            b"FAIL" => {
                let mut why = vec![0u8; n.min(4096)];
                s.read_exact(&mut why)?;
                bail!("adb push: {}", String::from_utf8_lossy(&why))
            }
            other => bail!("adb sync answered {:?}", String::from_utf8_lossy(other)),
        }
    }
}

fn request(s: &mut TcpStream, text: &str) -> Result<()> {
    if text.len() > 0xffff {
        bail!("adb request too long");
    }
    s.write_all(format!("{:04x}{text}", text.len()).as_bytes())?;
    status(s)
}

fn status(s: &mut TcpStream) -> Result<()> {
    let mut st = [0u8; 4];
    s.read_exact(&mut st).context("the adb server closed the connection")?;
    match &st {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(anyhow!("adb: {}", read_counted(s).unwrap_or_default())),
        other => bail!("adb server answered {:?}", String::from_utf8_lossy(other)),
    }
}

fn read_counted(s: &mut TcpStream) -> Result<String> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = usize::from_str_radix(std::str::from_utf8(&len)?, 16)?;
    let mut body = vec![0u8; n];
    s.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).to_string())
}
