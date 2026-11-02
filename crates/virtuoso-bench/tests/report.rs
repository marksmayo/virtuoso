//! The HTML benchmark report.

use virtuoso_bench::features::FeatureMatrix;
use virtuoso_bench::metrics::{RunRecord, StaticInfo, DEVICE_SCENARIOS};
use virtuoso_bench::orchestrate::Results;
use virtuoso_bench::report::render;
use std::collections::BTreeMap;

fn results() -> Results {
    let tools = vec!["virtuoso".to_string(), "maestro-runner".to_string(), "speedrun".to_string()];
    let mut runs = Vec::new();
    for (k, t) in tools.iter().enumerate() {
        for s in DEVICE_SCENARIOS.iter().chain(&["trivial", "invalid", "version"]) {
            for i in 0..3 {
                runs.push(RunRecord {
                    tool: t.clone(),
                    scenario: s.to_string(),
                    iteration: i,
                    wall_ms: 1000.0 * (k + 1) as f64 + i as f64,
                    peak_rss_kb: Some(10_000.0 * (k + 1) as f64),
                    cpu_ms: Some(100.0),
                    passed: !(t == "speedrun" && *s == "scroll"),
                    agent_rss_kb: Some(50_000.0),
                    error: (t == "speedrun" && *s == "scroll").then(|| "Element <b> not found".to_string()),
                    ..Default::default()
                });
            }
        }
    }
    let mut statics = BTreeMap::new();
    for t in &tools {
        statics.insert(t.clone(), StaticInfo { install_bytes: Some(4e6), version: format!("{t} 1.0") });
    }
    let mut meta = BTreeMap::new();
    meta.insert("device".to_string(), "emulator-5554 (Android 16, API 36)".to_string());
    Results { meta, tools, statics, runs }
}

fn features() -> FeatureMatrix {
    serde_json::from_str(r#"{"tools": ["virtuoso", "maestro-runner", "speedrun"], "categories": [
        {"name": "Platforms", "features": [
            {"key": "ios", "label": "iOS simulator", "support": {"virtuoso": "no", "maestro-runner": "yes", "speedrun": "yes"}, "notes": {"virtuoso": "Android only"}}
        ]}]}"#)
    .unwrap()
}

#[test]
fn the_page_is_self_contained_html() {
    let h = render(&results(), &features());
    assert!(h.starts_with("<!doctype html>"));
    assert!(h.contains("<title>"));
    assert!(!h.contains("<script src"), "no external scripts");
    assert!(h.contains("prefers-color-scheme: dark"), "dark mode");
}

#[test]
fn the_overall_medal_table_ranks_the_tool_with_most_golds_first() {
    let h = render(&results(), &features());
    let table = &h[h.find("id=\"overall\"").expect("overall table")..];
    let s = table.find(">virtuoso<").unwrap();
    let m = table.find(">maestro-runner<").unwrap();
    assert!(s < m, "virtuoso is fastest in the sample, so it leads");
}

#[test]
fn every_metric_and_feature_category_is_shown() {
    let h = render(&results(), &features());
    for label in ["App launch", "Navigation", "Whole suite", "Peak host memory", "Runs passed", "Install size", "Platforms", "iOS simulator"] {
        assert!(h.contains(label), "missing {label}");
    }
}

#[test]
fn feature_notes_and_run_errors_are_escaped() {
    let h = render(&results(), &features());
    assert!(h.contains("Android only"));
    assert!(h.contains("Element &lt;b&gt; not found"));
    assert!(!h.contains("Element <b> not found"));
}

#[test]
fn the_execution_chart_draws_a_bar_per_tool_and_scenario() {
    let h = render(&results(), &features());
    let svg = &h[h.find("<svg").expect("chart")..h.find("</svg>").unwrap()];
    let bars = svg.matches("class=\"bar").count();
    // speedrun has no scroll value (it failed), so one bar fewer.
    assert_eq!(bars, 3 * DEVICE_SCENARIOS.len() - 1);
}

#[test]
fn native_windows_runs_get_their_own_section_and_no_medals() {
    let mut r = results();
    for s in DEVICE_SCENARIOS {
        for i in 0..3 {
            r.runs.push(RunRecord {
                tool: "virtuoso-windows".into(),
                scenario: s.to_string(),
                iteration: i,
                wall_ms: 500.0,
                peak_rss_kb: Some(9000.0),
                cpu_ms: Some(50.0),
                passed: true,
                agent_rss_kb: None,
                error: None,
                ..Default::default()
            });
        }
    }
    let h = render(&r, &features());
    let section = &h[h.find("id=\"native\"").expect("native section")..];
    assert!(section.contains("virtuoso-windows") && section.contains("maestro-runner"));
    let overall = &h[h.find("id=\"overall\"").unwrap()..h.find("id=\"native\"").unwrap()];
    assert!(!overall[..overall.find("</table>").unwrap()].contains("virtuoso-windows"), "not in the medal table");
}

#[test]
fn the_publishable_fragment_has_no_document_wrapper_but_keeps_title_and_style() {
    let f = virtuoso_bench::report::render_fragment(&results(), &features());
    assert!(f.starts_with("<title>"), "{}", &f[..40]);
    assert!(!f.contains("<html") && !f.contains("<body") && !f.contains("<!doctype"));
    assert!(f.contains("claude:viewport-fit-sides"));
    assert!(f.contains("max(16px, env(safe-area-inset-left, 0px))"));
}

#[test]
fn a_tool_that_accepts_an_invalid_flow_is_called_out() {
    let mut r = results();
    for run in r.runs.iter_mut().filter(|x| x.tool == "speedrun" && x.scenario == "invalid") {
        run.passed = false;
    }
    let h = render(&r, &features());
    assert!(h.contains("speedrun exited 0 on a flow with an unknown command"), "callout missing");
}

#[test]
fn a_status_note_in_the_results_is_shown_under_the_title() {
    let mut r = results();
    r.meta.insert("Status".into(), "Partial: 3 of 8 scenarios".into());
    let h = render(&r, &features());
    let lead = h.find("class=\"lead\"").unwrap();
    let status = h.find("Partial: 3 of 8 scenarios").expect("status shown");
    assert!(status < h.find("Overall medal table").unwrap() && status > lead.saturating_sub(500));
}

#[test]
fn the_method_explains_the_competition_is_scored_on_its_best_runs() {
    let h = render(&results(), &features());
    assert!(h.contains("Best of every run for the competition"), "scoring rule missing from the method");
}

#[test]
fn every_feature_is_a_medal_event_in_the_features_and_overall_tables() {
    let h = render(&results(), &features());
    let features_table = &h[h.find("id=\"features\"").expect("features table")..];
    let features_table = &features_table[..features_table.find("</table>").unwrap()];
    let row = |tool: &str| {
        let at = features_table.find(&format!("<span>{tool}</span>")).unwrap();
        features_table[at..].split("</tr>").next().unwrap().to_string()
    };
    assert!(row("maestro-runner").contains(r#"<td class="num">1</td>"#), "iOS is a gold for maestro-runner");
    assert!(row("virtuoso").contains(r#"<td class="num">0</td><td class="num">0</td><td class="num">0</td><td class="num">0</td>"#), "virtuoso has no iOS: no medal");
    assert!(h.contains("and 1 features, each a medal event"), "the overall note counts feature events");
    let ios = &h[h.find("<td>iOS simulator</td>").unwrap()..];
    assert!(ios[..ios.find("</tr>").unwrap()].contains("🥇"), "the feature row shows its medals");
}

#[test]
fn a_feature_every_tool_has_is_listed_but_wins_no_medal() {
    let f: FeatureMatrix = serde_json::from_str(r#"{"tools": ["virtuoso", "maestro-runner", "speedrun"], "categories": [
        {"name": "Commands", "features": [
            {"key": "tapOn", "label": "tapOn", "support": {"virtuoso": "yes", "maestro-runner": "yes", "speedrun": "yes"}},
            {"key": "travel", "label": "travel", "support": {"virtuoso": "yes", "maestro-runner": "yes", "speedrun": "no"}}
        ]}]}"#)
    .unwrap();
    let h = render(&results(), &f);
    assert!(h.contains("and 1 features, each a medal event"), "only the feature with a gap counts");
    let row = &h[h.find("<td>tapOn<").expect("shared feature still listed")..];
    let row = &row[..row.find("</tr>").unwrap()];
    assert!(!row.contains("🥇"), "no medal for a feature everyone has");
    assert!(row.contains("every tool"), "marked as shared");
}
