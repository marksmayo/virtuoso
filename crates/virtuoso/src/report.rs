//! Suite reports: JUnit XML in Maestro's shape (CI systems read it), JSON for tools, Allure
//! results, and a self-contained HTML page for people.

use crate::runner::{FlowStatus, StepResult, StepStatus};
use serde::Serialize;
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

/// `s` with XML/HTML's special characters escaped and control characters (but newline and
/// tab) dropped, written straight into the output: plain runs go out as they are.
pub struct Esc<'a>(pub &'a str);

impl std::fmt::Display for Esc<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut plain = 0;
        for (i, c) in self.0.char_indices() {
            let with = match c {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&#39;",
                c if (c as u32) < 0x20 && c != '\n' && c != '\t' => "",
                _ => continue,
            };
            f.write_str(&self.0[plain..i])?;
            f.write_str(with)?;
            plain = i + 1;
        }
        f.write_str(&self.0[plain..])
    }
}

pub fn escape(s: &str) -> String {
    Esc(s).to_string()
}

pub fn junit(s: &SuiteReport) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
    let _ = writeln!(
        x,
        r#"  <testsuite name="{}" device="{}" tests="{}" failures="{}" time="{:.3}">"#,
        Esc(&s.name),
        Esc(&s.device),
        s.flows.len(),
        s.failed(),
        s.duration().as_secs_f64()
    );
    for f in &s.flows {
        let id = f.properties.get("junitId").unwrap_or(&f.name);
        let class = f.properties.get("junitClassname").unwrap_or(&f.name);
        let status = if f.status == FlowStatus::Passed { "SUCCESS" } else { "ERROR" };
        let _ = writeln!(x, r#"    <testcase id="{}" name="{}" classname="{}" time="{:.3}" status="{status}">"#, Esc(id), Esc(&f.name), Esc(class), f.duration.as_secs_f64());
        let tags = (!f.tags.is_empty()).then(|| f.tags.join(", "));
        let mut props = f.properties.iter().filter(|(k, _)| *k != "junitId" && *k != "junitClassname").map(|(k, v)| (k.as_str(), v.as_str())).chain(tags.as_deref().map(|t| ("tags", t))).peekable();
        if props.peek().is_some() {
            x.push_str("      <properties>\n");
            for (k, v) in props {
                let _ = writeln!(x, r#"        <property name="{}" value="{}"/>"#, Esc(k), Esc(v));
            }
            x.push_str("      </properties>\n");
        }
        if let Some(e) = &f.error {
            let _ = writeln!(x, r#"      <failure message="{}">{}</failure>"#, Esc(e), Esc(e));
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

// The JSON and Allure files are written from these, which borrow from the report: no
// serde_json::Value tree built and walked again. Fields are in sorted order, the order the
// Value maps printed them in, so the files stay byte for byte the same.

#[derive(Serialize)]
struct Details<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

#[derive(Serialize)]
struct Label<'a> {
    name: &'static str,
    value: &'a str,
}

#[derive(Serialize)]
struct AllureStep<'a> {
    name: &'a str,
    stage: &'static str,
    start: u64,
    status: &'static str,
    #[serde(rename = "statusDetails")]
    status_details: Details<'a>,
    stop: u64,
}

#[derive(Serialize)]
struct AllureResult<'a> {
    #[serde(rename = "fullName")]
    full_name: &'a str,
    #[serde(rename = "historyId")]
    history_id: String,
    labels: Vec<Label<'a>>,
    name: &'a str,
    stage: &'static str,
    start: u64,
    status: &'static str,
    #[serde(rename = "statusDetails")]
    status_details: Details<'a>,
    steps: Vec<AllureStep<'a>>,
    stop: u64,
    uuid: &'a str,
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
            let steps = f
                .steps
                .iter()
                .map(|st| {
                    let begin = step_at;
                    step_at += st.duration.as_millis() as u64;
                    AllureStep { name: &st.name, stage: "finished", start: begin, status: allure_status(st.status), status_details: Details { message: st.error.as_deref() }, stop: step_at }
                })
                .collect();
            let mut labels = vec![Label { name: "suite", value: &s.name }, Label { name: "host", value: &s.device }, Label { name: "framework", value: "virtuoso" }];
            labels.extend(f.tags.iter().map(|t| Label { name: "tag", value: t }));
            let uuid = uuid_from(&format!("{started_ms}/{n}/{}", f.file));
            let result = AllureResult {
                full_name: &f.file,
                history_id: uuid_from(&f.file),
                labels,
                name: &f.name,
                stage: "finished",
                start,
                status: if f.status == FlowStatus::Passed { "passed" } else { "failed" },
                status_details: Details { message: f.error.as_deref() },
                steps,
                stop,
                uuid: &uuid,
            };
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

#[derive(Serialize)]
struct JsonStep<'a> {
    depth: usize,
    duration_ms: u64,
    error: &'a Option<String>,
    name: &'a str,
    status: &'static str,
}

#[derive(Serialize)]
struct JsonFlow<'a> {
    duration_ms: u64,
    error: &'a Option<String>,
    file: &'a str,
    name: &'a str,
    status: &'static str,
    steps: Vec<JsonStep<'a>>,
    tags: &'a [String],
}

#[derive(Serialize)]
struct JsonSuite<'a> {
    device: &'a str,
    duration_ms: u64,
    failed: usize,
    flows: Vec<JsonFlow<'a>>,
    passed: usize,
    suite: &'a str,
}

pub fn json(s: &SuiteReport) -> String {
    let flows = s
        .flows
        .iter()
        .map(|f| JsonFlow {
            duration_ms: f.duration.as_millis() as u64,
            error: &f.error,
            file: &f.file,
            name: &f.name,
            status: if f.status == FlowStatus::Passed { "passed" } else { "failed" },
            steps: f.steps.iter().map(|st| JsonStep { depth: st.depth, duration_ms: st.duration.as_millis() as u64, error: &st.error, name: &st.name, status: status_name(st.status) }).collect(),
            tags: &f.tags,
        })
        .collect();
    serde_json::to_string(&JsonSuite { device: &s.device, duration_ms: s.duration().as_millis() as u64, failed: s.failed(), flows, passed: s.passed(), suite: &s.name }).unwrap()
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
        name = Esc(&s.name),
        device = Esc(&s.device),
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
            name = Esc(&f.name),
            file = Esc(&f.file),
            secs = f.duration.as_secs_f64()
        );
        for st in &f.steps {
            let _ = write!(h, r#"<tr><td style="padding-left:{pad}px"><span class="{c}">{c}</span> {name}"#, pad = 14 + 18 * st.depth.saturating_sub(1), c = status_name(st.status), name = Esc(&st.name));
            if let Some(e) = &st.error {
                let _ = write!(h, r#"<div class="err">{}</div>"#, Esc(e));
            }
            let _ = write!(h, r#"</td><td class="t">{} ms</td></tr>"#, st.duration.as_millis());
        }
        h.push_str("</table></details>\n");
    }
    h.push_str("</body></html>\n");
    h
}
