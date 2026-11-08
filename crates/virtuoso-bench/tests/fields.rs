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
