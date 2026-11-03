//! `--env-file`: KEY=VALUE lines as dotenv files are written. `#` starts a comment outside
//! quotes, `export ` in front of a name is allowed, and quotes around a value are removed.

pub fn parse_env_file(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected KEY=VALUE, found '{raw}'", n + 1))?;
        let v = v.trim();
        let value = match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_string(),
            _ => v.split(" #").next().unwrap_or("").trim().to_string(),
        };
        out.push((k.trim().to_string(), value));
    }
    Ok(out)
}
