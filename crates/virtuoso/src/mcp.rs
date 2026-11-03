//! `virtuoso mcp`: a Model Context Protocol server over stdio, so an AI agent can look at the
//! device's screen and drive the app with flow commands, one call at a time. The device stays
//! connected between calls; it is connected on the first call that needs it.
//!
//! Tools: `look` (what is on screen, a line per element), `run` (flow commands as YAML, then
//! each step's result and the screen after), `screenshot` (a PNG).

use crate::device::Device;
use crate::flow::parse_flow;
use crate::runner::{RunOptions, Runner, StepStatus};
use crate::screen::ScreenTree;
use anyhow::Result;
use serde_json::{json, Value};
use std::time::Duration;

pub type DeviceFactory = Box<dyn FnMut() -> Result<Box<dyn Device>>>;

pub struct McpServer {
    connect: DeviceFactory,
    device: Option<Box<dyn Device>>,
    app_id: Option<String>,
}

const PROTOCOL_VERSION: &str = "2025-06-18";

impl McpServer {
    pub fn new(connect: DeviceFactory) -> Self {
        McpServer { connect, device: None, app_id: None }
    }

    /// The reply to one JSON-RPC message; notifications get none.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?.clone();
        let method = msg["method"].as_str().unwrap_or("");
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "virtuoso", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Drive an Android app: `look` to see the screen, `run` to execute Maestro flow commands (tapOn, inputText, assertVisible, swipe...), `screenshot` for a picture.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => Ok(self.call_tool(&msg["params"])),
            _ => Err((-32601, format!("method {method} is not supported"))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        })
    }

    fn device(&mut self) -> Result<&mut Box<dyn Device>> {
        if self.device.is_none() {
            self.device = Some((self.connect)()?);
        }
        Ok(self.device.as_mut().unwrap())
    }

    fn call_tool(&mut self, params: &Value) -> Value {
        let args = &params["arguments"];
        let outcome = match params["name"].as_str().unwrap_or("") {
            "look" => self.device().and_then(|d| d.screen()).map(|s| (describe_screen(&s), false)),
            "run" => self.run(args),
            "screenshot" => {
                return match self.device().and_then(|d| d.screenshot()) {
                    Ok(png) => {
                        json!({"content": [{"type": "image", "mimeType": "image/png", "data": base64(&png)}], "isError": false})
                    }
                    Err(e) => text_result(&format!("{e:#}"), true),
                }
            }
            other => Ok((format!("no tool named {other}"), true)),
        };
        match outcome {
            Ok((text, is_error)) => text_result(&text, is_error),
            Err(e) => text_result(&format!("{e:#}"), true),
        }
    }

    fn run(&mut self, args: &Value) -> Result<(String, bool)> {
        if let Some(app) = args["appId"].as_str() {
            self.app_id = Some(app.to_string());
        }
        let commands = args["commands"].as_str().unwrap_or("");
        let app = self.app_id.clone().unwrap_or_else(|| "unknown.app".into());
        let flow = match parse_flow(&format!("appId: {app}\n---\n{commands}\n"), None) {
            Ok(f) => f,
            Err(e) => return Ok((e.to_string(), true)),
        };
        let timeout = Duration::from_millis(args["timeoutMs"].as_u64().unwrap_or(10_000));
        let opts = RunOptions { lookup_timeout: timeout, optional_lookup_timeout: timeout, condition_timeout: timeout.min(Duration::from_secs(7)), ..Default::default() };
        let dev = self.device()?;
        let result = Runner::new(dev.as_mut(), opts).run_flow(&flow);
        let mut text = String::new();
        for s in &result.steps {
            let word = match s.status {
                StepStatus::Passed => "passed",
                StepStatus::Failed => "FAILED",
                StepStatus::Warned => "warned",
                StepStatus::Skipped => "skipped",
            };
            text.push_str(&format!("{:indent$}{word} {} ({} ms){}\n", "", s.name, s.duration.as_millis(), s.error.as_ref().map(|e| format!(": {e}")).unwrap_or_default(), indent = 2 * s.depth.saturating_sub(1)));
        }
        let failed = result.error.is_some();
        if let Some(e) = &result.error {
            text.push_str(&format!("error: {e}\n"));
        }
        text.push_str("\nScreen now:\n");
        text.push_str(&describe_screen(&dev.screen()?));
        Ok((text, failed))
    }
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn tools() -> Value {
    json!([
        {
            "name": "look",
            "description": "List what is on the device's screen: one line per element with its text, id, position and whether it is clickable.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "run",
            "description": "Run Maestro flow commands (YAML list) on the device, e.g. \"- tapOn: Sign in\\n- inputText: hello\". Returns each step's result and the screen afterwards.",
            "inputSchema": {"type": "object", "properties": {
                "commands": {"type": "string", "description": "A YAML list of Maestro commands"},
                "appId": {"type": "string", "description": "The app for launchApp / stopApp; remembered for later calls"},
                "timeoutMs": {"type": "integer", "description": "How long a command waits for its element (default 10000)"}
            }, "required": ["commands"]}
        },
        {
            "name": "screenshot",
            "description": "A PNG screenshot of the device's screen.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

/// One line per element a selector could match that has a label or an id.
pub fn describe_screen(s: &ScreenTree) -> String {
    let mut out = String::new();
    for (i, e) in s.elements.iter().enumerate() {
        if !s.matchable(i) || (e.label().is_empty() && e.id.is_empty()) {
            continue;
        }
        let b = e.bounds;
        let mut line = String::new();
        if !e.label().is_empty() {
            line.push_str(&format!("\"{}\"", e.label()));
        }
        if !e.id.is_empty() {
            line.push_str(&format!(" #{}", e.id.rsplit('/').next().unwrap_or(&e.id)));
        }
        line.push_str(&format!(" [{},{} {}x{}]", b.left, b.top, b.width(), b.height()));
        if e.clickable() {
            line.push_str(" clickable");
        }
        out.push_str(line.trim_start());
        out.push('\n');
    }
    out
}

/// Standard, padded base64 (RFC 4648), for the screenshot an MCP image carries.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}
