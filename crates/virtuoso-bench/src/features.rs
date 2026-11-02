//! The feature matrix (bench/features.json): what each tool supports, by category. Each
//! feature that not every tool has alike is a medal event (full support beats partial; no
//! support wins nothing), and each
//! category has a score: a supported feature scores 1, a partial one 0.5.

use crate::medals::{Better, MetricResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    Yes,
    Partial,
    No,
}

impl Support {
    pub fn score(self) -> f64 {
        match self {
            Support::Yes => 1.0,
            Support::Partial => 0.5,
            Support::No => 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub key: String,
    pub label: String,
    pub support: BTreeMap<String, Support>,
    /// Why a tool's support is partial or missing, or where it is documented.
    #[serde(default)]
    pub notes: BTreeMap<String, String>,
}

impl Feature {
    /// Whether every tool supports this feature the same way (all fully, or all partly): it shows
    /// no gap between them, so it is not a medal event.
    pub fn shared(&self, tools: &[String]) -> bool {
        let first = self.support.get(&tools[0]).copied().unwrap_or(Support::No);
        first != Support::No && tools.iter().all(|t| self.support.get(t).copied().unwrap_or(Support::No) == first)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureMatrix {
    pub tools: Vec<String>,
    pub categories: Vec<Category>,
}

pub fn category_metrics(m: &FeatureMatrix) -> Vec<MetricResult> {
    m.categories
        .iter()
        .map(|c| MetricResult {
            key: format!("features-{}", c.name.to_lowercase().replace(|ch: char| !ch.is_alphanumeric(), "-")),
            label: c.name.clone(),
            group: "Features".into(),
            unit: format!("of {}", c.features.len()),
            better: Better::Higher,
            values: m
                .tools
                .iter()
                .map(|t| {
                    let score = c.features.iter().map(|f| f.support.get(t).map_or(0.0, |s| s.score())).sum();
                    (t.clone(), Some(score))
                })
                .collect(),
        })
        .collect()
}

/// One medal event per feature that shows a gap between the tools, grouped by its category: full
/// support scores 1, partial 0.5, and a tool without it has no value (so no medal).
pub fn feature_metrics(m: &FeatureMatrix) -> Vec<MetricResult> {
    m.categories
        .iter()
        .flat_map(|c| {
            c.features.iter().filter(|f| !f.shared(&m.tools)).map(|f| MetricResult {
                key: format!("feature-{}", f.key),
                label: f.label.clone(),
                group: c.name.clone(),
                unit: String::new(),
                better: Better::Higher,
                values: m
                    .tools
                    .iter()
                    .map(|t| (t.clone(), f.support.get(t).filter(|s| **s != Support::No).map(|s| s.score())))
                    .collect(),
            })
        })
        .collect()
}
