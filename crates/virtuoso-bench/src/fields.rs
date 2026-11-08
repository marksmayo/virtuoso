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

/// A field's name as a file name: "Android (Linux runner)" is android-linux-runner.
pub fn slug(name: &str) -> String {
    let dashed: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    dashed.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Every tool in any field, in the order they first appear.
pub fn all_tools(fields: &[Field]) -> Vec<String> {
    let mut tools: Vec<String> = Vec::new();
    for t in fields.iter().flat_map(|f| &f.results.tools) {
        if !tools.contains(t) {
            tools.push(t.clone());
        }
    }
    tools
}

/// Every field's performance metrics, each keyed and labelled by its field.
pub fn field_metrics(fields: &[Field]) -> Vec<MetricResult> {
    fields
        .iter()
        .flat_map(|f| {
            let slug = slug(&f.name);
            compute(&f.results.runs, &f.results.statics, &f.results.tools).into_iter().map(move |mut m| {
                m.key = format!("{slug}:{}", m.key);
                m.label = format!("{} ({})", m.label, f.name);
                m
            })
        })
        .collect()
}

/// A per-scenario time (an Execution scenario's, or one app flow's), not a total or a share.
fn is_scenario_time(m: &MetricResult) -> bool {
    let key = m.key.rsplit(':').next().unwrap_or(&m.key);
    let flow = (key.starts_with("app-") || key.starts_with("tam-")) && !key.ends_with("-total") && !key.ends_with("-passed");
    m.unit == "s" && (flow || (key.starts_with("exec-") && key != "exec-total"))
}

/// How long each tool takes against `base`: the geometric mean, over the scenarios both
/// passed, of the tool's time over the base's (4.0 is four times as long). `None` for a tool
/// with no scenario in common with it.
pub fn relative_times(metrics: &[MetricResult], tools: &[String], base: &str) -> Vec<(String, Option<f64>)> {
    tools
        .iter()
        .map(|t| {
            let logs: Vec<f64> = metrics
                .iter()
                .filter(|m| is_scenario_time(m))
                .filter_map(|m| match (m.values.get(t).copied().flatten(), m.values.get(base).copied().flatten()) {
                    (Some(v), Some(b)) if v > 0.0 && b > 0.0 => Some((v / b).ln()),
                    _ => None,
                })
                .collect();
            let mean = (!logs.is_empty()).then(|| (logs.iter().sum::<f64>() / logs.len() as f64).exp());
            (t.clone(), mean)
        })
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A field's README chart: one bar per tool, its time against `base`'s (from
/// [`relative_times`]), fastest first, the base's bar in the accent and the others neutral.
/// An image of its own (GitHub shows a README's SVG as an image), readable in light and dark.
pub fn relative_chart_svg(field: &str, times: &[(String, Option<f64>)], base: &str) -> String {
    use std::fmt::Write;
    let mut rows: Vec<&(String, Option<f64>)> = times.iter().collect();
    rows.sort_by(|a, b| match (a.1, b.1) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.0.cmp(&b.0),
    });
    let max = rows.iter().filter_map(|r| r.1).fold(1.0_f64, f64::max);
    // Inset from its own background: an image follows the system's theme, not the page's, so
    // it brings the surface its text was chosen for.
    let pad = 16.0;
    let (label_w, chart_w, bar_h, gap, top) = (pad + 140.0, 470.0, 18.0, 8.0, pad + 58.0);
    let width = label_w + chart_w + 90.0 + pad;
    let height = top + rows.len() as f64 * (bar_h + gap) + 34.0 + pad;
    let x_of = |v: f64| label_w + v / max * chart_w;
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" role="img" aria-label="{title}: time per scenario against {base}">"#,
        title = esc(field),
        base = esc(base)
    );
    s.push_str(
        "<style>text{font:13px system-ui,-apple-system,'Segoe UI',sans-serif;fill:#0b0b0b}.sub,.axis{fill:#52514e}.axis{font-size:11px}.title{font-size:15px;font-weight:600}\
         .bg{fill:#fcfcfb}.bar{fill:#8f8e86}.base{fill:#2a78d6}.ref{stroke:#52514e;stroke-dasharray:3 3}\
         @media (prefers-color-scheme: dark){.bg{fill:#1a1a19}text{fill:#f0efec}.sub,.axis{fill:#c3c2b7}.base{fill:#3987e5}.ref{stroke:#c3c2b7}}</style>",
    );
    let _ = write!(s, r#"<rect class="bg" width="{width}" height="{height}" rx="8"/>"#);
    let _ = write!(s, r#"<text class="title" x="{pad}" y="{:.1}">{}</text>"#, pad + 18.0, esc(field));
    let _ = write!(
        s,
        r#"<text class="sub" x="{pad}" y="{:.1}">Time per scenario against {}: geometric mean over the scenarios both passed (lower is faster)</text>"#,
        pad + 38.0,
        esc(base)
    );
    for (i, (tool, v)) in rows.iter().enumerate() {
        let y = top + i as f64 * (bar_h + gap);
        let _ = write!(s, r#"<text x="{:.1}" y="{:.1}" text-anchor="end">{}</text>"#, label_w - 10.0, y + bar_h - 4.0, esc(tool));
        match v {
            Some(v) => {
                let class = if tool == base { "bar base" } else { "bar" };
                let w = (x_of(*v) - label_w).max(2.0);
                // Close to the base, the second decimal is the difference.
                let shown = if *v < 2.0 { format!("{v:.2}") } else { format!("{v:.1}") };
                let _ = write!(
                    s,
                    r#"<rect class="{class}" x="{label_w}" y="{y:.1}" width="{w:.1}" height="{bar_h}" rx="3"><title>{t}: {v:.2}× {b}'s time</title></rect><text x="{:.1}" y="{:.1}">{shown}×</text>"#,
                    label_w + w + 6.0,
                    y + bar_h - 4.0,
                    t = esc(tool),
                    b = esc(base)
                );
            }
            None => {
                let _ = write!(s, r#"<text class="sub" x="{label_w}" y="{:.1}">no scenario passed in common</text>"#, y + bar_h - 4.0);
            }
        }
    }
    // The base's own time, as a reference line through every row.
    let (rx, bottom) = (x_of(1.0), top + rows.len() as f64 * (bar_h + gap));
    let _ = write!(s, r#"<line class="ref" x1="{rx:.1}" y1="{:.1}" x2="{rx:.1}" y2="{bottom:.1}"/>"#, top - 6.0);
    let _ = write!(s, r#"<text class="axis" x="{rx:.1}" y="{:.1}" text-anchor="middle">1× = {}</text>"#, bottom + 16.0, esc(base));
    s.push_str("</svg>");
    s
}
