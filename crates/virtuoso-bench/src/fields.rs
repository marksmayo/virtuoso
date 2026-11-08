//! A report from several fields: a field is one platform on one kind of host (Android on a
//! Linux runner, Android on a Windows runner, iOS on a Mac), with its own results. Each
//! performance metric is a medal event of its own in each field, so a tool that wins on one
//! platform and not another shows both.

use crate::medals::MetricResult;
use crate::metrics::compute;
use crate::orchestrate::Results;

pub struct Field {
    pub name: String,
    pub results: Results,
}

/// Every field's performance metrics, each keyed and labelled by its field.
pub fn field_metrics(fields: &[Field]) -> Vec<MetricResult> {
    fields
        .iter()
        .flat_map(|f| {
            let slug: String = f.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
            compute(&f.results.runs, &f.results.statics, &f.results.tools).into_iter().map(move |mut m| {
                m.key = format!("{slug}:{}", m.key);
                m.label = format!("{} ({})", m.label, f.name);
                m
            })
        })
        .collect()
}
