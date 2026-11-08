//! One report from several fields (a platform on a host): each performance metric is a medal
//! event of its own in each field.

use std::collections::BTreeMap;
use virtuoso_bench::fields::{field_metrics, Field};
use virtuoso_bench::medals::{medal_table, Medal};
use virtuoso_bench::metrics::RunRecord;
use virtuoso_bench::orchestrate::Results;

fn results(times: &[(&str, f64)]) -> Results {
    let tools: Vec<String> = times.iter().map(|(t, _)| t.to_string()).collect();
    let runs = times
        .iter()
        .flat_map(|(t, s)| (0..3).map(move |i| RunRecord { tool: t.to_string(), scenario: "launch".into(), iteration: i, wall_ms: s * 1000.0, passed: true, ..Default::default() }))
        .collect();
    Results { meta: BTreeMap::new(), tools, statics: BTreeMap::new(), runs }
}

#[test]
fn each_field_scores_every_metric_as_an_event_of_its_own() {
    // virtuoso wins launch on Android, maestro on iOS: both show.
    let fields = vec![
        Field { name: "Android".into(), results: results(&[("virtuoso", 1.0), ("maestro", 5.0)]) },
        Field { name: "iOS".into(), results: results(&[("virtuoso", 6.0), ("maestro", 2.0)]) },
    ];
    let metrics = field_metrics(&fields);
    let launch: Vec<_> = metrics.iter().filter(|m| m.label.starts_with("App launch")).collect();
    assert_eq!(launch.len(), 2, "{:?}", metrics.iter().map(|m| &m.label).collect::<Vec<_>>());
    assert!(launch.iter().any(|m| m.label.contains("Android")) && launch.iter().any(|m| m.label.contains("iOS")));
    assert_ne!(launch[0].key, launch[1].key, "distinct events");
    let tools = vec!["virtuoso".to_string(), "maestro".to_string()];
    let gold = |name: &str| {
        let m: Vec<_> = launch.iter().filter(|m| m.label.contains(name)).map(|m| (*m).clone()).collect();
        medal_table(&m, &tools).into_iter().find(|r| r.gold == 1).map(|r| r.tool)
    };
    assert_eq!(gold("Android").as_deref(), Some("virtuoso"));
    assert_eq!(gold("iOS").as_deref(), Some("maestro"));
    let _ = Medal::Gold;
}

use virtuoso_bench::fields::relative_times;
use virtuoso_bench::medals::{Better, MetricResult};

fn timed(key: &str, values: &[(&str, Option<f64>)]) -> MetricResult {
    MetricResult { key: key.into(), label: key.into(), group: "Execution".into(), unit: "s".into(), better: Better::Lower, values: values.iter().map(|(t, v)| (t.to_string(), *v)).collect() }
}

#[test]
fn each_tool_is_timed_against_the_base_over_the_scenarios_both_passed() {
    let tools: Vec<String> = ["virtuoso", "maestro", "speedrun", "argent"].iter().map(|s| s.to_string()).collect();
    let metrics = vec![
        timed("android:exec-launch", &[("virtuoso", Some(1.0)), ("maestro", Some(2.0)), ("speedrun", Some(1.0)), ("argent", None)]),
        timed("android:tam-login_form", &[("virtuoso", Some(2.0)), ("maestro", Some(16.0)), ("speedrun", Some(2.0)), ("argent", None)]),
        // Not a scenario: a suite's total, and a share of runs passed.
        timed("android:exec-total", &[("virtuoso", Some(3.0)), ("maestro", Some(300.0))]),
        timed("android:tam-passed", &[("virtuoso", Some(100.0)), ("maestro", Some(1.0))]),
    ];
    let r: std::collections::BTreeMap<String, Option<f64>> = relative_times(&metrics, &tools, "virtuoso").into_iter().collect();
    // Geometric mean of 2x and 8x: 4x.
    assert!((r["maestro"].unwrap() - 4.0).abs() < 1e-9, "{r:?}");
    assert!((r["speedrun"].unwrap() - 1.0).abs() < 1e-9);
    assert_eq!(r["virtuoso"], Some(1.0));
    assert_eq!(r["argent"], None, "no scenario in common");
}

use virtuoso_bench::fields::relative_chart_svg;

#[test]
fn the_readme_chart_is_a_standalone_svg_with_each_tool_named_and_virtuoso_marked() {
    let times = vec![("virtuoso".to_string(), Some(1.0)), ("maestro".to_string(), Some(6.14)), ("argent".to_string(), None)];
    let svg = relative_chart_svg("Android (Linux runner)", &times, "virtuoso");
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""), "an image file, not a page fragment");
    for want in ["virtuoso", "maestro", "argent", "6.1×", "Android (Linux runner)"] {
        assert!(svg.contains(want), "missing {want}");
    }
    assert!(svg.contains("prefers-color-scheme: dark"), "readable on GitHub's dark theme");
    assert!(svg.contains("class=\"bar base\""), "virtuoso's bar stands out");
    assert!(svg.contains("no scenario passed in common"), "a tool without a value says why");
}
