//! Suite reports: JUnit (Maestro's shape), JSON and HTML.

use virtuoso::report::{allure, html, json, junit, FlowReport, SuiteReport};
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
