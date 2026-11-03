//! The benchmark report: one self-contained HTML page with the medal tables, every metric,
//! an execution-time chart, the feature matrix, the method, and every raw run.

use crate::features::{category_metrics, feature_metrics, FeatureMatrix, Support};
use crate::medals::{medal_table, rank, Better, Medal, MetricResult, TableRow};
use crate::metrics::{compute, scenario_label, APP_GROUP, DEVICE_SCENARIOS};
use crate::orchestrate::Results;
use std::fmt::Write;

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn medal_glyph(m: Option<Medal>) -> &'static str {
    match m {
        Some(Medal::Gold) => "🥇",
        Some(Medal::Silver) => "🥈",
        Some(Medal::Bronze) => "🥉",
        None => "",
    }
}

fn fmt_value(v: f64, unit: &str) -> String {
    match unit {
        "ms" => format!("{v:.0} ms"),
        "s" => format!("{v:.2} s"),
        "MB" => format!("{v:.1} MB"),
        "%" => format!("{v:.0}%"),
        "% CV" => format!("{v:.1}%"),
        u if u.starts_with("of ") => format!("{} {u}", if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.1}") }),
        u => format!("{v:.2} {u}"),
    }
}

/// The tool's series slot: fixed, so a tool keeps its colour in every chart.
fn slot(tools: &[String], tool: &str) -> usize {
    tools.iter().position(|t| t == tool).unwrap_or(0) + 1
}

const STYLE: &str = r#"
:root { color-scheme: light; --surface:#fcfcfb; --surface-2:#f3f2ef; --ink:#0b0b0b; --ink-2:#52514e; --muted:#7a7974; --line:#dedcd6;
  --s1:#2a78d6; --s2:#eb6834; --s3:#1baf7a; --s4:#eda100; --s5:#e87ba4; --gold:#b98900; --yes:#1a7f37; --partial:#9a6700; --no:#b42318; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { color-scheme: dark; --surface:#1a1a19; --surface-2:#232321; --ink:#f0efec; --ink-2:#c3c2b7; --muted:#8f8e86; --line:#3a3936;
  --s1:#3987e5; --s2:#d95926; --s3:#199e70; --s4:#c98500; --s5:#d55181; --gold:#e3b341; --yes:#3fb950; --partial:#d29922; --no:#f85149; } }
:root[data-theme="dark"] { color-scheme: dark; --surface:#1a1a19; --surface-2:#232321; --ink:#f0efec; --ink-2:#c3c2b7; --muted:#8f8e86; --line:#3a3936;
  --s1:#3987e5; --s2:#d95926; --s3:#199e70; --s4:#c98500; --s5:#d55181; --gold:#e3b341; --yes:#3fb950; --partial:#d29922; --no:#f85149; }
* { box-sizing: border-box; }
body { margin:0; background:var(--surface); color:var(--ink); font:15px/1.55 system-ui,-apple-system,"Segoe UI",sans-serif; }
main { max-width:1080px; margin:0 auto; padding-block:32px 64px; padding-left:max(16px, env(safe-area-inset-left, 0px)); padding-right:max(16px, env(safe-area-inset-right, 0px)); }
h1 { font-size:28px; margin:0 0 6px; letter-spacing:-0.01em; } h2 { font-size:20px; margin:40px 0 10px; } h3 { font-size:16px; margin:24px 0 8px; }
p.lead { color:var(--ink-2); margin:0 0 20px; max-width:760px; }
.muted { color:var(--muted); } .small { font-size:13px; }
table { border-collapse:collapse; width:100%; font-variant-numeric:tabular-nums; }
th, td { text-align:left; padding:7px 10px; border-bottom:1px solid var(--line); vertical-align:top; }
th { font-size:12px; text-transform:uppercase; letter-spacing:0.04em; color:var(--muted); font-weight:600; }
td.num, th.num { text-align:right; white-space:nowrap; }
.wrap { overflow-x:auto; }
.medals td.num { font-size:17px; }
.medals tr:first-child td { font-weight:700; }
.best { font-weight:700; }
.swatch { display:inline-block; width:10px; height:10px; border-radius:3px; margin-right:7px; vertical-align:baseline; }
.sw1 { background:var(--s1); } .sw2 { background:var(--s2); } .sw3 { background:var(--s3); } .sw4 { background:var(--s4); } .sw5 { background:var(--s5); }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:12px; margin:16px 0; }
.card { background:var(--surface-2); border-radius:10px; padding:14px 16px; }
.card .big { font-size:26px; font-weight:700; } .card .label { color:var(--ink-2); font-size:13px; }
.yes { color:var(--yes); } .partial { color:var(--partial); } .no { color:var(--no); }
details { border:1px solid var(--line); border-radius:10px; margin:10px 0; background:var(--surface); }
summary { cursor:pointer; padding:10px 14px; font-weight:600; }
details > div { padding:0 14px 12px; }
.chart text { fill:var(--ink-2); font-size:12px; } .chart .val { fill:var(--ink); font-size:11px; }
.chart .grid { stroke:var(--line); stroke-width:1; }
.bar1 { fill:var(--s1); } .bar2 { fill:var(--s2); } .bar3 { fill:var(--s3); } .bar4 { fill:var(--s4); } .bar5 { fill:var(--s5); }
.chart rect.bar:hover { opacity:0.8; }
.legend { display:flex; gap:18px; flex-wrap:wrap; margin:6px 0 0; font-size:13px; color:var(--ink-2); }
.notice { background:var(--surface-2); border-left:3px solid var(--gold); padding:10px 14px; border-radius:6px; max-width:760px; }
code { font-size:13px; background:var(--surface-2); padding:1px 5px; border-radius:4px; }
"#;

fn table_html(id: &str, rows: &[TableRow], tools: &[String]) -> String {
    let mut h = format!(r#"<div class="wrap"><table class="medals" id="{id}"><thead><tr><th>Tool</th><th class="num">🥇 Gold</th><th class="num">🥈 Silver</th><th class="num">🥉 Bronze</th><th class="num">Total</th></tr></thead><tbody>"#);
    for r in rows {
        let _ = write!(
            h,
            r#"<tr><td><span class="swatch sw{s}"></span><span>{t}</span></td><td class="num">{g}</td><td class="num">{si}</td><td class="num">{b}</td><td class="num">{tot}</td></tr>"#,
            s = slot(tools, &r.tool),
            t = esc(&r.tool),
            g = r.gold,
            si = r.silver,
            b = r.bronze,
            tot = r.total()
        );
    }
    h.push_str("</tbody></table></div>");
    h
}

/// Per category: each tool's score and the medals its features won.
fn category_rows(features: &FeatureMatrix, events: &[MetricResult], scores: &[MetricResult]) -> String {
    let tools = &features.tools;
    let mut h = String::from(r#"<div class="wrap"><table><thead><tr><th>Category</th>"#);
    for t in tools {
        let _ = write!(h, r#"<th class="num"><span class="swatch sw{}"></span>{}</th>"#, slot(tools, t), esc(t));
    }
    h.push_str("</tr></thead><tbody>");
    for (c, score) in features.categories.iter().zip(scores) {
        let in_category: Vec<MetricResult> = events.iter().filter(|m| m.group == c.name).cloned().collect();
        let table = medal_table(&in_category, tools);
        let _ = write!(h, "<tr><td>{}</td>", esc(&c.name));
        for t in tools {
            let row = table.iter().find(|r| &r.tool == t);
            let (g, s, b) = row.map_or((0, 0, 0), |r| (r.gold, r.silver, r.bronze));
            let v = score.values.get(t).copied().flatten().unwrap_or(0.0);
            let _ = write!(h, r#"<td class="num">{v} of {}<div class="muted small">🥇{g} 🥈{s} 🥉{b}</div></td>"#, c.features.len());
        }
        h.push_str("</tr>");
    }
    h.push_str("</tbody></table></div>");
    h
}

fn metric_rows(metrics: &[MetricResult], tools: &[String]) -> String {
    let mut h = String::from(r#"<div class="wrap"><table><thead><tr><th>Metric</th>"#);
    for t in tools {
        let _ = write!(h, r#"<th class="num"><span class="swatch sw{}"></span>{}</th>"#, slot(tools, t), esc(t));
    }
    h.push_str(r#"<th>Better</th></tr></thead><tbody>"#);
    for m in metrics {
        let medals = rank(m);
        let _ = write!(h, "<tr><td>{}</td>", esc(&m.label));
        for t in tools {
            let cell = match m.values.get(t).copied().flatten() {
                Some(v) => {
                    let medal = medals.get(t).copied().flatten();
                    format!(r#"<td class="num{}">{} {}</td>"#, if medal == Some(Medal::Gold) { " best" } else { "" }, fmt_value(v, &m.unit), medal_glyph(medal))
                }
                None => r#"<td class="num muted">—</td>"#.to_string(),
            };
            h.push_str(&cell);
        }
        let _ = write!(h, r#"<td class="muted small">{}</td></tr>"#, if m.better == Better::Lower { "lower" } else { "higher" });
    }
    h.push_str("</tbody></table></div>");
    h
}

/// Grouped horizontal bars: one group per scenario, one bar per tool, value labels at the end.
fn execution_chart(metrics: &[MetricResult], tools: &[String]) -> String {
    let rows: Vec<&MetricResult> = DEVICE_SCENARIOS.iter().filter_map(|s| metrics.iter().find(|m| m.key == format!("exec-{s}"))).collect();
    bar_chart(&rows, tools)
}

fn bar_chart(rows: &[&MetricResult], tools: &[String]) -> String {
    let max = rows.iter().flat_map(|m| m.values.values().filter_map(|v| *v)).fold(0.0_f64, f64::max).max(0.001);
    let (label_w, chart_w, bar_h, gap, group_gap) = (150.0, 560.0, 14.0, 2.0, 18.0);
    let group_h = tools.len() as f64 * (bar_h + gap) - gap;
    let height = rows.len() as f64 * (group_h + group_gap) + 24.0;
    let width = label_w + chart_w + 70.0;
    let mut s = format!(r#"<svg class="chart" viewBox="0 0 {width} {height}" width="100%" role="img" aria-label="Median wall time per scenario, seconds">"#);
    // A recessive grid at round seconds.
    let step = if max > 40.0 { 10.0 } else if max > 16.0 { 5.0 } else if max > 6.0 { 2.0 } else { 1.0 };
    let mut x = 0.0;
    while x <= max + 1e-9 {
        let px = label_w + x / max * chart_w;
        let _ = write!(s, r#"<line class="grid" x1="{px:.1}" y1="0" x2="{px:.1}" y2="{:.1}"/><text x="{px:.1}" y="{:.1}" text-anchor="middle">{x:.0}s</text>"#, height - 18.0, height - 4.0);
        x += step;
    }
    for (gi, m) in rows.iter().enumerate() {
        let y0 = gi as f64 * (group_h + group_gap);
        let _ = write!(s, r#"<text x="{:.1}" y="{:.1}" text-anchor="end">{}</text>"#, label_w - 10.0, y0 + group_h / 2.0 + 4.0, esc(&m.label));
        for (ti, t) in tools.iter().enumerate() {
            let Some(v) = m.values.get(t).copied().flatten() else { continue };
            let y = y0 + ti as f64 * (bar_h + gap);
            let w = (v / max * chart_w).max(2.0);
            let _ = write!(
                s,
                r#"<rect class="bar bar{n}" x="{label_w}" y="{y:.1}" width="{w:.1}" height="{bar_h}" rx="4"><title>{t}: {v:.2} s</title></rect><text class="val" x="{:.1}" y="{:.1}">{v:.1}s</text>"#,
                label_w + w + 5.0,
                y + bar_h - 3.0,
                n = slot(tools, t),
                t = esc(t)
            );
        }
    }
    s.push_str("</svg>");
    let mut legend = String::from(r#"<div class="legend">"#);
    for t in tools {
        let _ = write!(legend, r#"<span><span class="swatch sw{}"></span>{}</span>"#, slot(tools, t), esc(t));
    }
    legend.push_str("</div>");
    s + &legend
}

/// The whole report as a standalone HTML document.
pub fn render(r: &Results, features: &FeatureMatrix) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1,viewport-fit=cover\"></head><body>{}</body></html>
",
        render_fragment(r, features)
    )
}

/// The report without the document wrapper, for hosts that add their own (an Artifact).
pub fn render_fragment(r: &Results, features: &FeatureMatrix) -> String {
    let perf = compute(&r.runs, &r.statics, &r.tools);
    let feats = feature_metrics(features);
    let categories = category_metrics(features);
    let all: Vec<MetricResult> = perf.iter().chain(&feats).cloned().collect();
    let overall = medal_table(&all, &r.tools);
    let perf_table = medal_table(&perf, &r.tools);
    let feat_table = medal_table(&feats, &r.tools);

    let mut h = String::new();
    let _ = write!(
        h,
        r#"<title>Runner Benchmark Medals</title><meta name="claude:viewport-fit-sides" content="cover"><style>{STYLE}</style><main>
<h1>Mobile test runner benchmark</h1>
<p class="lead">{} run the same scenarios on the same Android emulator. Each metric awards gold, silver and bronze, and so does each feature where the tools differ (full support beats partial; no support wins nothing; features every tool has award nothing). Results within 2% of each other tie.</p>"#,
        esc(&tool_list(&r.tools))
    );
    if let Some(status) = r.meta.get("Status") {
        let _ = write!(h, r#"<p class="notice"><b>Partial results.</b> {}</p>"#, esc(status));
    }
    let leader = &overall[0];
    let fastest = perf.iter().find(|m| m.key == "exec-total").and_then(|m| {
        let mut v: Vec<(&String, f64)> = m.values.iter().filter_map(|(t, v)| v.map(|v| (t, v))).collect();
        v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        (v.len() >= 2).then(|| (v[0].0.clone(), v[0].1, v[1].0.clone(), v[1].1))
    });
    let most_features = categories.iter().fold(std::collections::BTreeMap::<String, f64>::new(), |mut acc, m| {
        for (t, v) in &m.values {
            *acc.entry(t.clone()).or_default() += v.unwrap_or(0.0);
        }
        acc
    });
    let total_features: usize = features.categories.iter().map(|c| c.features.len()).sum();
    let (feat_leader, feat_score) = most_features.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(t, v)| (t.clone(), *v)).unwrap_or_default();
    h.push_str(r#"<div class="cards">"#);
    let _ = write!(h, r#"<div class="card"><div class="label">Most medals</div><div class="big">{}</div><div class="label">{} gold · {} total</div></div>"#, esc(&leader.tool), leader.gold, leader.total());
    if let Some((t, v, t2, v2)) = &fastest {
        let _ = write!(h, r#"<div class="card"><div class="label">Fastest whole suite</div><div class="big">{} </div><div class="label">{v:.1} s, {:.1}× faster than {}</div></div>"#, esc(t), v2 / v, esc(t2));
    }
    let _ = write!(h, r#"<div class="card"><div class="label">Most features</div><div class="big">{}</div><div class="label">{feat_score:.1} of {total_features} features</div></div>"#, esc(&feat_leader));
    h.push_str("</div>");

    h.push_str("<h2>Overall medal table</h2>");
    h.push_str(&table_html("overall", &overall, &r.tools));
    let _ = write!(h, r#"<p class="muted small">{} performance metrics and {} features, each a medal event.</p>"#, perf.len(), feats.len());

    h.push_str("<h2>Performance</h2>");
    h.push_str(&table_html("performance", &perf_table, &r.tools));
    for group in ["Start-up", "Execution", "Resources", "Reliability"] {
        let ms: Vec<MetricResult> = perf.iter().filter(|m| m.group == group).cloned().collect();
        let _ = write!(h, "<h3>{group}</h3>");
        if group == "Execution" {
            h.push_str(r#"<p class="muted small">Median wall time of each tool's passing runs, from starting the command to its exit (connecting to the device included).</p>"#);
            h.push_str(&execution_chart(&perf, &r.tools));
        }
        h.push_str(&metric_rows(&ms, &r.tools));
        if group == "Start-up" {
            for t in &r.tools {
                let invalid: Vec<_> = r.runs.iter().filter(|x| &x.tool == t && x.scenario == "invalid").collect();
                if !invalid.is_empty() && invalid.iter().all(|x| !x.passed) {
                    let _ = write!(
                        h,
                        r#"<p class="small"><span class="no">✗</span> {} exited 0 on a flow with an unknown command (it printed the error), so a CI job would report that run as passing. It gets no value for "Invalid flow reported".</p>"#,
                        esc(t)
                    );
                }
            }
        }
    }

    let app: Vec<MetricResult> = perf.iter().filter(|m| m.group == APP_GROUP).cloned().collect();
    if !app.is_empty() {
        let missing: Vec<&String> = r.tools.iter().filter(|t| app.iter().all(|m| m.values.get(*t).copied().flatten().is_none())).collect();
        let _ = write!(
            h,
            r#"<h3 id="bench-app">{APP_GROUP}</h3><p class="muted small">The flows of speedrun's own benchmark app (<code>bench/app</code> in github.com/hubastard/speedrun): a React Native app built to be hard to test, with screens that never stop moving, popups, lists that load as they scroll, keypads and a Jetpack Compose screen. Each flow's time is from start to exit; a failed flow has no time.{}</p>"#,
            if missing.is_empty() { String::new() } else { format!(" No value for {}: its tests are code, not flow files.", esc(&tool_list(&missing.iter().map(|t| t.to_string()).collect::<Vec<_>>()))) }
        );
        let flows: Vec<&MetricResult> = app.iter().filter(|m| m.key != "app-total" && m.key != "app-passed").collect();
        h.push_str(&bar_chart(&flows, &r.tools));
        h.push_str(&metric_rows(&app, &r.tools));
    }

    if r.runs.iter().any(|run| run.tool == "virtuoso-windows") {
        let pair: Vec<String> = ["virtuoso-windows", "maestro-runner"].iter().map(|s| s.to_string()).collect();
        let native: Vec<MetricResult> = compute(&r.runs, &r.statics, &pair)
            .into_iter()
            .filter(|m| m.group == "Execution" || m.key == "host-memory" || m.key == "host-cpu" || m.key == "cli-startup" || m.key == "first-command")
            .collect();
        h.push_str(r#"<h2 id="native">Supplement: both natively on Windows</h2><p class="muted small">virtuoso's native Windows build against maestro-runner, both measured the same way (Windows process handle and job). Not in the medal tables.</p>"#);
        h.push_str(&metric_rows(&native, &pair));
    }

    h.push_str("<h2>Features</h2>");
    h.push_str(&table_html("features", &feat_table, &r.tools));
    let outside: Vec<String> = r.tools.iter().filter(|t| !features.tools.contains(t)).cloned().collect();
    if !outside.is_empty() {
        let _ = write!(h, r#"<p class="muted small">{} {} not in the feature matrix, which compares Maestro flow runners: no feature medals.</p>"#, esc(&tool_list(&outside)), if outside.len() == 1 { "is" } else { "are" });
    }
    h.push_str(&category_rows(features, &feats, &categories));
    h.push_str(r#"<p class="muted small">Every feature that shows a gap between the tools is a medal event: tools with full support share gold, partial support takes the next medal, no support wins nothing. A feature every tool supports the same way is listed but awards no medal. A category's score counts a supported feature as 1 and a partial one as ½. Open a category for every feature, its medals, and the reason behind each partial or missing one.</p>"#);
    for c in &features.categories {
        let gaps = c.features.iter().filter(|f| !f.shared(&features.tools)).count();
        let _ = write!(
            h,
            "<details><summary>{} ({} features, {gaps} with a gap)</summary><div class=\"wrap\"><table><thead><tr><th>Feature</th>",
            esc(&c.name),
            c.features.len()
        );
        for t in &features.tools {
            let _ = write!(h, "<th>{}</th>", esc(t));
        }
        h.push_str("</tr></thead><tbody>");
        for f in &c.features {
            let shared = f.shared(&features.tools);
            let mark = if shared { r#"<div class="muted small">every tool: no medal</div>"# } else { "" };
            let _ = write!(h, "<tr><td>{}{mark}</td>", esc(&f.label));
            let medals = feats.iter().find(|m| m.key == format!("feature-{}", f.key)).map(rank).unwrap_or_default();
            for t in &features.tools {
                let s = f.support.get(t).copied().unwrap_or(Support::No);
                let (cls, word) = match s {
                    Support::Yes => ("yes", "✓ yes"),
                    Support::Partial => ("partial", "◐ partial"),
                    Support::No => ("no", "✗ no"),
                };
                let note = f.notes.get(t).map(|n| format!(r#"<div class="muted small">{}</div>"#, esc(n))).unwrap_or_default();
                let medal = medal_glyph(medals.get(t).copied().flatten());
                let _ = write!(h, r#"<td><span class="{cls}">{word}</span> {medal}{note}</td>"#);
            }
            h.push_str("</tr>");
        }
        h.push_str("</tbody></table></div></details>");
    }

    h.push_str("<h2>Method and environment</h2><div class=\"wrap\"><table><tbody>");
    for (k, v) in &r.meta {
        let _ = write!(h, "<tr><td class=\"muted\">{}</td><td>{}</td></tr>", esc(k), esc(v));
    }
    for t in &r.tools {
        if let Some(s) = r.statics.get(t) {
            let _ = write!(h, "<tr><td class=\"muted\">{} version</td><td>{}</td></tr>", esc(t), esc(&s.version));
        }
    }
    h.push_str("</tbody></table></div>");
    h.push_str(r#"<ul class="small">
<li><b>Scenarios</b> are Maestro flows in <code>bench/flows</code>, the same file for every flow runner, run against Android's Settings app. Appium runs the same steps as code (<code>bench/appium/run.mjs</code>: a UiAutomator2 session, UiSelector lookups, the server in the test's own process, started and stopped with each run); it has no flow file to reject, so no invalid-flow value. Before every run the device is reset: test agents stopped, adb forwards removed, Settings' data cleared, animations back on, portrait, light mode, home screen.</li>
<li>Each tool runs each scenario once as an unrecorded warm-up, then the recorded iterations; tools take turns within an iteration so drift spreads evenly.</li>
<li><b>Wall time</b> is from starting the command to its exit. A scenario's value is the median of passing runs; a tool that passed fewer than half its runs gets no value (and no medal) for that scenario.</li>
<li><b>Peak host memory</b> is the peak working set (Windows) or peak RSS (Linux, GNU time) of the tool's main process. <b>Host CPU</b> is user + kernel time. <b>Device agent memory</b> is the highest resident memory of the tool's on-device processes, sampled every 0.5 s.</li>
<li><b>Install size</b> counts what a user needs on disk to drive Android: the binary, plus maestro-runner's Android driver APKs; Maestro's <code>lib</code> folder (not the Java runtime it needs); Appium's server and UiAutomator2 driver (not Node.js).</li>
<li><b>Run-to-run variation</b> is the mean coefficient of variation of wall time over the scenarios with at least three passing runs.</li>
<li><b>Best of every run for the competition.</b> The other tools keep every batch of runs recorded at their current version, and are scored per scenario on their fastest batch, and on their lowest memory and CPU figures from any batch. virtuoso is scored on its latest batch only. A new competitor version discards its earlier batches.</li>
</ul>"#);

    h.push_str("<details><summary>Every run</summary><div class=\"wrap\"><table><thead><tr><th>Scenario</th><th>Tool</th><th class=\"num\">#</th><th class=\"num\">Wall</th><th class=\"num\">Peak memory</th><th class=\"num\">CPU</th><th class=\"num\">Agent</th><th>Result</th></tr></thead><tbody>");
    for run in &r.runs {
        let label = if DEVICE_SCENARIOS.contains(&run.scenario.as_str()) { scenario_label(&run.scenario).to_string() } else { run.scenario.clone() };
        let _ = write!(
            h,
            r#"<tr><td>{}</td><td>{}</td><td class="num">{}</td><td class="num">{:.0} ms</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td>{}</td></tr>"#,
            esc(&label),
            esc(&run.tool),
            run.iteration + 1,
            run.wall_ms,
            run.peak_rss_kb.map(|k| format!("{:.1} MB", k / 1024.0)).unwrap_or_default(),
            run.cpu_ms.map(|c| format!("{c:.0} ms")).unwrap_or_default(),
            run.agent_rss_kb.map(|k| format!("{:.0} MB", k / 1024.0)).unwrap_or_default(),
            if run.passed { r#"<span class="yes">pass</span>"#.to_string() } else { format!(r#"<span class="no">fail</span><div class="muted small">{}</div>"#, esc(run.error.as_deref().unwrap_or(""))) }
        );
    }
    h.push_str("</tbody></table></div></details></main></body></html>\n");
    h
}

/// "a, b and c".
fn tool_list(tools: &[String]) -> String {
    match tools {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

const README_START: &str = "<!-- benchmark:start -->";
const README_END: &str = "<!-- benchmark:end -->";

fn markdown_medals(rows: &[TableRow]) -> String {
    let mut m = String::from("| Tool | 🥇 | 🥈 | 🥉 | Total |\n|---|---:|---:|---:|---:|\n");
    for r in rows {
        let _ = writeln!(m, "| {} | {} | {} | {} | {} |", r.tool, r.gold, r.silver, r.bronze, r.total());
    }
    m
}

/// The results as Markdown for the README: the overall and performance medal tables, and
/// every performance metric with the gold value in bold.
pub fn readme_summary(r: &Results, features: &FeatureMatrix) -> String {
    let perf = compute(&r.runs, &r.statics, &r.tools);
    let feats = feature_metrics(features);
    let all: Vec<MetricResult> = perf.iter().chain(&feats).cloned().collect();
    let mut m = String::new();
    if let Some(status) = r.meta.get("Status") {
        let _ = writeln!(m, "> **Partial results.** {status}\n");
    }
    let device = r.meta.get("Device").map(|d| format!(" Device: {d}.")).unwrap_or_default();
    let _ = writeln!(
        m,
        "Each of the {} performance metrics and {} features is a medal event.{device} Written by `virtuoso-bench report`, which also writes the full HTML report.\n",
        perf.len(),
        feats.len()
    );
    m.push_str("**Overall**\n\n");
    m.push_str(&markdown_medals(&medal_table(&all, &r.tools)));
    m.push_str("\n**Performance**\n\n");
    m.push_str(&markdown_medals(&medal_table(&perf, &r.tools)));
    let _ = write!(m, "\n| Metric | {} |\n|---|{}\n", r.tools.join(" | "), "---:|".repeat(r.tools.len()));
    for metric in &perf {
        let medals = rank(metric);
        let _ = write!(m, "| {} |", metric.label);
        for t in &r.tools {
            let cell = match metric.values.get(t).copied().flatten() {
                Some(v) if medals.get(t).copied().flatten() == Some(Medal::Gold) => format!("**{}**", fmt_value(v, &metric.unit)),
                Some(v) => fmt_value(v, &metric.unit),
                None => "—".to_string(),
            };
            let _ = write!(m, " {cell} |");
        }
        m.push('\n');
    }
    m
}

/// `readme` with the text between the benchmark markers replaced by `section`. Without the
/// markers, they go in before the License heading, or at the end.
pub fn update_readme(readme: &str, section: &str) -> String {
    let block = format!("{README_START}\n{section}{README_END}");
    if let (Some(s), Some(e)) = (readme.find(README_START), readme.find(README_END)) {
        if s < e {
            return format!("{}{block}{}", &readme[..s], &readme[e + README_END.len()..]);
        }
    }
    match readme.find("\n## License") {
        Some(i) => format!("{}{block}\n{}", &readme[..i + 1], &readme[i..]),
        None => format!("{}\n{block}\n", readme.trim_end_matches('\n').to_string() + "\n"),
    }
}
