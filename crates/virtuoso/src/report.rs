//! Suite reports: JUnit XML in Maestro's shape (CI systems read it), JSON for tools, Allure
//! results, and a self-contained HTML page for people.

use crate::runner::{FlowStatus, StepResult, StepStatus};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FlowReport {
    pub name: String,
    pub file: String,
    pub status: FlowStatus,
    pub duration: Duration,
    pub error: Option<String>,
    pub steps: Vec<StepResult>,
    pub tags: Vec<String>,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SuiteReport {
    pub name: String,
    pub device: String,
    pub flows: Vec<FlowReport>,
}

impl SuiteReport {
    pub fn passed(&self) -> usize {
        self.flows.iter().filter(|f| f.status == FlowStatus::Passed).count()
    }
    pub fn failed(&self) -> usize {
        self.flows.len() - self.passed()
    }
    pub fn duration(&self) -> Duration {
        self.flows.iter().map(|f| f.duration).sum()
    }
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

pub fn junit(s: &SuiteReport) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
    let _ = writeln!(
        x,
        r#"  <testsuite name="{}" device="{}" tests="{}" failures="{}" time="{:.3}">"#,
        escape(&s.name),
        escape(&s.device),
        s.flows.len(),
        s.failed(),
        s.duration().as_secs_f64()
    );
    for f in &s.flows {
        let id = f.properties.get("junitId").cloned().unwrap_or_else(|| f.name.clone());
        let class = f.properties.get("junitClassname").cloned().unwrap_or_else(|| f.name.clone());
        let status = if f.status == FlowStatus::Passed { "SUCCESS" } else { "ERROR" };
        let _ = writeln!(
            x,
            r#"    <testcase id="{}" name="{}" classname="{}" time="{:.3}" status="{status}">"#,
            escape(&id),
            escape(&f.name),
            escape(&class),
            f.duration.as_secs_f64()
        );
        let props: Vec<(String, String)> = f
            .properties
            .iter()
            .filter(|(k, _)| *k != "junitId" && *k != "junitClassname")
            .map(|(k, v)| (k.clone(), v.clone()))
            .chain((!f.tags.is_empty()).then(|| ("tags".to_string(), f.tags.join(", "))))
            .collect();
        if !props.is_empty() {
            x.push_str("      <properties>\n");
            for (k, v) in props {
                let _ = writeln!(x, r#"        <property name="{}" value="{}"/>"#, escape(&k), escape(&v));
            }
            x.push_str("      </properties>\n");
        }
        if let Some(e) = &f.error {
            let _ = writeln!(x, r#"      <failure message="{}">{}</failure>"#, escape(e), escape(e));
        }
        x.push_str("    </testcase>\n");
    }
    x.push_str("  </testsuite>\n</testsuites>\n");
    x
}

fn status_name(s: StepStatus) -> &'static str {
    match s {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Warned => "warned",
        StepStatus::Skipped => "skipped",
    }
}

/// Allure results: one `<uuid>-result.json` per flow, as (file name, contents). Flows ran one
/// after another from `started_ms` (Unix milliseconds).
pub fn allure(s: &SuiteReport, started_ms: u64) -> Vec<(String, String)> {
    let mut at = started_ms;
    s.flows
        .iter()
        .enumerate()
        .map(|(n, f)| {
            let start = at;
            let stop = start + f.duration.as_millis() as u64;
            at = stop;
            let mut step_at = start;
            let steps: Vec<serde_json::Value> = f
                .steps
                .iter()
                .map(|st| {
                    let begin = step_at;
                    step_at += st.duration.as_millis() as u64;
                    serde_json::json!({
                        "name": st.name,
                        "status": allure_status(st.status),
                        "stage": "finished",
                        "start": begin,
                        "stop": step_at,
                        "statusDetails": st.error.as_ref().map(|e| serde_json::json!({ "message": e })).unwrap_or(serde_json::json!({})),
                    })
                })
                .collect();
            let mut labels = vec![
                serde_json::json!({ "name": "suite", "value": s.name }),
                serde_json::json!({ "name": "host", "value": s.device }),
                serde_json::json!({ "name": "framework", "value": "virtuoso" }),
            ];
            labels.extend(f.tags.iter().map(|t| serde_json::json!({ "name": "tag", "value": t })));
            let uuid = uuid_from(&format!("{started_ms}/{n}/{}", f.file));
            let result = serde_json::json!({
                "uuid": uuid,
                "historyId": uuid_from(&f.file),
                "name": f.name,
                "fullName": f.file,
                "status": if f.status == FlowStatus::Passed { "passed" } else { "failed" },
                "statusDetails": f.error.as_ref().map(|e| serde_json::json!({ "message": e })).unwrap_or(serde_json::json!({})),
                "stage": "finished",
                "start": start,
                "stop": stop,
                "labels": labels,
                "steps": steps,
            });
            (format!("{uuid}-result.json"), serde_json::to_string(&result).unwrap())
        })
        .collect()
}

fn allure_status(s: StepStatus) -> &'static str {
    match s {
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
        // An optional step that failed did not fail the flow.
        StepStatus::Passed | StepStatus::Warned => "passed",
    }
}

/// A stable UUID-shaped id for `key`.
fn uuid_from(key: &str) -> String {
    use std::hash::{Hash, Hasher};
    let half = |salt: u8| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (salt, key).hash(&mut h);
        h.finish()
    };
    let hex = format!("{:016x}{:016x}", half(1), half(2));
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

pub fn json(s: &SuiteReport) -> String {
    let flows: Vec<serde_json::Value> = s
        .flows
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name,
                "file": f.file,
                "status": if f.status == FlowStatus::Passed { "passed" } else { "failed" },
                "duration_ms": f.duration.as_millis() as u64,
                "error": f.error,
                "tags": f.tags,
                "steps": f.steps.iter().map(|st| serde_json::json!({
                    "name": st.name,
                    "depth": st.depth,
                    "status": status_name(st.status),
                    "duration_ms": st.duration.as_millis() as u64,
                    "error": st.error,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::to_string(&serde_json::json!({
        "suite": s.name,
        "device": s.device,
        "passed": s.passed(),
        "failed": s.failed(),
        "duration_ms": s.duration().as_millis() as u64,
        "flows": flows,
    }))
    .unwrap()
}

pub fn html(s: &SuiteReport) -> String {
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{name}</title>
<style>:root{{--bg:#fff;--fg:#1b1f24;--muted:#59636e;--line:#d1d9e0;--ok:#1a7f37;--bad:#cf222e;--warn:#9a6700;--card:#f6f8fa}}@media (prefers-color-scheme:dark){{:root{{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--line:#3d444d;--ok:#3fb950;--bad:#f85149;--warn:#d29922;--card:#151b23}}}}body{{background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif;margin:0 auto;max-width:960px;padding:24px 16px}}h1{{font-size:22px;margin:0 0 4px}}.muted{{color:var(--muted)}}details{{border:1px solid var(--line);border-radius:8px;margin:10px 0;background:var(--card)}}summary{{cursor:pointer;padding:10px 14px;display:flex;gap:10px;align-items:baseline}}.passed{{color:var(--ok)}}.failed{{color:var(--bad)}}.warned{{color:var(--warn)}}.skipped{{color:var(--muted)}}table{{width:100%;border-collapse:collapse;font-size:14px}}td{{padding:4px 14px;border-top:1px solid var(--line);vertical-align:top}}td.t{{text-align:right;white-space:nowrap;font-variant-numeric:tabular-nums}}.err{{color:var(--bad);white-space:pre-wrap}}</style></head><body>
<h1>{name}</h1>
<p class="muted">{device} · <span class="passed">{passed} passed</span> · <span class="failed">{failed} failed</span> · {secs:.1} s</p>
"#,
        name = escape(&s.name),
        device = escape(&s.device),
        passed = s.passed(),
        failed = s.failed(),
        secs = s.duration().as_secs_f64()
    );
    for f in &s.flows {
        let cls = if f.status == FlowStatus::Passed { "passed" } else { "failed" };
        let _ = write!(
            h,
            r#"<details{open}><summary><b class="{cls}">{mark}</b><b>{name}</b><span class="muted">{file} · {secs:.1} s</span></summary><table>"#,
            open = if cls == "failed" { " open" } else { "" },
            mark = if cls == "passed" { "✓" } else { "✗" },
            name = escape(&f.name),
            file = escape(&f.file),
            secs = f.duration.as_secs_f64()
        );
        for st in &f.steps {
            let _ = write!(
                h,
                r#"<tr><td style="padding-left:{pad}px"><span class="{c}">{c}</span> {name}{err}</td><td class="t">{ms} ms</td></tr>"#,
                pad = 14 + 18 * st.depth.saturating_sub(1),
                c = status_name(st.status),
                name = escape(&st.name),
                err = st.error.as_ref().map(|e| format!(r#"<div class="err">{}</div>"#, escape(e))).unwrap_or_default(),
                ms = st.duration.as_millis()
            );
        }
        h.push_str("</table></details>\n");
    }
    h.push_str("</body></html>\n");
    h
}
