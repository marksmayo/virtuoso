//! Suite reports: JUnit (Maestro's shape), JSON and HTML.

use virtuoso::report::{allure, escape, html, json, junit, FlowReport, SuiteReport};
use virtuoso::runner::{FlowStatus, StepResult, StepStatus};
use std::collections::BTreeMap;
use std::time::Duration;

fn suite() -> SuiteReport {
    let step = |name: &str, status, error: Option<&str>| StepResult {
        name: name.into(),
        depth: 1,
        status,
        duration: Duration::from_millis(120),
        error: error.map(String::from),
    };
    let mut props = BTreeMap::new();
    props.insert("junitId".to_string(), "T-7".to_string());
    SuiteReport {
        name: "Test Suite".into(),
        device: "emulator-5554".into(),
        flows: vec![
            FlowReport {
                name: "login".into(),
                file: "flows/login.yaml".into(),
                status: FlowStatus::Passed,
                duration: Duration::from_millis(2500),
                error: None,
                steps: vec![step("Tap on \"Sign in\"", StepStatus::Passed, None)],
                tags: vec!["smoke".into()],
                properties: props,
            },
            FlowReport {
                name: "checkout <cart>".into(),
                file: "flows/checkout.yaml".into(),
                status: FlowStatus::Failed,
                duration: Duration::from_millis(1250),
                error: Some("Element not found: \"Pay & go\"".into()),
                steps: vec![step("Tap on \"Pay & go\"", StepStatus::Failed, Some("Element not found: \"Pay & go\""))],
                tags: vec![],
                properties: BTreeMap::new(),
            },
        ],
    }
}

#[test]
fn junit_has_one_testcase_per_flow_and_escapes_text() {
    let x = junit(&suite());
    assert!(x.starts_with("<?xml"));
    assert!(x.contains(r#"<testsuite name="Test Suite" device="emulator-5554" tests="2" failures="1""#), "{x}");
    assert!(x.contains(r#"<testcase id="T-7" name="login" classname="login" time="2.500" status="SUCCESS">"#), "{x}");
    assert!(x.contains(r#"name="checkout &lt;cart&gt;""#), "{x}");
    assert!(x.contains(r#"<failure message="Element not found: &quot;Pay &amp; go&quot;""#), "{x}");
    assert!(x.contains(r#"<property name="tags" value="smoke"/>"#), "{x}");
}

#[test]
fn json_lists_flows_with_steps_and_totals() {
    let v: serde_json::Value = serde_json::from_str(&json(&suite())).unwrap();
    assert_eq!(v["passed"], 1);
    assert_eq!(v["failed"], 1);
    assert_eq!(v["flows"][1]["status"], "failed");
    assert_eq!(v["flows"][0]["steps"][0]["duration_ms"], 120);
    assert_eq!(v["flows"][1]["error"], "Element not found: \"Pay & go\"");
}

#[test]
fn html_is_a_self_contained_page_with_every_flow() {
    let h = html(&suite());
    assert!(h.starts_with("<!doctype html>"));
    assert!(h.contains("checkout &lt;cart&gt;"));
    assert!(h.contains("1 passed") && h.contains("1 failed"), "summary");
    assert!(!h.contains("<script src"), "no external scripts");
}

#[test]
fn allure_writes_one_result_file_per_flow_with_steps_and_labels() {
    let files = allure(&suite(), 1_700_000_000_000);
    assert_eq!(files.len(), 2);
    let (name, text) = &files[0];
    assert!(name.ends_with("-result.json"), "{name}");
    assert_ne!(files[0].0, files[1].0, "each result has its own uuid");
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(v["name"], "login");
    assert_eq!(v["status"], "passed");
    assert_eq!(v["stage"], "finished");
    assert_eq!(v["start"], 1_700_000_000_000u64);
    assert_eq!(v["stop"], 1_700_000_002_500u64);
    let labels = v["labels"].as_array().unwrap();
    assert!(labels.iter().any(|l| l["name"] == "suite" && l["value"] == "Test Suite"));
    assert!(labels.iter().any(|l| l["name"] == "tag" && l["value"] == "smoke"));
    assert_eq!(v["steps"][0]["name"], "Tap on \"Sign in\"");
    let failed: serde_json::Value = serde_json::from_str(&files[1].1).unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["start"], 1_700_000_002_500u64, "flows run one after another");
    assert_eq!(failed["statusDetails"]["message"], "Element not found: \"Pay & go\"");
    assert_eq!(failed["steps"][0]["status"], "failed");
}

#[test]
fn json_and_allure_bytes_stay_the_same() {
    // Keys in sorted order, `statusDetails: {}` without an error: what tools have read so far.
    assert_eq!(
        json(&suite()),
        r#"{"device":"emulator-5554","duration_ms":3750,"failed":1,"flows":[{"duration_ms":2500,"error":null,"file":"flows/login.yaml","name":"login","status":"passed","steps":[{"depth":1,"duration_ms":120,"error":null,"name":"Tap on \"Sign in\"","status":"passed"}],"tags":["smoke"]},{"duration_ms":1250,"error":"Element not found: \"Pay & go\"","file":"flows/checkout.yaml","name":"checkout <cart>","status":"failed","steps":[{"depth":1,"duration_ms":120,"error":"Element not found: \"Pay & go\"","name":"Tap on \"Pay & go\"","status":"failed"}],"tags":[]}],"passed":1,"suite":"Test Suite"}"#
    );
    let files = allure(&suite(), 1_700_000_000_000);
    assert_eq!(files[0].0, "2921e847-941d-e190-3b40-44b6df5054d8-result.json");
    assert_eq!(
        files[0].1,
        r#"{"fullName":"flows/login.yaml","historyId":"1a629402-2949-8f3f-b317-5227b2346b27","labels":[{"name":"suite","value":"Test Suite"},{"name":"host","value":"emulator-5554"},{"name":"framework","value":"virtuoso"},{"name":"tag","value":"smoke"}],"name":"login","stage":"finished","start":1700000000000,"status":"passed","statusDetails":{},"steps":[{"name":"Tap on \"Sign in\"","stage":"finished","start":1700000000000,"status":"passed","statusDetails":{},"stop":1700000000120}],"stop":1700000002500,"uuid":"2921e847-941d-e190-3b40-44b6df5054d8"}"#
    );
    assert_eq!(files[1].0, "bb087f31-1c66-69e3-0001-0541d6dc25c6-result.json");
    assert_eq!(
        files[1].1,
        r#"{"fullName":"flows/checkout.yaml","historyId":"0a7dd8f9-6f98-a370-090d-c4b960d3579f","labels":[{"name":"suite","value":"Test Suite"},{"name":"host","value":"emulator-5554"},{"name":"framework","value":"virtuoso"}],"name":"checkout <cart>","stage":"finished","start":1700000002500,"status":"failed","statusDetails":{"message":"Element not found: \"Pay & go\""},"steps":[{"name":"Tap on \"Pay & go\"","stage":"finished","start":1700000002500,"status":"failed","statusDetails":{"message":"Element not found: \"Pay & go\""},"stop":1700000002620}],"stop":1700000003750,"uuid":"bb087f31-1c66-69e3-0001-0541d6dc25c6"}"#
    );
}

#[test]
fn junit_and_html_bytes_stay_the_same() {
    // Written into the file as they are made now, not built whole first: the same bytes.
    assert_eq!(junit(&suite()), r##"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="Test Suite" device="emulator-5554" tests="2" failures="1" time="3.750">
    <testcase id="T-7" name="login" classname="login" time="2.500" status="SUCCESS">
      <properties>
        <property name="tags" value="smoke"/>
      </properties>
    </testcase>
    <testcase id="checkout &lt;cart&gt;" name="checkout &lt;cart&gt;" classname="checkout &lt;cart&gt;" time="1.250" status="ERROR">
      <failure message="Element not found: &quot;Pay &amp; go&quot;">Element not found: &quot;Pay &amp; go&quot;</failure>
    </testcase>
  </testsuite>
</testsuites>
"##);
    assert_eq!(html(&suite()), r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Test Suite</title>
<style>:root{--bg:#fff;--fg:#1b1f24;--muted:#59636e;--line:#d1d9e0;--ok:#1a7f37;--bad:#cf222e;--warn:#9a6700;--card:#f6f8fa}@media (prefers-color-scheme:dark){:root{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--line:#3d444d;--ok:#3fb950;--bad:#f85149;--warn:#d29922;--card:#151b23}}body{background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif;margin:0 auto;max-width:960px;padding:24px 16px}h1{font-size:22px;margin:0 0 4px}.muted{color:var(--muted)}details{border:1px solid var(--line);border-radius:8px;margin:10px 0;background:var(--card)}summary{cursor:pointer;padding:10px 14px;display:flex;gap:10px;align-items:baseline}.passed{color:var(--ok)}.failed{color:var(--bad)}.warned{color:var(--warn)}.skipped{color:var(--muted)}table{width:100%;border-collapse:collapse;font-size:14px}td{padding:4px 14px;border-top:1px solid var(--line);vertical-align:top}td.t{text-align:right;white-space:nowrap;font-variant-numeric:tabular-nums}.err{color:var(--bad);white-space:pre-wrap}</style></head><body>
<h1>Test Suite</h1>
<p class="muted">emulator-5554 · <span class="passed">1 passed</span> · <span class="failed">1 failed</span> · 3.8 s</p>
<details><summary><b class="passed">✓</b><b>login</b><span class="muted">flows/login.yaml · 2.5 s</span></summary><table><tr><td style="padding-left:14px"><span class="passed">passed</span> Tap on &quot;Sign in&quot;</td><td class="t">120 ms</td></tr></table></details>
<details open><summary><b class="failed">✗</b><b>checkout &lt;cart&gt;</b><span class="muted">flows/checkout.yaml · 1.2 s</span></summary><table><tr><td style="padding-left:14px"><span class="failed">failed</span> Tap on &quot;Pay &amp; go&quot;<div class="err">Element not found: &quot;Pay &amp; go&quot;</div></td><td class="t">120 ms</td></tr></table></details>
</body></html>
"##);
}

#[test]
fn escaping_drops_control_characters_but_newline_and_tab() {
    assert_eq!(escape("plain text"), "plain text");
    assert_eq!(escape("a'b\u{1}c\td\ne<f>&\"g\u{1f}"), "a&#39;bc\td\ne&lt;f&gt;&amp;&quot;g");
    assert_eq!(escape("café ✓ <"), "café ✓ &lt;");
}
