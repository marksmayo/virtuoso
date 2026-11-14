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
    // macOS: the linker leaves out a dylib nothing calls into (std names libiconv), one load less
    // for dyld at every start. It keeps every dylib a symbol resolves to.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-dead_strip_dylibs");
    }
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

    let mut sources = Vec::new();
    collect(src, &mut sources);
    sources.sort();
    for s in &sources {
        println!("cargo:rerun-if-changed={}", s.display());
    }
    let rules = "-keep class dev.virtuoso.agent.Main { public static void main(java.lang.String[]); }\n-dontwarn **\n";

    // Built once per set of inputs, under target/: debug, release and test builds (each its own
    // OUT_DIR) and a rerun of this script for an iOS change copy it rather than run javac and R8
    // (two JVMs, seconds each). The key covers the sources, this script (the tool flags and the
    // zip writer are in it), the tools and the SDK files.
    let kept = keep_dir(out).map(|d| d.join(format!("{:016x}.jar", inputs_key(src, &sources, &[&android_jar, &d8], rules))));
    if let Some(jar) = kept.as_ref().and_then(|k| fs::read(k).ok()).filter(|j| !j.is_empty()) {
        return fs::write(out, jar).map_err(|e| e.to_string());
    }

    let work = out.with_extension("classes");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;

    // -g:none: R8 keeps the line tables javac writes (~900 bytes of dex), and nothing reads
    // them: errors reach the host as messages, never as stack traces.
    // Both JVMs run briefly over a dozen small classes: C2 compiling costs more than it saves
    // (javac+R8 ~11 s -> ~5 s here), and the output is the same bytes.
    let status = Command::new(tool("javac"))
        .args(["-J-XX:TieredStopAtLevel=1", "-J-Xshare:auto"])
        .args(["--release", "11", "-g:none", "-nowarn", "-encoding", "UTF-8", "-cp"])
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
    let rules_file = out.with_extension("pro");
    fs::write(&rules_file, rules).map_err(|e| e.to_string())?;
    let dex_dir = out.with_extension("dex");
    let _ = fs::remove_dir_all(&dex_dir);
    fs::create_dir_all(&dex_dir).map_err(|e| e.to_string())?;
    let status = Command::new(tool("java"))
        .args(["-XX:TieredStopAtLevel=1", "-Xshare:auto", "-XX:+UseSerialGC"])
        .arg("-cp")
        .arg(&d8)
        .arg("com.android.tools.r8.R8")
        .args(["--release", "--min-api", "26", "--pg-conf"])
        .arg(&rules_file)
        .arg("--lib")
        .arg(&android_jar)
        .arg("--output")
        .arg(&dex_dir)
        .args(&classes)
        .status()
        .map_err(|e| format!("r8: {e}"))?;
    if !status.success() {
        return Err("r8 failed".into());
    }
    let dex = fs::read(dex_dir.join("classes.dex")).map_err(|e| format!("r8 output: {e}"))?;
    let jar = jar(&dex)?;
    if let Some(kept) = kept {
        // Written aside and renamed: another build may be reading or writing the same one.
        let tmp = kept.with_extension(format!("{}.tmp", std::process::id()));
        if fs::create_dir_all(kept.parent().unwrap()).and_then(|_| fs::write(&tmp, &jar)).and_then(|_| fs::rename(&tmp, &kept)).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
    fs::write(out, jar).map_err(|e| e.to_string())
}

/// target/virtuoso-agent (target/<triple>/virtuoso-agent in a cross build): OUT_DIR is
/// target/<profile>/build/virtuoso-<hash>/out.
fn keep_dir(out_jar: &Path) -> Option<PathBuf> {
    out_jar.ancestors().nth(5).map(|target| target.join("virtuoso-agent"))
}

/// What the agent jar is made from: each source's path and bytes, this script, the javac and
/// java it finds, each SDK file's path and size, and the R8 rules.
fn inputs_key(src: &Path, sources: &[PathBuf], files: &[&Path], rules: &str) -> u64 {
    let mut bytes = Vec::new();
    for s in sources {
        bytes.extend_from_slice(s.strip_prefix(src).unwrap_or(s).to_string_lossy().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&fs::read(s).unwrap_or_default());
        bytes.push(0);
    }
    bytes.extend_from_slice(include_bytes!("build.rs"));
    for t in [tool("javac"), tool("java")] {
        bytes.extend_from_slice(t.to_string_lossy().as_bytes());
        bytes.push(0);
    }
    for f in files {
        let len = fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        bytes.extend_from_slice(format!("{}\0{len}\0", f.display()).as_bytes());
    }
    bytes.extend_from_slice(rules.as_bytes());
    fnv1a(&bytes)
}

/// classes.dex alone in the smallest zip ART reads: deflated by zopfli (~8% under R8's own
/// jar), no extra fields, data descriptor or comment. The binary embeds it byte for byte.
fn jar(dex: &[u8]) -> Result<Vec<u8>, String> {
    let mut packed = Vec::new();
    zopfli::compress(zopfli::Options::default(), zopfli::Format::Deflate, dex, &mut packed).map_err(|e| e.to_string())?;
    let name = b"classes.dex";
    let (crc, packed_len, len) = (crc32(dex), packed.len() as u32, dex.len() as u32);
    // version 2.0, no flags, deflated, 1980-01-01 00:00
    let entry = |z: &mut Vec<u8>| {
        for v in [20u16, 0, 8, 0, 0x21] {
            z.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, packed_len, len] {
            z.extend_from_slice(&v.to_le_bytes());
        }
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
    };
    let mut z = Vec::with_capacity(packed.len() + 128);
    z.extend_from_slice(&0x04034b50u32.to_le_bytes());
    entry(&mut z);
    z.extend_from_slice(name);
    z.extend_from_slice(&packed);
    let central = z.len() as u32;
    z.extend_from_slice(&0x02014b50u32.to_le_bytes());
    z.extend_from_slice(&20u16.to_le_bytes());
    entry(&mut z);
    // comment length, disk, internal and external attributes, the local header's offset
    z.extend_from_slice(&[0; 14]);
    z.extend_from_slice(name);
    let central_len = z.len() as u32 - central;
    z.extend_from_slice(&0x06054b50u32.to_le_bytes());
    for v in [0u16, 0, 1, 1] {
        z.extend_from_slice(&v.to_le_bytes());
    }
    z.extend_from_slice(&central_len.to_le_bytes());
    z.extend_from_slice(&central.to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    Ok(z)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= *b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
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
    let mut files = Vec::new();
    ios_files(&root, &root, &mut files);
    files.sort();
    // Each file, and the folders they are in (a file added there is noticed): not agent/ios
    // whole, which Cargo would scan through the fixture app and its DerivedData under build/,
    // so that every fixture build would run this script again and rebuild virtuoso.
    let mut dirs: Vec<&str> = files.iter().filter_map(|rel| rel.rsplit_once('/').map(|(dir, _)| dir)).collect();
    dirs.dedup();
    // What is watched also goes to OUT_DIR/ios_watched.txt, for a test to check.
    let mut watched = String::new();
    for dir in dirs {
        watched.push_str(&format!("{}\n", root.join(dir).display()));
    }
    let mut code = String::from("pub static IOS_AGENT: &[(&str, &[u8])] = &[
");
    for rel in &files {
        let abs = root.join(rel).canonicalize().unwrap();
        watched.push_str(&format!("{}\n", abs.display()));
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
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out.join("ios_agent.rs"), code).unwrap();
    for path in watched.lines() {
        println!("cargo:rerun-if-changed={path}");
    }
    fs::write(out.join("ios_watched.txt"), watched).unwrap();
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
