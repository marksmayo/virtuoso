//! JavaScript for flows, on QuickJS: `${...}` expressions, `runScript`, `evalScript`,
//! `assertTrue` and `when: true:`. One engine per flow run; `output` and variables persist in it.
//!
//! Scripts see what Maestro gives them: env variables as globals, `output`, `maestro.platform`,
//! `maestro.copiedText`, `console.log`, `json()`, `http` (sent with curl) and a small `faker`,
//! plus CommonJS `require()` of script modules (as maestro-runner).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use rquickjs::{CatchResultExt, Context, Function, Runtime};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

const PRELUDE: &str = include_str!("prelude.js");

/// The engine starts on first use: most flows have no JavaScript, and a QuickJS runtime with
/// the prelude costs ~150 KB of heap and ~1 ms.
pub struct Js {
    platform: String,
    engine: std::cell::OnceCell<Engine>,
    /// Variables and copied text set before the engine started, for it to start with.
    pending: RefCell<Vec<(String, String)>>,
    copied: RefCell<Option<String>>,
    logs: Rc<RefCell<Vec<String>>>,
    /// Where a relative `require()` from a script looks: the running script's folder, then the
    /// flow's.
    module_dirs: Rc<RefCell<Vec<PathBuf>>>,
}

struct Engine {
    _rt: Runtime,
    ctx: Context,
}

impl Js {
    pub fn new(platform: &str) -> Js {
        Js {
            platform: platform.to_string(),
            engine: std::cell::OnceCell::new(),
            pending: RefCell::new(Vec::new()),
            copied: RefCell::new(None),
            logs: Rc::new(RefCell::new(Vec::new())),
            module_dirs: Rc::new(RefCell::new(vec![PathBuf::from(".")])),
        }
    }

    /// Whether the engine has started.
    pub fn started(&self) -> bool {
        self.engine.get().is_some()
    }

    fn ctx(&self) -> &Context {
        &self
            .engine
            .get_or_init(|| {
                let e = start_engine(&self.platform, self.logs.clone(), self.module_dirs.clone());
                e.ctx.with(|ctx| {
                    for (k, v) in self.pending.borrow_mut().drain(..) {
                        ctx.globals().set(k, v).unwrap();
                    }
                    if let Some(t) = self.copied.borrow_mut().take() {
                        let m: rquickjs::Object = ctx.globals().get("maestro").unwrap();
                        m.set("copiedText", t).unwrap();
                    }
                });
                e
            })
            .ctx
    }

    /// The folders a relative `require()` looks in, first to last.
    pub fn set_module_dirs(&self, dirs: Vec<PathBuf>) {
        *self.module_dirs.borrow_mut() = dirs;
    }

    pub fn set_var(&self, name: &str, value: &str) {
        match self.engine.get() {
            Some(e) => e.ctx.with(|ctx| ctx.globals().set(name, value).unwrap()),
            None => self.pending.borrow_mut().push((name.to_string(), value.to_string())),
        }
    }

    pub fn set_copied_text(&self, text: &str) {
        match self.engine.get() {
            Some(e) => e.ctx.with(|ctx| {
                let m: rquickjs::Object = ctx.globals().get("maestro").unwrap();
                m.set("copiedText", text).unwrap();
            }),
            None => *self.copied.borrow_mut() = Some(text.to_string()),
        }
    }

    fn eval_string(&self, expr: &str) -> Result<String> {
        self.ctx().with(|ctx| {
            ctx.eval::<String, _>(format!("String(({expr}\n))"))
                .catch(&ctx)
                .map_err(|e| anyhow!("in `{expr}`: {e}"))
        })
    }

    /// Replaces each `${expression}` in `s` with its value.
    pub fn expand(&self, s: &str) -> Result<String> {
        if !s.contains("${") {
            return Ok(s.to_string());
        }
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let body = &rest[start + 2..];
            // Unclosed, it stays text, as in Maestro.
            let Some(end) = expression_end(body) else {
                out.push_str(&rest[start..]);
                return Ok(out);
            };
            out.push_str(&self.eval_string(&body[..end])?);
            rest = &body[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// A condition as `assertTrue` and `when: true:` read it: `${...}` text, or a bare expression.
    pub fn truthy(&self, cond: &str) -> Result<bool> {
        let t = cond.trim();
        if t.contains("${") {
            let v = self.expand(t)?;
            return Ok(!matches!(v.trim(), "" | "false" | "0" | "undefined" | "null" | "NaN"));
        }
        self.ctx().with(|ctx| ctx.eval::<bool, _>(format!("!!({t}\n)")).catch(&ctx).map_err(|e| anyhow!("in `{t}`: {e}")))
    }

    /// Runs a script with `env` set as globals first.
    pub fn run_script(&self, code: &str, env: &[(String, String)]) -> Result<()> {
        for (k, v) in env {
            let v = self.expand(v)?;
            self.set_var(k, &v);
        }
        self.ctx().with(|ctx| ctx.eval::<(), _>(code).catch(&ctx).map_err(|e| anyhow!("{e}")))
    }

    pub fn take_logs(&self) -> Vec<String> {
        std::mem::take(&mut self.logs.borrow_mut())
    }
}

/// A QuickJS runtime with the prelude loaded.
fn start_engine(platform: &str, sink: Rc<RefCell<Vec<String>>>, module_dirs: Rc<RefCell<Vec<PathBuf>>>) -> Engine {
    let rt = Runtime::new().expect("QuickJS runtime");
    // The built-ins flows use; Proxy, typed arrays, BigInt, WeakRef and performance are left out.
    use rquickjs::context::intrinsic::{Date, Eval, Json, MapSet, Promise, RegExp, RegExpCompiler};
    let ctx = Context::custom::<(Date, Eval, RegExpCompiler, RegExp, Json, MapSet, Promise)>(&rt).expect("QuickJS context");
    let dirs = module_dirs.clone();
    let form_dirs = module_dirs;
    ctx.with(|ctx| {
        let g = ctx.globals();
        g.set("__log", Function::new(ctx.clone(), move |m: String| sink.borrow_mut().push(m)).unwrap()).unwrap();
        let send = move |method: String, url: String, headers: String, body: Option<String>, form: Option<String>| {
            // A form's relative file paths are the flow's, like every other file a flow names.
            let base = form_dirs.borrow().last().cloned().unwrap_or_default();
            http(method, url, headers, body, form, &base)
        };
        g.set("__http", Function::new(ctx.clone(), send).unwrap()).unwrap();
        g.set("__platform", platform).unwrap();
        g.set("__module", Function::new(ctx.clone(), move |path: String, from: String| find_module(&path, &from, &dirs.borrow())).unwrap()).unwrap();
        ctx.eval::<(), _>(PRELUDE).catch(&ctx).map_err(|e| e.to_string()).expect("prelude");
    });
    Engine { _rt: rt, ctx }
}

/// Where the `}` closing an expression is: braces nest, and quotes hide them.
fn expression_end(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `require(path)`: the module file, as JSON {path, dir, source} or {error}. A path relative to
/// a module (`from` is its folder) resolves there; otherwise in each of `dirs` in turn. `.js`
/// and `/index.js` may be left off.
fn find_module(path: &str, from: &str, dirs: &[PathBuf]) -> String {
    let bases: Vec<PathBuf> = if !from.is_empty() { vec![PathBuf::from(from)] } else { dirs.to_vec() };
    let candidates = |base: &Path| {
        let p = base.join(path);
        [p.clone(), p.with_extension("js"), p.join("index.js")]
    };
    for file in bases.iter().flat_map(|b| candidates(b)) {
        if file.is_file() {
            if let Ok(source) = std::fs::read_to_string(&file) {
                let dir = file.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
                return serde_json::json!({ "path": file.to_string_lossy(), "dir": dir, "source": source }).to_string();
            }
        }
    }
    serde_json::json!({ "error": format!("require: cannot find module '{path}'") }).to_string()
}

/// `http.*` in scripts: curl, for http and https only. Returns the response as JSON text.
fn http(method: String, url: String, headers: String, body: Option<String>, form: Option<String>, base: &Path) -> String {
    let reply = |status: i64, body: &str, err: Option<&str>| {
        serde_json::json!({ "ok": (200..300).contains(&status), "status": status, "body": body, "error": err }).to_string()
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return reply(0, "", Some("only http and https URLs"));
    }
    let mut c = Command::new("curl");
    c.args(["-sS", "-L", "--proto", "=http,https", "-X", &method, "-w", "\n%{http_code}"]);
    if let Ok(serde_json::Value::Object(h)) = serde_json::from_str::<serde_json::Value>(&headers) {
        for (k, v) in h {
            c.arg("-H").arg(format!("{k}: {}", v.as_str().map(str::to_string).unwrap_or(v.to_string())));
        }
    }
    if let Some(b) = body {
        c.arg("--data-binary").arg(b);
    }
    // multipartForm: {field: "text"} or {field: {filePath, mediaType}}.
    if let Some(serde_json::Value::Object(fields)) = form.as_deref().and_then(|f| serde_json::from_str(f).ok()) {
        for (name, v) in fields {
            match v.get("filePath").and_then(|p| p.as_str()) {
                Some(path) => {
                    let mut part = format!("{name}=@{}", base.join(path).to_string_lossy());
                    if let Some(t) = v.get("mediaType").and_then(|t| t.as_str()) {
                        part.push_str(&format!(";type={t}"));
                    }
                    c.arg("-F").arg(part);
                }
                // --form-string: a value starting with @ or < is text, not a file.
                None => {
                    c.arg("--form-string").arg(format!("{name}={}", v.as_str().map(str::to_string).unwrap_or(v.to_string())));
                }
            }
        }
    }
    c.arg(&url);
    match c.output() {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
            reply(code.trim().parse().unwrap_or(0), body, (!out.status.success()).then_some("curl failed"))
        }
        Err(e) => reply(0, "", Some(&format!("curl: {e}"))),
    }
}
