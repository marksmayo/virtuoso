//! Which flows a run picks, as Maestro picks them:
//!
//! - A file given on the command line runs as is.
//! - A folder runs the flows its `config.yaml` names (`flows:` globs relative to the folder,
//!   `!` to leave out; by default `*`, the top level), never config.yaml itself.
//! - `includeTags` / `excludeTags` from the config and the command line filter folder flows.
//! - `executionOrder.flowsOrder` runs those flows first, in that order, one after another.

use crate::flow::{parse_flow, Flow};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_yaml::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Selection {
    pub flows: Vec<Flow>,
    /// The first `sequential` flows come from executionOrder and run in that order.
    pub sequential: usize,
    pub continue_on_failure: bool,
    pub test_output_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct Config {
    flows: Vec<String>,
    include_tags: Vec<String>,
    exclude_tags: Vec<String>,
    order: Vec<String>,
    continue_on_failure: Option<bool>,
    test_output_dir: Option<String>,
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::Sequence(s) => s.iter().filter_map(crate::flow::scalar).collect(),
        Value::Null => vec![],
        v => crate::flow::scalar(v).into_iter().collect(),
    }
}

fn read_config(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let v: Value = serde_yaml::from_str(&text).with_context(|| format!("{}: invalid YAML", path.display()))?;
    let order = &v["executionOrder"];
    Ok(Config {
        flows: strings(&v["flows"]),
        include_tags: strings(&v["includeTags"]),
        exclude_tags: strings(&v["excludeTags"]),
        order: strings(&order["flowsOrder"]),
        continue_on_failure: order["continueOnFailure"].as_bool(),
        test_output_dir: crate::flow::scalar(&v["testOutputDir"]),
    })
}

/// A glob over `/`-separated relative paths: `*` and `?` stay within a folder, `**` crosses them.
pub fn glob_regex(glob: &str) -> Regex {
    let mut re = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                re.push_str(".*");
                i += 1;
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                    re.push_str("/?");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).unwrap()
}

fn is_flow_file(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()), Some("yaml" | "yml")) && p.file_stem().is_some_and(|s| s != "config")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if is_flow_file(&p) {
            out.push(p);
        }
    }
}

fn relative(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// The flows to run for `paths`, filtered by tags, in run order.
pub fn collect(paths: &[PathBuf], include_tags: &[String], exclude_tags: &[String], config: Option<&Path>) -> Result<Selection> {
    let mut sel = Selection { continue_on_failure: true, ..Default::default() };
    let mut ordered: Vec<(usize, Flow)> = Vec::new();
    let mut rest: Vec<Flow> = Vec::new();
    for path in paths {
        if path.is_file() {
            rest.push(load(path)?);
            continue;
        }
        if !path.is_dir() {
            return Err(anyhow!("{} does not exist", path.display()));
        }
        let cfg_path = config.map(Path::to_path_buf).unwrap_or_else(|| path.join("config.yaml"));
        let cfg = if cfg_path.exists() { read_config(&cfg_path)? } else { Config::default() };
        if let Some(c) = cfg.continue_on_failure {
            sel.continue_on_failure = c;
        }
        if let Some(d) = &cfg.test_output_dir {
            sel.test_output_dir = Some(path.join(d));
        }
        let globs = if cfg.flows.is_empty() { vec!["*".to_string()] } else { cfg.flows.clone() };
        let (exclude, include): (Vec<&String>, Vec<&String>) = globs.iter().partition(|g| g.starts_with('!'));
        let include: Vec<Regex> = include.iter().map(|g| glob_regex(g)).collect();
        let exclude: Vec<Regex> = exclude.iter().map(|g| glob_regex(&g[1..])).collect();
        let mut files = Vec::new();
        walk(path, &mut files);
        files.retain(|f| {
            if f == &cfg_path {
                return false;
            }
            let rel = relative(path, f);
            let no_ext = rel.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(rel.clone());
            let hit = |r: &Regex| r.is_match(&rel) || r.is_match(&no_ext);
            include.iter().any(hit) && !exclude.iter().any(hit)
        });
        files.sort_by_key(|f| relative(path, f));
        let inc: Vec<&String> = include_tags.iter().chain(&cfg.include_tags).collect();
        let exc: Vec<&String> = exclude_tags.iter().chain(&cfg.exclude_tags).collect();
        for f in files {
            let flow = load(&f)?;
            let tags = &flow.config.tags;
            if (!inc.is_empty() && !inc.iter().any(|t| tags.contains(t))) || exc.iter().any(|t| tags.contains(t)) {
                continue;
            }
            let stem = f.file_stem().unwrap().to_string_lossy().to_string();
            let named = cfg.order.iter().position(|o| *o == stem || Some(o) == flow.config.name.as_ref());
            match named {
                Some(at) => ordered.push((at, flow)),
                None => rest.push(flow),
            }
        }
    }
    ordered.sort_by_key(|(at, _)| *at);
    sel.sequential = ordered.len();
    sel.flows = ordered.into_iter().map(|(_, f)| f).chain(rest).collect();
    Ok(sel)
}

fn load(path: &Path) -> Result<Flow> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(parse_flow(&text, Some(path))?)
}

/// What `--continuous` watches: every flow, script and data file under `paths`, with its
/// modification time and size. Two snapshots differ when a file was edited, added or removed.
pub fn snapshot(paths: &[PathBuf]) -> std::collections::BTreeMap<PathBuf, (Option<std::time::SystemTime>, u64)> {
    fn walk(p: &Path, out: &mut std::collections::BTreeMap<PathBuf, (Option<std::time::SystemTime>, u64)>) {
        if p.is_dir() {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                walk(&e.path(), out);
            }
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("yaml" | "yml" | "js" | "json")) {
            if let Ok(m) = std::fs::metadata(p) {
                out.insert(p.to_path_buf(), (m.modified().ok(), m.len()));
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    for p in paths {
        walk(p, &mut out);
    }
    out
}
