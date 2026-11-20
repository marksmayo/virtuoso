//! The Android agent's JSON string writer, run on the host's JVM, against the escaping the
//! host's parser expects. Needs a JDK (`javac` and `java` on PATH or in JAVA_HOME), as building
//! the agent does; without one this passes without running.

use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = Path::new(&home).join("bin").join(&exe);
        if p.exists() {
            return Some(p);
        }
    }
    Command::new(name).arg("-version").output().ok().map(|_| PathBuf::from(name))
}

/// Each string's JSON literal, as the agent writes it.
fn agent(strings: &[Vec<u16>]) -> Option<Vec<Vec<u16>>> {
    let (javac, java) = (tool("javac")?, tool("java")?);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("../../agent/android/src/dev/virtuoso/agent");
    let out = std::env::temp_dir().join(format!("virtuoso-agent-java-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let status = Command::new(javac).arg("-d").arg(&out).arg(src.join("Json.java")).arg(root.join("tests/fixtures/java/AgentCheck.java")).status().unwrap();
    assert!(status.success(), "javac failed");
    let hex = |s: &[u16]| s.iter().map(|u| format!("{u:04x}")).collect::<String>();
    let run = Command::new(java).arg("-cp").arg(&out).arg("dev.virtuoso.agent.AgentCheck").args(strings.iter().map(|s| hex(s))).output().unwrap();
    let _ = std::fs::remove_dir_all(&out);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let lines = String::from_utf8(run.stdout).unwrap();
    Some(lines.lines().map(|json| (0..json.len()).step_by(4).map(|i| u16::from_str_radix(&json[i..i + 4], 16).unwrap()).collect()).collect())
}

/// The JSON string literal of `s` as the agent writes it: `"`, `\` and \n \r \t escaped, other
/// control characters as \u00XX, everything else (lone surrogates too) as it is.
fn literal(s: &[u16]) -> Vec<u16> {
    let mut out = vec![b'"' as u16];
    for &u in s {
        match u {
            0x22 => out.extend("\\\"".encode_utf16()),
            0x5c => out.extend("\\\\".encode_utf16()),
            0x0a => out.extend("\\n".encode_utf16()),
            0x0d => out.extend("\\r".encode_utf16()),
            0x09 => out.extend("\\t".encode_utf16()),
            u if u < 0x20 => out.extend(format!("\\u{u:04x}").encode_utf16()),
            u => out.push(u),
        }
    }
    out.push(b'"' as u16);
    out
}

#[test]
fn the_agents_json_strings_escape_what_json_needs_and_keep_the_rest() {
    let mut strings: Vec<Vec<u16>> = [
        "",
        "plain text, and a lot of it with nothing to escape at all",
        "Say \"hi\" \\ back",
        "tab\there\nnew line\r\u{1}\u{1f}\u{7f}",
        "é ü 中文 € 😀 ok",
        "\"\"",
    ]
    .iter()
    .map(|s| s.encode_utf16().collect())
    .collect();
    strings.push(vec![0x61, 0xd800, 0x62]); // a lone high surrogate
    strings.push(vec![0xdc00]); // a lone low surrogate
    let Some(got) = agent(&strings) else { return };
    assert_eq!(got.len(), strings.len());
    for (s, json) in strings.iter().zip(got) {
        assert_eq!(json, literal(s), "the literal of {:?}", String::from_utf16_lossy(s));
    }
}
