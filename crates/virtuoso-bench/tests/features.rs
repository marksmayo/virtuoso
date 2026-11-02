//! The feature matrix and the medal metrics it yields.

use virtuoso_bench::features::{category_metrics, feature_metrics, FeatureMatrix, Support};
use virtuoso_bench::medals::{rank, Better, Medal};

const SAMPLE: &str = r#"{
  "tools": ["a", "b"],
  "categories": [
    {"name": "Commands", "features": [
      {"key": "tapOn", "label": "tapOn", "support": {"a": "yes", "b": "yes"}},
      {"key": "travel", "label": "travel", "support": {"a": "partial", "b": "no"}, "notes": {"b": "not implemented"}}
    ]},
    {"name": "Platforms", "features": [
      {"key": "ios", "label": "iOS simulator", "support": {"a": "no", "b": "yes"}}
    ]}
  ]
}"#;

#[test]
fn the_matrix_parses_support_levels() {
    let m: FeatureMatrix = serde_json::from_str(SAMPLE).unwrap();
    assert_eq!(m.categories[0].features[1].support["a"], Support::Partial);
    assert_eq!(m.categories[0].features[1].notes["b"], "not implemented");
}

#[test]
fn each_category_is_a_metric_scoring_yes_one_and_partial_half() {
    let m: FeatureMatrix = serde_json::from_str(SAMPLE).unwrap();
    let metrics = category_metrics(&m);
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0].label, "Commands");
    assert_eq!(metrics[0].better, Better::Higher);
    assert_eq!(metrics[0].group, "Features");
    assert_eq!(metrics[0].values["a"], Some(1.5));
    assert_eq!(metrics[0].values["b"], Some(1.0));
    assert_eq!(metrics[1].values["a"], Some(0.0));
}

#[test]
fn a_tool_missing_from_a_feature_counts_as_unsupported() {
    let json = r#"{"tools": ["a", "b"], "categories": [{"name": "C", "features": [{"key": "k", "label": "k", "support": {"a": "yes"}}]}]}"#;
    let m: FeatureMatrix = serde_json::from_str(json).unwrap();
    assert_eq!(category_metrics(&m)[0].values["b"], Some(0.0));
}

#[test]
fn the_shipped_matrix_is_valid_and_covers_every_tool_for_every_feature() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/features.json")).unwrap();
    let m: FeatureMatrix = serde_json::from_str(&text).unwrap();
    assert_eq!(m.tools, vec!["virtuoso", "maestro-runner", "speedrun"]);
    let mut keys = std::collections::HashSet::new();
    for c in &m.categories {
        for f in &c.features {
            assert!(keys.insert(f.key.clone()), "duplicate feature {}", f.key);
            for t in &m.tools {
                assert!(f.support.contains_key(t), "{} has no entry for {t}", f.key);
            }
        }
    }
    assert!(keys.len() >= 80, "only {} features", keys.len());
}

#[test]
fn each_feature_is_a_medal_event_and_no_support_wins_nothing() {
    let m: FeatureMatrix = serde_json::from_str(SAMPLE).unwrap();
    let events = feature_metrics(&m);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].label, "travel");
    assert_eq!(events[0].group, "Commands");
    let medals = |i: usize| rank(&events[i]);
    assert_eq!(medals(0)["a"], Some(Medal::Gold), "partial beats no support");
    assert_eq!(medals(0)["b"], None, "no support, no medal");
    assert_eq!(medals(1)["a"], None);
    assert_eq!(medals(1)["b"], Some(Medal::Gold));
}

#[test]
fn a_feature_every_tool_supports_alike_is_not_a_medal_event() {
    let json = r#"{"tools": ["a", "b"], "categories": [{"name": "C", "features": [
        {"key": "all", "label": "all", "support": {"a": "yes", "b": "yes"}},
        {"key": "half", "label": "half", "support": {"a": "partial", "b": "partial"}},
        {"key": "gap", "label": "gap", "support": {"a": "yes", "b": "partial"}}
    ]}]}"#;
    let m: FeatureMatrix = serde_json::from_str(json).unwrap();
    let keys: Vec<String> = feature_metrics(&m).into_iter().map(|e| e.key).collect();
    assert_eq!(keys, ["feature-gap"]);
    assert!(m.categories[0].features[0].shared(&m.tools));
    assert!(!m.categories[0].features[2].shared(&m.tools));
}

#[test]
fn full_support_beats_partial_support_of_a_feature() {
    let json = r#"{"tools": ["a", "b"], "categories": [{"name": "C", "features": [{"key": "k", "label": "k", "support": {"a": "partial", "b": "yes"}}]}]}"#;
    let m: FeatureMatrix = serde_json::from_str(json).unwrap();
    let medals = rank(&feature_metrics(&m)[0]);
    assert_eq!(medals["b"], Some(Medal::Gold));
    assert_eq!(medals["a"], Some(Medal::Silver));
}
