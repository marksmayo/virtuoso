//! An interactive session: Maestro commands run one at a time on a connected device, and the
//! ones that passed are kept, to be saved as a flow (`virtuoso session ...`, `virtuoso do`).

use crate::device::Device;
use crate::flow::parse_flow;
use crate::runner::{FlowStatus, RunOptions, Runner};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub command: String,
    pub passed: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

pub struct Session {
    app_id: String,
    opts: RunOptions,
    log: Vec<String>,
}

impl Session {
    pub fn new(app_id: &str, opts: RunOptions) -> Session {
        Session { app_id: app_id.to_string(), opts, log: vec![] }
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Runs `commands` (each as written in a flow, without the `- `) in order, until one fails.
    /// Those that pass are kept.
    pub fn run(&mut self, dev: &mut dyn Device, commands: &[String]) -> Vec<CommandResult> {
        let mut out = Vec::new();
        for c in commands {
            let start = std::time::Instant::now();
            let result = match parse_flow(&format!("appId: {}\n---\n{}", self.app_id, list_item(c)), None) {
                Err(e) => Err(e.msg),
                Ok(flow) => {
                    let r = Runner::new(dev, self.opts.clone()).run_flow(&flow);
                    if r.status == FlowStatus::Passed {
                        Ok(())
                    } else {
                        Err(r.error.unwrap_or_else(|| "failed".into()))
                    }
                }
            };
            let passed = result.is_ok();
            out.push(CommandResult { command: c.clone(), passed, error: result.err(), duration_ms: start.elapsed().as_millis() as u64 });
            if !passed {
                break;
            }
            self.log.push(c.clone());
        }
        out
    }

    /// The commands kept so far.
    pub fn log(&self) -> &[String] {
        &self.log
    }

    pub fn clear(&mut self) {
        self.log.clear();
    }

    /// The kept commands as a flow file.
    pub fn flow_yaml(&self) -> String {
        format!("appId: {}\n---\n{}", self.app_id, self.log.iter().map(|c| list_item(c)).collect::<String>())
    }
}

/// A command as a flow list item: `- ` before its first line, its other lines indented to match.
fn list_item(command: &str) -> String {
    let mut lines = command.trim_end().lines();
    let mut s = format!("- {}\n", lines.next().unwrap_or(""));
    for l in lines {
        s.push_str(&format!("  {l}\n"));
    }
    s
}
