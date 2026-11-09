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
fn the_overall_medal_table_links_each_tool_to_its_repository() {
    let h = render(&results(), &features());
    let table = &h[h.find("id=\"overall\"").expect("overall table")..];
    let table = &table[..table.find("</table>").unwrap()];
    for (tool, repo) in [
        ("virtuoso", "https://github.com/marksmayo/virtuoso"),
        ("maestro-runner", "https://github.com/devicelab-dev/maestro-runner"),
        ("speedrun", "https://github.com/hubastard/speedrun"),
    ] {
        assert!(table.contains(&format!(r#"<a href="{repo}">{tool}</a>"#)), "{tool} links to {repo}");
    }
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
        let at = features_table.find(&format!(">{tool}</a></span>")).unwrap();
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

#[test]
fn the_readme_summary_has_the_medal_tables_and_each_scenarios_times() {
    let md = virtuoso_bench::report::readme_summary(&results(), &features());
    assert!(md.contains("| Tool | 🥇 | 🥈 | 🥉 | Total |"), "{md}");
    let overall = md.find("| [virtuoso](").expect("virtuoso row");
    assert!(overall < md.find("| [maestro-runner](").unwrap(), "most golds first");
    assert!(md.contains("App launch"));
    // The fastest time in a row is bold; speedrun failed every scroll run, so it has no value.
    assert!(md.contains("**1.00 s**"), "{md}");
    let scroll = md.lines().find(|l| l.starts_with("| Scroll")).expect("scroll row");
    assert!(scroll.ends_with("| — |"), "{scroll}");
}

#[test]
fn the_readme_medal_tables_link_each_tool_to_its_repository() {
    let md = virtuoso_bench::report::readme_summary(&results(), &features());
    let overall = &md[md.find("**Overall**").unwrap()..md.find("**Performance**").unwrap()];
    for (tool, repo) in [
        ("virtuoso", "https://github.com/marksmayo/virtuoso"),
        ("maestro-runner", "https://github.com/devicelab-dev/maestro-runner"),
        ("speedrun", "https://github.com/hubastard/speedrun"),
    ] {
        assert!(overall.contains(&format!("| [{tool}]({repo}) |")), "{tool} links to {repo}: {overall}");
    }
}

#[test]
fn the_readme_section_between_the_markers_is_replaced_and_the_rest_kept() {
    use virtuoso_bench::report::update_readme;
    let readme = "# x\n\n## Benchmark\n\nintro\n\n<!-- benchmark:start -->\nold\n<!-- benchmark:end -->\n\n## License\n";
    let new = update_readme(readme, "new table\n");
    assert_eq!(new, "# x\n\n## Benchmark\n\nintro\n\n<!-- benchmark:start -->\nnew table\n<!-- benchmark:end -->\n\n## License\n");
    // Without markers the section goes in before the license.
    let bare = update_readme("# x\n\n## License\n", "t\n");
    assert_eq!(bare, "# x\n\n<!-- benchmark:start -->\nt\n<!-- benchmark:end -->\n\n## License\n");
    assert_eq!(update_readme("# x\n", "t\n"), "# x\n\n<!-- benchmark:start -->\nt\n<!-- benchmark:end -->\n");
}

/// The sample with Maestro and Appium added, and two bench app flows that Appium cannot run.
fn five_tools() -> Results {
    let mut r = results();
    for t in ["maestro", "appium"] {
        r.tools.push(t.into());
        r.statics.insert(t.into(), StaticInfo { install_bytes: Some(3e8), version: format!("{t} 2.0") });
        for s in DEVICE_SCENARIOS {
            r.runs.push(RunRecord { tool: t.into(), scenario: s.to_string(), wall_ms: 9000.0, passed: true, ..Default::default() });
        }
    }
    for t in ["virtuoso", "maestro-runner", "speedrun", "maestro"] {
        for f in ["chat", "cart"] {
            r.runs.push(RunRecord { tool: t.into(), scenario: format!("app/{f}"), wall_ms: 20_000.0, passed: true, ..Default::default() });
        }
    }
    r
}

#[test]
fn the_bench_app_flows_get_their_own_section_with_a_row_per_flow() {
    let h = render(&five_tools(), &features());
    let section = &h[h.find("id=\"bench-app\"").expect("bench app section")..];
    let section = &section[..section.find("<h2").unwrap_or(section.len())];
    for label in ["chat", "cart", "Whole app suite", "App flows passed", "hubastard/speedrun"] {
        assert!(section.contains(label), "missing {label}");
    }
    assert!(section.contains("<svg"), "a chart of the flows");
}

#[test]
fn tester_armys_mobile_benchmark_gets_a_section_of_its_own() {
    let mut r = five_tools();
    for t in ["virtuoso", "maestro"] {
        r.runs.push(RunRecord { tool: t.into(), scenario: "tam/login_form".into(), wall_ms: 5_000.0, passed: true, ..Default::default() });
    }
    let h = render(&r, &features());
    let section = &h[h.find("id=\"tam-app\"").expect("mobile benchmark section")..];
    let section = &section[..section.find("<h3").unwrap_or(section.len())];
    for label in ["login_form", "Whole app suite", "tester-army/e2e"] {
        assert!(section.contains(label), "missing {label}");
    }
    assert!(!section.contains(">chat<"), "the Speedrun app's flows stay in their section");
}

#[test]
fn each_app_suites_section_explains_its_failures_and_workarounds() {
    let mut r = five_tools();
    r.runs.push(RunRecord { tool: "maestro".into(), scenario: "tam/login_form".into(), wall_ms: 5_000.0, passed: true, ..Default::default() });
    let h = render(&r, &features());
    let section = |id: &str| {
        let s = &h[h.find(&format!("id=\"{id}\"")).expect(id)..];
        s[..s[4..].find("<h3").map_or(s.len(), |i| i + 4)].to_string()
    };
    let (app, tam) = (section("bench-app"), section("tam-app"));
    assert!(app.contains("Notes on these runs") && tam.contains("Notes on these runs"));
    assert!(app.contains("Backspace"), "the Inputs limit of e2e and Argent");
    assert!(tam.contains("20 swipes"), "maestro-runner's scroll limit");
    assert!(!app.contains("20 swipes"), "each suite keeps its own notes");
}

#[test]
fn every_tool_has_its_own_series_colour_and_the_lead_names_them_all() {
    let h = render(&five_tools(), &features());
    for slot in 1..=5 {
        assert!(h.contains(&format!(".sw{slot} {{")) && h.contains(&format!(".bar{slot} {{")), "no colour for slot {slot}");
    }
    let lead = &h[h.find("class=\"lead\"").unwrap()..];
    let lead = &lead[..lead.find("</p>").unwrap()];
    for t in ["virtuoso", "maestro-runner", "speedrun", "maestro", "appium"] {
        assert!(lead.contains(t), "lead leaves out {t}: {lead}");
    }
}

#[test]
fn a_tool_outside_the_feature_matrix_is_said_to_take_no_part_in_it() {
    let h = render(&five_tools(), &features());
    let section = &h[h.find("<h2>Features</h2>").unwrap()..];
    assert!(section.contains("appium") && section.contains("not in the feature matrix"), "{}", &section[..400]);
}

#[test]
fn the_readme_from_several_fields_has_overall_medals_and_a_section_with_a_chart_per_field() {
    use virtuoso_bench::fields::Field;
    use virtuoso_bench::report::readme_fields_summary;
    let fields = vec![
        Field { name: "Android (Linux runner)".into(), results: results() },
        Field { name: "iOS (macOS runner)".into(), results: results() },
    ];
    let md = readme_fields_summary(&fields, &features(), "docs/bench");
    assert!(md.contains("**Overall**"), "{md}");
    for f in ["Android (Linux runner)", "iOS (macOS runner)"] {
        assert!(md.contains(&format!("### {f}")), "a section for {f}");
    }
    assert!(md.contains("![Android (Linux runner)](docs/bench/android-linux-runner.svg)"), "{md}");
    assert!(md.contains("![iOS (macOS runner)](docs/bench/ios-macos-runner.svg)"), "{md}");
    // Each field's performance events count separately, the features once.
    let perf_per_field = virtuoso_bench::metrics::compute(&results().runs, &results().statics, &results().tools).len();
    assert!(md.contains(&format!("Each of the {} performance events", perf_per_field * 2)), "{md}");
}

#[test]
fn a_run_of_virtuoso_windows_alone_still_reports() {
    // A run of only the supplement lists no competing tools: no medals, so no leader to name.
    let mut r = results();
    r.tools = vec![];
    for run in &mut r.runs {
        run.tool = "virtuoso-windows".to_string();
    }
    let h = render(&r, &features());
    assert!(h.contains("id=\"native\""), "the native section still shows its runs");
}
