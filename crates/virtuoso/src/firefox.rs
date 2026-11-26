//! Firefox over WebDriver BiDi: finding and launching the browser (headless, a throwaway
//! profile) and its session. BiDi's messages have CDP's shape (an id, a method and params; a
//! reply's result; events with no id), so the protocol client is Chrome's.

use crate::chrome::Cdp;
use crate::websocket::WebSocket;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Where Firefox is usually installed, in the order to try: `env` looks a variable up.
pub fn firefox_candidates(os: &str, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    match os {
        "windows" => ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|k| env(k))
            .map(|base| PathBuf::from(format!(r"{}\Mozilla Firefox\firefox.exe", base.trim_end_matches(['\\', '/']))))
            .collect(),
        "macos" => ["/Applications/Firefox.app/Contents/MacOS/firefox"].iter().map(PathBuf::from).collect(),
        _ => ["/usr/bin/firefox", "/usr/local/bin/firefox", "/snap/bin/firefox", "/usr/lib/firefox/firefox"].iter().map(PathBuf::from).collect(),
    }
}

/// The first candidate that exists, or `VIRTUOSO_FIREFOX`.
pub fn find_firefox() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("VIRTUOSO_FIREFOX") {
        return Ok(PathBuf::from(p));
    }
    let env = |k: &str| std::env::var(k).ok();
    firefox_candidates(std::env::consts::OS, &env)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("Firefox is not installed where virtuoso looks for it: set VIRTUOSO_FIREFOX to its path"))
}

/// The preferences a throwaway profile starts with: none of a first run's prompts, reports,
/// updates, welcome or start pages, session restores or default-browser checks (on a Windows
/// runner a tab Firefox opened or restored at its start took the place of the flow's). (The remote agent sets its own recommended ones for automation on top.)
const PREFS: &str = r#"user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.aboutwelcome.enabled", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("app.update.disabledForTesting", true);
user_pref("browser.startup.page", 0);
user_pref("browser.startup.homepage", "about:blank");
user_pref("startup.homepage_welcome_url", "");
user_pref("startup.homepage_welcome_url.additional", "");
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.newtabpage.enabled", false);
user_pref("browser.sessionstore.resume_from_crash", false);
user_pref("browser.sessionstore.max_resumed_crashes", 0);
user_pref("toolkit.startup.max_resumed_crashes", -1);
user_pref("browser.tabs.closeWindowWithLastTab", false);
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.warnOnQuit", false);
user_pref("browser.uitour.enabled", false);
user_pref("browser.defaultbrowser.notificationbar", false);
user_pref("default-browser-agent.enabled", false);
user_pref("browser.disableResetPrompt", true);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
"#;

/// How long a launch waits for Firefox to listen.
const LISTEN_TIMEOUT: Duration = Duration::from_secs(30);

/// A Firefox being driven, with its BiDi session: stopped, and its profile removed, when dropped.
pub struct Firefox {
    pub bidi: Cdp,
    /// What the session was given (`session.new`'s capabilities).
    pub capabilities: Value,
    // Last: the session goes before the browser does.
    _process: FirefoxProcess,
}

/// The browser's processes and profile: stopped and removed when dropped.
pub struct FirefoxProcess {
    child: Option<Child>,
    /// Firefox and every process it starts (Windows): ended together at the stop.
    #[cfg(windows)]
    tree: Option<crate::chrome::tree::Tree>,
    profile: Option<PathBuf>,
}

impl Firefox {
    /// Firefox at `path`, headless or not, on a profile of its own and a free port, with a
    /// BiDi session open.
    pub fn launch(path: &Path, headless: bool) -> Result<Firefox> {
        let (process, url) = start(path, headless)?;
        let mut bidi = Cdp::new(WebSocket::connect(&format!("{url}/session"))?);
        let session = bidi.call(None, "session.new", json!({"capabilities": {}}))?;
        Ok(Firefox { bidi, capabilities: session["capabilities"].clone(), _process: process })
    }
}

/// Firefox at `path`, headless or not, on a profile of its own and a free port: its process
/// (stopped, and its profile removed, when dropped) and the WebSocket it listens on.
pub fn start(path: &Path, headless: bool) -> Result<(FirefoxProcess, String)> {
    {
        static N: AtomicU32 = AtomicU32::new(0);
        let profile = std::env::temp_dir().join(format!("virtuoso-firefox-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&profile);
        std::fs::create_dir_all(&profile).with_context(|| format!("making a profile folder {}", profile.display()))?;
        let diag = std::env::var("VIRTUOSO_FIREFOX_LOG").ok().map(PathBuf::from);
        let mut prefs = PREFS.to_string();
        if diag.is_some() {
            prefs.push_str("user_pref(\"remote.log.level\", \"Trace\");\nuser_pref(\"remote.log.truncate\", true);\n");
        }
        std::fs::write(profile.join("user.js"), prefs)?;
        let log = diag.and_then(|d| {
            let _ = std::fs::create_dir_all(&d);
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis()).unwrap_or(0);
            std::fs::File::create(d.join(format!("{stamp}-{}.log", profile.file_name().unwrap_or_default().to_string_lossy()))).ok()
        }).map(|f| std::sync::Arc::new(std::sync::Mutex::new(f)));
        let mut args: Vec<String> = Vec::new();
        if headless {
            args.push("--headless".into());
        }
        args.extend(["--no-remote".into(), "--profile".into(), profile.display().to_string(), "--remote-debugging-port=0".into(), "about:blank".into()]);
        let mut child = match Command::new(path)
            .args(&args)
            // A desktop's window, the same as Chrome's (headless Firefox's own is 1366 x 768;
            // headless, it takes its size from these, not from --window-size).
            .env("MOZ_HEADLESS_WIDTH", "1280")
            .env("MOZ_HEADLESS_HEIGHT", "800").stdin(Stdio::null()).stdout(if log.is_some() { Stdio::piped() } else { Stdio::null() }).stderr(Stdio::piped()).spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&profile);
                bail!("could not start Firefox at {}: {e}", path.display());
            }
        };
        #[cfg(windows)]
        let tree = crate::chrome::tree::Tree::of(&child);
        // Firefox says where it listens on its error output; read on a thread of its own, which
        // keeps the pipe drained for as long as Firefox writes to it.
        let (tx, rx) = std::sync::mpsc::channel();
        let stderr = child.stderr.take().expect("piped");
        if let (Some(out), Some(f)) = (child.stdout.take(), log.clone()) {
            std::thread::spawn(move || {
                use std::io::{BufRead, Write};
                for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = writeln!(f.lock().unwrap(), "{line}");
                }
            });
        }
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut tx = Some(tx);
            for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(f) = &log {
                    use std::io::Write;
                    let _ = writeln!(f.lock().unwrap(), "{line}");
                }
                if let Some(url) = listening_url(&line) {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(url);
                    }
                }
            }
        });
        // From here a failure drops the process, which stops it and removes the profile.
        let process = FirefoxProcess {
            child: Some(child),
            #[cfg(windows)]
            tree,
            profile: Some(profile),
        };
        let url = rx.recv_timeout(LISTEN_TIMEOUT).map_err(|_| anyhow!("Firefox did not open its WebDriver BiDi port within {LISTEN_TIMEOUT:?}"))?;
        Ok((process, url))
    }
}

/// The WebSocket Firefox says it listens on (`WebDriver BiDi listening on ws://…`).
pub fn listening_url(line: &str) -> Option<String> {
    let at = line.find("WebDriver BiDi listening on ")?;
    let url = line[at + "WebDriver BiDi listening on ".len()..].trim();
    url.starts_with("ws://").then(|| url.trim_end_matches('/').to_string())
}

impl FirefoxProcess {
    /// Its throwaway profile.
    pub fn profile(&self) -> Option<&Path> {
        self.profile.as_deref()
    }
}

impl Drop for FirefoxProcess {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            // The profile is thrown away, nothing in it to save: every process ended at once.
            #[cfg(windows)]
            if let Some(t) = self.tree.take() {
                t.end();
            }
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(p) = self.profile.take() {
            // Its last processes hold files a moment after they are ended.
            let end = Instant::now() + Duration::from_secs(5);
            while std::fs::remove_dir_all(&p).is_err() && p.exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_firefox_listens_is_read_from_its_announcement() {
        assert_eq!(listening_url("WebDriver BiDi listening on ws://127.0.0.1:51234"), Some("ws://127.0.0.1:51234".to_string()));
        assert_eq!(listening_url("1764000000000\tRemote Agent\tINFO\tWebDriver BiDi listening on ws://127.0.0.1:9222/"), Some("ws://127.0.0.1:9222".to_string()));
        assert_eq!(listening_url("Read port: 0"), None);
    }

    #[test]
    fn firefox_is_looked_for_where_each_os_installs_it() {
        let env = |k: &str| (k == "ProgramFiles").then(|| r"C:\Program Files".to_string());
        assert_eq!(firefox_candidates("windows", &env), vec![PathBuf::from(r"C:\Program Files\Mozilla Firefox\firefox.exe")]);
        assert_eq!(firefox_candidates("macos", &env), vec![PathBuf::from("/Applications/Firefox.app/Contents/MacOS/firefox")]);
        assert!(firefox_candidates("linux", &env).contains(&PathBuf::from("/usr/bin/firefox")));
    }
}
