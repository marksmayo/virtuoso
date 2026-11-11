//! Builds the Android agent (agent/android) into a dex jar that the binary embeds.
//!
//! Needs a JDK (`javac`/`java` on PATH or in JAVA_HOME) and an Android SDK (ANDROID_HOME or
//! ANDROID_SDK_ROOT) with a platform and build-tools. VIRTUOSO_AGENT_JAR=<path> uses a jar built
//! elsewhere instead (cross builds). Without either, the binary embeds an empty agent and
//! refuses Android devices at run time.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("virtuoso-agent.jar");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent/android/src");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-env-changed=VIRTUOSO_AGENT_JAR");

    if let Ok(prebuilt) = env::var("VIRTUOSO_AGENT_JAR") {
        fs::copy(&prebuilt, &out).unwrap_or_else(|e| panic!("VIRTUOSO_AGENT_JAR {prebuilt}: {e}"));
    } else if let Err(e) = build(&src, &out) {
        println!("cargo:warning=Android agent not built ({e}); Android devices will be refused");
        fs::write(&out, b"").unwrap();
    }
    embed_ios_agent();
    // `--version` names the agent: two builds with different agents push over each other's.
    let jar = fs::read(&out).unwrap();
    println!("cargo:rustc-env=VIRTUOSO_AGENT_HASH={:016x}", fnv1a(&jar));
}

/// FNV-1a, as the agent's file name on the device uses (`android::fnv1a`).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn build(src: &Path, out: &Path) -> Result<(), String> {
    let sdk = env::var("ANDROID_HOME")
        .or_else(|_| env::var("ANDROID_SDK_ROOT"))
        .map_err(|_| "ANDROID_HOME is not set".to_string())?;
    let sdk = PathBuf::from(sdk);
    let android_jar = newest(&sdk.join("platforms"), |p| p.join("android.jar").exists())?.join("android.jar");
    let d8 = newest(&sdk.join("build-tools"), |p| p.join("lib/d8.jar").exists())?.join("lib/d8.jar");

    let work = out.with_extension("classes");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut sources = Vec::new();
    collect(src, &mut sources);
    for s in &sources {
        println!("cargo:rerun-if-changed={}", s.display());
    }

    let status = Command::new(tool("javac"))
        .args(["--release", "11", "-nowarn", "-encoding", "UTF-8", "-cp"])
        .arg(&android_jar)
        .arg("-d")
        .arg(&work)
        .args(&sources)
        .status()
        .map_err(|e| format!("javac: {e}"))?;
    if !status.success() {
        return Err("javac failed".into());
    }
    let mut classes = Vec::new();
    collect(&work, &mut classes);
    // R8 (shipped in d8.jar) removes unused code and shortens names; only the entry point
    // app_process starts is kept by name. The agent reflects on framework classes only.
    let rules = out.with_extension("pro");
    fs::write(&rules, "-keep class dev.virtuoso.agent.Main { public static void main(java.lang.String[]); }\n-dontwarn **\n")
        .map_err(|e| e.to_string())?;
    let status = Command::new(tool("java"))
        .arg("-cp")
        .arg(&d8)
        .arg("com.android.tools.r8.R8")
        .args(["--release", "--min-api", "26", "--pg-conf"])
        .arg(&rules)
        .arg("--lib")
        .arg(&android_jar)
        .arg("--output")
        .arg(out)
        .args(&classes)
        .status()
        .map_err(|e| format!("r8: {e}"))?;
    if !status.success() {
        return Err("r8 failed".into());
    }
    Ok(())
}

/// The highest-versioned child directory (android-36 over android-35, 37.0.0 over 36.1.0) that
/// `ok` accepts. Previews (names with letters after the number) lose to releases.
fn newest(dir: &Path, ok: impl Fn(&Path) -> bool) -> Result<PathBuf, String> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let tail = name.trim_start_matches("android-");
        if tail.chars().any(|c| c.is_ascii_alphabetic()) || !ok(&p) {
            continue;
        }
        let key: Vec<u32> = tail.split('.').filter_map(|s| s.parse().ok()).collect();
        if best.as_ref().is_none_or(|(k, _)| key > *k) {
            best = Some((key, p));
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| format!("nothing usable in {}", dir.display()))
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if matches!(p.extension().and_then(|x| x.to_str()), Some("java" | "class")) {
                out.push(p);
            }
        }
    }
}

fn tool(name: &str) -> PathBuf {
    if let Ok(home) = env::var("JAVA_HOME") {
        let p = Path::new(&home).join("bin").join(name);
        if p.exists() || p.with_extension("exe").exists() {
            return p;
        }
    }
    PathBuf::from(name)
}

/// The iOS agent's sources (agent/ios, less the tests' fixture app and any build output), as
/// `IOS_AGENT` in OUT_DIR/ios_agent.rs: a Mac builds the agent from them on first use.
fn embed_ios_agent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent/ios");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    ios_files(&root, &root, &mut files);
    files.sort();
    let mut code = String::from("pub static IOS_AGENT: &[(&str, &[u8])] = &[
");
    for rel in &files {
        let abs = root.join(rel).canonicalize().unwrap();
        println!("cargo:rerun-if-changed={}", abs.display());
        code.push_str(&format!("    ({rel:?}, include_bytes!({:?})),
", abs.display().to_string()));
    }
    code.push_str("];
");
    // Their hash, as ios.rs names the agent's build by it: worked out here, once.
    let mut hash: u64 = 0xcbf29ce484222325;
    for rel in &files {
        let bytes = fs::read(root.join(rel)).unwrap();
        for b in rel.as_bytes().iter().chain(bytes.iter()) {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    code.push_str(&format!("pub static IOS_AGENT_HASH: u64 = {hash};
"));
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("ios_agent.rs");
    fs::write(out, code).unwrap();
}

fn ios_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || rel == "Fixture" || rel == "build" || name == "xcuserdata" {
            continue;
        }
        if p.is_dir() {
            ios_files(root, &p, out);
        } else {
            out.push(rel);
        }
    }
}
