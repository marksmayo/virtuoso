//! Which flows a run picks, as Maestro picks them:
//!
//! - A file given on the command line runs as is.
//! - A folder runs the flows its `config.yaml` names (`flows:` globs relative to the folder,
//!   `!` to leave out; by default `*`, the top level), never config.yaml itself.
//! - `includeTags` / `excludeTags` from the config and the command line filter folder flows.
//! - `executionOrder.flowsOrder` runs those flows first, in that order, one after another.

use crate::flow::{parse_flow, Flow};
use anyhow::{anyhow, Context, Result};
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

/// The folder's config, or the defaults when there is none: one read, not a check and a read.
fn read_config(path: &Path) -> Result<Config> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
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

/// Whether `path` (`/`-separated, relative) matches `glob`: `*` and `?` stay within a folder,
/// `**` crosses them, and `**/` also matches no folder at all.
pub fn glob_matches(glob: &str, path: &str) -> bool {
    // On the UTF-8 bytes: `*`, `?` and `/` are ASCII, which no byte of another character
    // equals. Wildcards only stop at character starts, and `?` takes a whole character.
    fn starts_char(p: &[u8], i: usize) -> bool {
        p.get(i).is_none_or(|&b| b & 0xc0 != 0x80)
    }
    fn go(g: &[u8], p: &[u8]) -> bool {
        match g.first() {
            None => p.is_empty(),
            Some(b'*') if g.get(1) == Some(&b'*') => {
                let rest = if g.get(2) == Some(&b'/') { &g[3..] } else { &g[2..] };
                (0..=p.len()).any(|i| starts_char(p, i) && go(rest, &p[i..]))
            }
            Some(b'*') => (0..=p.len()).take_while(|&i| i == 0 || p[i - 1] != b'/').any(|i| starts_char(p, i) && go(&g[1..], &p[i..])),
            Some(b'?') => p.first().is_some_and(|&c| c != b'/') && go(&g[1..], &p[(1..=p.len()).find(|&i| starts_char(p, i)).unwrap_or(1)..]),
            Some(&c) => p.first() == Some(&c) && go(&g[1..], &p[1..]),
        }
    }
    go(glob.as_bytes(), path.as_bytes())
}

/// Whether `glob` can match below the folder's top level: only `/` or `**` cross a folder.
fn reaches_subfolders(glob: &str) -> bool {
    glob.contains('/') || glob.contains("**")
}

fn is_flow_file(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()), Some("yaml" | "yml")) && p.file_stem().is_some_and(|s| s != "config")
}

/// The flow files in `dir`, and in its subfolders when `deep`.
fn walk(dir: &Path, deep: bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        // The listing gives each entry's type; only a link is looked up, to follow it.
        let is_dir = match e.file_type() {
            Ok(t) if !t.is_symlink() => t.is_dir(),
            _ => p.is_dir(),
        };
        if is_dir {
            if deep {
                walk(&p, deep, out);
            }
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
    let stats: Vec<_> = paths.iter().map(|p| fs::metadata(p).ok()).collect();
    collect_stated(paths, &stats, include_tags, exclude_tags, config)
}

/// [`collect`], with each path's metadata already taken (None: it does not exist): a caller
/// that checked the paths exist need not have them looked up again.
pub fn collect_stated(paths: &[PathBuf], stats: &[Option<fs::Metadata>], include_tags: &[String], exclude_tags: &[String], config: Option<&Path>) -> Result<Selection> {
    let mut sel = Selection { continue_on_failure: true, ..Default::default() };
    let mut ordered: Vec<(usize, Flow)> = Vec::new();
    let mut rest: Vec<Flow> = Vec::new();
    for (path, stat) in paths.iter().zip(stats) {
        match stat {
            Some(m) if m.is_file() => {
                rest.push(load(path)?);
                continue;
            }
            Some(m) if m.is_dir() => {}
            _ => return Err(anyhow!("{} does not exist", path.display())),
        }
        let cfg_path = config.map(Path::to_path_buf).unwrap_or_else(|| path.join("config.yaml"));
        let cfg = read_config(&cfg_path)?;
        if let Some(c) = cfg.continue_on_failure {
            sel.continue_on_failure = c;
        }
        if let Some(d) = &cfg.test_output_dir {
            sel.test_output_dir = Some(path.join(d));
        }
        let globs = if cfg.flows.is_empty() { vec!["*".to_string()] } else { cfg.flows.clone() };
        let (exclude, include): (Vec<&String>, Vec<&String>) = globs.iter().partition(|g| g.starts_with('!'));
        let include: Vec<&str> = include.iter().map(|g| g.as_str()).collect();
        let exclude: Vec<&str> = exclude.iter().map(|g| &g[1..]).collect();
        let mut files = Vec::new();
        // With the default `*`, or any other glob that stays in the folder, its subfolders
        // (scripts, assets, test output) are not listed at all.
        walk(path, include.iter().any(|g| reaches_subfolders(g)), &mut files);
        files.retain(|f| {
            if f == &cfg_path {
                return false;
            }
            let rel = relative(path, f);
            let no_ext = rel.rsplit_once('.').map_or(rel.as_str(), |(a, _)| a);
            let hit = |g: &&str| glob_matches(g, &rel) || glob_matches(g, no_ext);
            include.iter().any(hit) && !exclude.iter().any(hit)
        });
        crate::sort::sort_small_by_key(&mut files, |f| relative(path, f));
        let inc: Vec<&String> = include_tags.iter().chain(&cfg.include_tags).collect();
        let exc: Vec<&String> = exclude_tags.iter().chain(&cfg.exclude_tags).collect();
        let mut loaded = load_many(&files).into_iter();
        for f in files {
            // In file order, so the first bad file is the one named, as when read one by one.
            let flow = match loaded.next() {
                Some(r) => r?,
                None => load(&f)?,
            };
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
    crate::sort::sort_small_by_key(&mut ordered, |(at, _)| *at);
    sel.sequential = ordered.len();
    sel.flows = ordered.into_iter().map(|(_, f)| f).chain(rest).collect();
    Ok(sel)
}

/// Many files' flows, in order, read and parsed on a few threads (each ~0.1 ms, before the
/// first flow can start); none for a few files, which are read one by one.
fn load_many(files: &[PathBuf]) -> Vec<Result<Flow>> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4);
    if files.len() <= 16 || threads < 2 {
        return Vec::new();
    }
    std::thread::scope(|s| {
        let parts: Vec<_> = files.chunks(files.len().div_ceil(threads)).map(|part| s.spawn(move || part.iter().map(|f| load(f)).collect::<Vec<_>>())).collect();
        parts.into_iter().flat_map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
    })
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
