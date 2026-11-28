//! Tests against a real Chrome: run with VIRTUOSO_WEB=1 (and VIRTUOSO_CHROME=<path> where
//! Chrome is not installed in the usual place). Without it they pass without running. With
//! VIRTUOSO_WEB_BROWSER=firefox the same tests drive Firefox (Chrome's own tests are skipped).

use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use virtuoso::chrome::{browser_for, find_chrome, is_headless_shell, Browser};
use virtuoso::device::{Device, LaunchOptions};
use virtuoso::web::WebDevice;

/// Whether this run's web tests drive Firefox (VIRTUOSO_WEB_BROWSER=firefox) rather than Chrome.
fn on_firefox() -> bool {
    std::env::var("VIRTUOSO_WEB_BROWSER").is_ok_and(|b| b.eq_ignore_ascii_case("firefox"))
}

/// Chrome, for the tests of Chrome itself (its switches, its processes): none on a Firefox run.
fn chrome() -> Option<std::path::PathBuf> {
    if std::env::var("VIRTUOSO_WEB").is_err() || on_firefox() {
        return None;
    }
    Some(find_chrome().expect("Chrome, for VIRTUOSO_WEB"))
}

/// A server on a free port that answers every request with `page`: its base URL.
fn serve(page: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        // A thread per connection: Chrome opens one ahead of time and may send nothing on it.
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let _ = request_path(&mut s);
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// Reads a request's head: its path and query.
fn request_path(s: &mut std::net::TcpStream) -> Option<String> {
    let mut r = BufReader::new(s.try_clone().ok()?);
    let mut first = String::new();
    r.read_line(&mut first).ok()?;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
    }
    Some(first.split_whitespace().nth(1)?.to_string())
}

/// A server for the fixture site (tests/fixtures/web): its base URL.
fn serve_fixtures() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("web");
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let root = root.clone();
            std::thread::spawn(move || {
                let full = request_path(&mut s).unwrap_or_default();
                // slow?ms=N answers "ok" after N ms (the path keeps its query here).
                if let Some(ms) = full.split("slow?ms=").nth(1).and_then(|n| n.parse::<u64>().ok()) {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    let _ = write!(s, "HTTP/1.1 200 OK
Content-Type: text/plain
Content-Length: 2
Connection: close

ok");
                    return;
                }
                // drip?ms=N answers at once, and sends its body ("ok") N ms later.
                if let Some(ms) = full.split("drip?ms=").nth(1).and_then(|n| n.parse::<u64>().ok()) {
                    let _ = write!(s, "HTTP/1.1 200 OK
Content-Type: text/plain
Content-Length: 2
Connection: close

");
                    let _ = s.flush();
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    let _ = write!(s, "ok");
                    return;
                }
                let path = full.split(['?', '#']).next().unwrap_or_default().to_string();
                let path = path.trim_start_matches('/');
                let path = if path.is_empty() { "index.html" } else { path };
                match std::fs::read(root.join(path)).ok().filter(|_| !path.contains("..")) {
                    Some(body) => {
                        let kind = if path.ends_with(".html") { "text/html; charset=utf-8" } else { "application/octet-stream" };
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(&body);
                    }
                    None => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// The browser this run drives, launched headless.
fn device() -> Option<WebDevice> {
    if std::env::var("VIRTUOSO_WEB").is_ok() && on_firefox() {
        let path = virtuoso::firefox::find_firefox().expect("Firefox, for VIRTUOSO_WEB_BROWSER=firefox");
        return Some(WebDevice::launch_firefox(&path, true).unwrap());
    }
    let path = chrome()?;
    Some(WebDevice::launch(&path, true).unwrap())
}

fn launch(d: &mut WebDevice, url: &str, clear_state: bool) {
    d.launch_app(url, &LaunchOptions { clear_state, stop_app: true, ..Default::default() }).unwrap();
}

#[test]
fn a_launched_chrome_is_headless_answers_and_opens_a_page() {
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let product = b.product().unwrap();
    assert!(product.starts_with("Chrome/") || product.starts_with("HeadlessChrome/"), "{product}");
    let ua = b.cdp.call(None, "Browser.getVersion", json!({})).unwrap()["userAgent"].as_str().unwrap_or_default().to_string();
    assert!(ua.contains("HeadlessChrome/"), "headless: {ua}");
    let t = b.cdp.call(None, "Target.createTarget", json!({"url": "about:blank"})).unwrap();
    assert!(t["targetId"].as_str().is_some_and(|s| !s.is_empty()), "{t}");
}

/// The headless shell a headless launch of `chrome` runs instead (VIRTUOSO_CHROME_SHELL, or one
/// of its version installed beside it), if there is one.
fn shell_of(chrome: &std::path::Path) -> Option<std::path::PathBuf> {
    let shell = browser_for(chrome, true, std::env::var("VIRTUOSO_CHROME_SHELL").ok().as_deref());
    is_headless_shell(&shell).then_some(shell)
}

#[test]
fn a_headless_launch_drives_chromes_headless_shell_when_there_is_one() {
    // The old headless mode, a binary of its own: it starts sooner, in less memory.
    let Some(path) = chrome() else { return };
    let Some(shell) = shell_of(&path) else {
        // CI's shell leg sets VIRTUOSO_WEB_SHELL: there, none found is a failure, not a skip.
        assert!(std::env::var("VIRTUOSO_WEB_SHELL").unwrap_or_default().is_empty(), "no headless shell found for {}", path.display());
        return;
    };
    let mut b = Browser::launch(&path, true).unwrap();
    assert_eq!(b.binary(), Some(shell.as_path()));
    assert!(b.product().unwrap().starts_with("HeadlessChrome/"), "the shell's product");
    let name = b.profile().unwrap().file_name().unwrap().to_string_lossy().to_string();
    let images = images_of(&name);
    assert!(!images.is_empty() && images.iter().all(|i| i.contains("chrome-headless-shell")), "{images:?}");
}

/// The executables of the processes whose command line names `profile` (its folder's name).
fn images_of(profile: &str) -> Vec<String> {
    let out = if cfg!(windows) {
        let find = format!("Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{profile}*' -and $_.Name -ne 'powershell.exe' }} | ForEach-Object Name");
        std::process::Command::new("powershell").args(["-NoProfile", "-Command", &find]).output().unwrap()
    } else {
        // The command line up to its switches: the executable's path (spaces and all, on macOS).
        let list = format!("for p in $(pgrep -f '{profile}'); do [ $p != $$ ] && ps -p $p -o command= | sed 's/ --.*//'; done");
        std::process::Command::new("sh").args(["-c", &list]).output().unwrap()
    };
    String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// What a launched Chrome runs besides its pages, a moment after it starts: (type, url).
fn background_targets(b: &mut Browser) -> Vec<(String, String)> {
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let t = b.cdp.call(None, "Target.getTargets", json!({})).unwrap();
    t["targetInfos"].as_array().unwrap().iter().map(|i| (i["type"].as_str().unwrap_or_default().to_string(), i["url"].as_str().unwrap_or_default().to_string())).collect()
}

#[test]
fn a_launched_chrome_starts_no_extensions_of_its_own() {
    // Chrome's component extensions start with it, each in a renderer of its own, competing
    // with the flow's first page for the CPU (~55 ms a run here, more on a 4-core runner).
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let ext: Vec<_> = background_targets(&mut b).into_iter().filter(|(_, u)| u.starts_with("chrome-extension://")).collect();
    assert!(ext.is_empty(), "{ext:?}");
}

#[test]
fn a_launched_chrome_preloads_none_of_its_own_ui() {
    // Headless, it still preloads the address bar's popup in a renderer (~60 MB) never shown.
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let ui: Vec<_> = background_targets(&mut b).into_iter().filter(|(t, u)| t == "browser_ui" || u.starts_with("chrome://")).collect();
    assert!(ui.is_empty(), "{ui:?}");
}

/// The types of a launched Chrome's processes (browser, renderer, GPU, network service...).
fn process_types(b: &mut Browser) -> Vec<String> {
    let p = b.cdp.call(None, "SystemInfo.getProcessInfo", json!({})).unwrap();
    p["processInfo"].as_array().unwrap().iter().map(|i| i["type"].as_str().unwrap_or_default().to_string()).collect()
}

#[test]
fn a_launched_chrome_runs_its_network_in_the_browser_process_on_linux_only() {
    // A process less to start and to switch to for every request the page makes. On macOS the
    // browser process is the largest, and the network made it larger still: its own process.
    // On Windows a busy browser process held a page's loading up for seconds: its own too.
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    background_targets(&mut b);
    let types = process_types(&mut b);
    assert_eq!(types.iter().any(|t| t.contains("NetworkService")), !cfg!(target_os = "linux"), "{types:?}");
}

#[test]
#[cfg(target_os = "linux")]
fn on_linux_a_launched_chrome_starts_no_renderer_its_pages_do_not_use() {
    // Chrome starts a spare renderer ahead of a navigation that may never come: a process
    // started, and CPU spent, beside the flow's page.
    let Some(path) = chrome() else { return };
    let url = serve_fixtures();
    let mut b = Browser::launch(&path, true).unwrap();
    b.cdp.call(None, "Target.createTarget", json!({"url": format!("{url}/form.html")})).unwrap();
    let pages = background_targets(&mut b).into_iter().filter(|(t, _)| t == "page").count();
    let types = process_types(&mut b);
    let renderers = types.iter().filter(|t| *t == "renderer").count();
    assert!(renderers <= pages, "{renderers} renderers for {pages} pages: {types:?}");
}

/// The features a launched Chrome was started with turned off: its browser process's last
/// --disable-features (the one Chrome takes).
fn features_off(b: &mut Browser) -> Vec<String> {
    let p = b.cdp.call(None, "SystemInfo.getProcessInfo", json!({})).unwrap();
    let pid = p["processInfo"].as_array().unwrap().iter().find(|i| i["type"] == "browser").and_then(|i| i["id"].as_u64()).expect("the browser process");
    let line = if cfg!(windows) {
        let get = format!("(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").CommandLine");
        String::from_utf8_lossy(&std::process::Command::new("powershell").args(["-NoProfile", "-Command", &get]).output().unwrap().stdout).to_string()
    } else {
        String::from_utf8_lossy(&std::process::Command::new("ps").args(["-ww", "-o", "args=", "-p", &pid.to_string()]).output().unwrap().stdout).to_string()
    };
    let last = line.split_whitespace().filter_map(|a| a.trim_matches('"').strip_prefix("--disable-features=")).next_back().unwrap_or_else(|| panic!("no --disable-features in {line}"));
    last.split(',').map(String::from).collect()
}

#[test]
fn a_launched_chrome_holds_no_first_paint_after_a_navigation() {
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let off = features_off(&mut b);
    assert!(off.iter().any(|f| f == "PaintHolding"), "{off:?}");
}

#[test]
fn a_launched_chrome_on_windows_never_judges_its_window_covered() {
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let off = features_off(&mut b);
    assert_eq!(off.iter().any(|f| f == "CalculateNativeWinOcclusion"), cfg!(windows), "{off:?}");
}

#[test]
fn a_launched_chrome_keeps_no_gpu_caches_on_disk() {
    // Drawing in software, a launched Chrome still made eight GPU and shader caches in its
    // profile: files written as the first page loads, and removed again at the end.
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let url = serve_fixtures();
    b.cdp.call(None, "Target.createTarget", json!({"url": format!("{url}/form.html")})).unwrap();
    background_targets(&mut b);
    let profile = b.profile().unwrap().to_path_buf();
    let mut dirs = vec![profile.clone(), profile.join("Default")];
    let mut gpu = Vec::new();
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if ["Shader", "GPU", "Dawn"].iter().any(|k| name.contains(k)) {
                gpu.push(name);
            }
        }
    }
    assert!(gpu.is_empty(), "{gpu:?}");
}

/// A process, by its id and when it started: Windows hands a stopped one's id out again at once
/// (soon another test's Chrome's), but not with the same start.
type Proc = (u64, String);

/// A browser's running processes: those whose command line matches `pattern` (a regex naming
/// its profile: the browser's, and Chrome's own children, which are told the profile too), and
/// every process they started (the headless shell's children are not told it).
fn chromes_of(pattern: &str) -> Vec<Proc> {
    let lines = if cfg!(windows) {
        // Still running: a process that has exited stays listed while anything (a virus scan)
        // still holds it open, and it is not one left running.
        let find = format!(
            "$all = @(Get-CimInstance Win32_Process -Filter \"Name='chrome.exe' OR Name='chrome-headless-shell.exe'\"); \
             $ids = @($all | Where-Object {{ $_.CommandLine -match '{pattern}' }} | ForEach-Object ProcessId); \
             do {{ $n = $ids.Count; $ids = @($ids) + @($all | Where-Object {{ $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId }} | ForEach-Object ProcessId) }} while ($ids.Count -gt $n); \
             $all | Where-Object {{ $ids -contains $_.ProcessId }} | Where-Object {{ $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; $p -and -not $p.HasExited }} | ForEach-Object {{ \"$($_.ProcessId) $($_.CreationDate.ToString('o'))\" }}"
        );
        let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &find]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    } else {
        let pids = |args: &[&str]| -> Vec<u64> {
            let out = std::process::Command::new("pgrep").args(args).output().unwrap();
            String::from_utf8_lossy(&out.stdout).split_whitespace().filter_map(|p| p.parse().ok()).collect()
        };
        let mut ids = pids(&["-f", pattern]);
        let mut i = 0;
        while i < ids.len() {
            for c in pids(&["-P", &ids[i].to_string()]) {
                if !ids.contains(&c) {
                    ids.push(c);
                }
            }
            i += 1;
        }
        ids.iter().filter_map(|p| started(*p).map(|s| format!("{p} {s}"))).collect::<Vec<_>>().join("\n")
    };
    lines.lines().filter_map(|l| l.trim().split_once(' ').and_then(|(p, s)| Some((p.parse().ok()?, s.to_string())))).collect()
}

/// When process `pid` started, if it is running (not Windows).
fn started(pid: u64) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-o", "lstart=", "-p", &pid.to_string()]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// Those of `procs` still running.
fn running(procs: &[Proc]) -> Vec<Proc> {
    if cfg!(windows) {
        let ids = procs.iter().map(|p| p.0.to_string()).collect::<Vec<_>>().join(",");
        let find = format!("@({ids}) | ForEach-Object {{ Get-CimInstance Win32_Process -Filter \"ProcessId=$_\" }} | Where-Object {{ $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; $p -and -not $p.HasExited }} | ForEach-Object {{ \"$($_.ProcessId) $($_.CreationDate.ToString('o'))\" }}");
        let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &find]).output().unwrap();
        let now: Vec<Proc> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().split_once(' ').and_then(|(p, s)| Some((p.parse().ok()?, s.to_string())))).collect();
        procs.iter().filter(|p| now.contains(p)).cloned().collect()
    } else {
        procs.iter().filter(|(pid, s)| started(*pid).as_ref() == Some(s)).cloned().collect()
    }
}

/// Those of `procs` still running after up to `limit`: ended at once, a loaded machine still
/// took over 5 s to tear them down (the web suite at 3-4 times its usual length), where ones
/// left running never go.
fn chromes_left_after(procs: &[Proc], limit: std::time::Duration) -> Vec<Proc> {
    let end = std::time::Instant::now() + limit;
    loop {
        let left = running(procs);
        if left.is_empty() || std::time::Instant::now() >= end {
            return left;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
#[cfg(windows)]
fn stopping_chrome_does_not_wait_for_its_processes_to_be_gone() {
    // Ended, they cannot come back (and end with virtuoso anyway): Windows tears them down
    // without the stop waiting to see it (30 to 50 ms, for a browser with a page open).
    let Some(path) = chrome() else { return };
    let url = serve_fixtures();
    let quickest = (0..3)
        .map(|_| {
            let mut b = Browser::launch(&path, true).unwrap();
            b.cdp.call(None, "Target.createTarget", json!({"url": format!("{url}/form.html")})).unwrap();
            background_targets(&mut b);
            let began = std::time::Instant::now();
            drop(b);
            began.elapsed()
        })
        .min()
        .unwrap();
    assert!(quickest < std::time::Duration::from_millis(10), "the quickest stop of three took {quickest:?}");
}

#[test]
#[cfg(target_os = "linux")]
fn on_linux_stopping_chrome_kills_its_whole_tree_with_no_graceful_close() {
    // The profile is thrown away: nothing in it to save, so no Browser.close to wait for while
    // Chrome writes its session and preferences and takes each process down. Every process of
    // its is killed at once (the benchmark counts them all either way: those Chrome no longer
    // waits for are reaped and counted by its subreaper), and none is left running.
    let Some(path) = chrome() else { return };
    let url = serve_fixtures();
    let mut quickest = std::time::Duration::MAX;
    for _ in 0..3 {
        let mut b = Browser::launch(&path, true).unwrap();
        b.cdp.call(None, "Target.createTarget", json!({"url": format!("{url}/form.html")})).unwrap();
        background_targets(&mut b);
        let profile = b.profile().unwrap().to_path_buf();
        let name = profile.file_name().unwrap().to_string_lossy();
        let procs = chromes_of(&format!("{name}( |$)"));
        assert!(procs.len() >= 3, "its processes found while it runs: {procs:?}");
        // All in this process's session, as they are in a benchmarked run's: what the
        // benchmark finds a killed run's processes by once they are reaped.
        let session = |pid: u64| std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().and_then(|s| s.get(s.rfind(')')? + 1..)?.split_whitespace().nth(3)?.parse::<i64>().ok());
        let mine = session(u64::from(std::process::id()));
        let others: Vec<_> = procs.iter().filter(|(pid, _)| session(*pid).is_some_and(|s| Some(s) != mine)).collect();
        assert!(others.is_empty(), "outside the session {mine:?}: {others:?}");
        let phases = b.close().unwrap();
        quickest = quickest.min(phases.close.chrome_exit);
        let left = chromes_left_after(&procs, std::time::Duration::from_secs(5));
        assert!(left.is_empty(), "still running: {left:?}");
        assert!(!profile.exists());
    }
    // Asked to close, a browser with a page open took tens of ms more to end on its own.
    assert!(quickest < std::time::Duration::from_millis(30), "the quickest of three stops took {quickest:?} to end Chrome");
}

#[test]
fn a_stopped_chrome_leaves_none_of_its_processes_running() {
    // Stopped, every one of its processes ends with it at once (the profile is thrown away:
    // nothing to save), and none is left to hold the profile's files.
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let url = serve_fixtures();
    b.cdp.call(None, "Target.createTarget", json!({"url": format!("{url}/form.html")})).unwrap();
    background_targets(&mut b);
    let profile = b.profile().unwrap().to_path_buf();
    // The whole name, to its end in the command line (-1 is not -10): a quote (Chrome quotes
    // it for its children on Windows), a space or the line's end follows it.
    let name = profile.file_name().unwrap().to_string_lossy();
    let procs = chromes_of(&format!("{name}(\"| |$)"));
    assert!(procs.len() >= 2, "its processes found while it runs: {procs:?}");
    drop(b);
    let left = chromes_left_after(&procs, std::time::Duration::from_secs(15));
    assert!(left.is_empty(), "still running: {left:?}");
    // Windows leaves it for the next launch (a_stopped_chromes_profile_is_left_for_the_next_launch_to_remove).
    assert!(cfg!(windows) || !profile.exists());
}

#[test]
#[cfg(windows)]
fn a_chrome_virtuoso_launched_ends_with_it_even_when_virtuoso_is_killed() {
    // Killed mid-flow (a CI timeout, a closed terminal), virtuoso stops nothing itself: its
    // Chrome went on running, an orphan holding its profile. In a job of virtuoso's, it ends
    // when virtuoso does, all its processes at once.
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, format!("url: {url}/form.html
---
- launchApp
- extendedWaitUntil:
    visible: \"Never there\"
    timeout: 20000
")).unwrap();
    let mut v = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(["--platform", "web", "test", "--no-ansi"]).arg(&path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    // Its Chrome: the processes whose command line names its profile, and theirs.
    let profile = format!("virtuoso-chrome-{}-", v.id());
    let procs = (0..40)
        .find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(250));
            Some(chromes_of(&profile)).filter(|p| p.len() >= 3)
        })
        .expect("no Chrome of virtuoso's found");
    v.kill().unwrap();
    v.wait().unwrap();
    let left = chromes_left_after(&procs, std::time::Duration::from_secs(15));
    assert!(left.is_empty(), "still running after virtuoso was killed: {left:?}");
}

#[test]
fn a_running_chrome_can_be_attached_to_and_is_left_running() {
    let Some(path) = chrome() else { return };
    let mut launched = Browser::launch(&path, true).unwrap();
    let mut attached = Browser::attach(&launched.ws_url).unwrap();
    assert_eq!(attached.product().unwrap(), launched.product().unwrap());
    drop(attached);
    // Still there: attaching does not stop the browser.
    assert!(launched.cdp.call(None, "Browser.getVersion", json!({})).is_ok());
}

#[test]
fn a_flow_on_an_attached_chrome_opens_a_tab_of_its_own_and_leaves_the_browsers_alone() {
    // Not virtuoso's browser: its blank tab is someone's, not the first page's to take.
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    let mut d = WebDevice::attach(&b.ws_url).unwrap();
    let url = serve(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    let mut open = tabs(&mut d);
    open.sort();
    assert_eq!(open, ["about:blank".to_string(), format!("{url}/start")]);
}

#[test]
#[cfg(not(windows))]
fn a_launched_chrome_takes_its_profile_with_it_when_dropped() {
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    // Its own profile, by name: the other tests' browsers make theirs alongside it meanwhile.
    let profile = b.profile().expect("a profile of its own").to_path_buf();
    assert!(profile.is_dir(), "{} is there while it runs", profile.display());
    drop(b);
    assert!(!profile.exists(), "{} left behind", profile.display());
}

/// Waits up to 15 s for `p` to be gone: whether it is. (Removed while its Chrome's processes
/// are still ending, it is gone once they are: over 5 s on a loaded machine.)
#[cfg(windows)]
fn gone(p: &std::path::Path) -> bool {
    (0..1500).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        !p.exists()
    })
}

#[test]
#[cfg(windows)]
fn a_stopped_chromes_profile_is_left_at_the_stop() {
    // Windows: removing it took longer than anything else in the stop (Chrome's files are let go
    // a while after its processes end). The run is over without waiting for that: it is left
    // for the sweep after the run.
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    let profile = b.profile().expect("a profile of its own").to_path_buf();
    drop(b);
    assert!(profile.is_dir(), "{} removed at the stop", profile.display());
}

#[test]
#[cfg(target_os = "linux")]
fn on_linux_a_launched_chromes_profile_is_in_memory_and_gone_once_it_is_closed() {
    // /dev/shm, not the runner's disk, where a flush showed in Chrome's start and its stop.
    let Some(path) = chrome() else { return };
    let mut b = Browser::launch(&path, true).unwrap();
    let profile = b.profile().unwrap().to_path_buf();
    assert_eq!(profile.parent().unwrap(), std::path::Path::new("/dev/shm"), "{}", profile.display());
    assert!(profile.join("Local State").exists() || profile.join("Default").exists(), "Chrome writes its profile there: {}", profile.display());
    b.close();
    assert!(!profile.exists(), "left behind: {}", profile.display());
}

#[test]
#[cfg(windows)]
fn a_launched_chromes_profile_is_in_virtuosos_own_folder_in_the_temp_folder() {
    // The sweep reads that folder alone, not the whole temp folder.
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    assert_eq!(b.profile().unwrap().parent().unwrap(), std::env::temp_dir().join("virtuoso"));
}

/// A process id no process has now: one that has exited.
#[cfg(windows)]
fn a_gone_pid() -> u32 {
    let mut c = std::process::Command::new("cmd").args(["/c", "exit"]).spawn().unwrap();
    let pid = c.id();
    c.wait().unwrap();
    pid
}

/// `virtuoso --platform web test` of a flow that opens a page and waits `wait_ms`, started with
/// a temp folder of its own (the run's and its sweeper's, apart from the other tests'), and
/// that folder's profiles' folder.
#[cfg(windows)]
fn web_run(wait_ms: u64) -> (std::process::Child, std::path::PathBuf, tempfile::TempDir) {
    let url = serve(PROBE);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, format!("url: {url}\n---\n- launchApp\n- assertVisible: hello\n- wait: {wait_ms}\n")).unwrap();
    let temp = dir.path().join("temp");
    let profiles = temp.join("virtuoso");
    std::fs::create_dir_all(&profiles).unwrap();
    let v = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .args(["--platform", "web", "test", "--no-ansi"])
        .arg(&path)
        .env("TMP", &temp)
        .env("TEMP", &temp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    (v, profiles, dir)
}

/// Waits up to 15 s for the last sweep's summary in `profiles` to name virtuoso `pid`: whether
/// it did.
#[cfg(windows)]
fn swept_after(profiles: &std::path::Path, pid: u32) -> bool {
    (0..1500).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::read_to_string(profiles.join("last-sweep.txt")).is_ok_and(|s| s.contains(&format!("after virtuoso {pid})")))
    })
}

#[test]
#[cfg(windows)]
fn the_profiles_runs_left_are_swept_after_a_run_exits_not_while_it_runs() {
    // Removing them beside Chrome as it started (a thread of the run's) raced its start for the
    // disk: Chrome listened seconds late. After the run, nothing of the sweep competes with a flow.
    if chrome().is_none() {
        return;
    }
    let gone_pid = a_gone_pid();
    let (mut v, profiles, _dir) = web_run(1500);
    // Made as the run starts: in its folder before its sweep could be.
    let left = profiles.join(format!("virtuoso-chrome-{gone_pid}-0"));
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join("Local State"), "{}").unwrap();
    let own = profiles.join(format!("virtuoso-chrome-{}-0", v.id()));
    let started = (0..1000).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        own.join("DevToolsActivePort").exists()
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    let during = (left.exists(), v.try_wait().unwrap().is_none());
    let status = v.wait().unwrap();
    assert!(started && status.success(), "the run's Chrome started and the run passed");
    assert_eq!(during, (true, true), "a profile left was removed while the run ran");
    assert!(gone(&left), "{} left after the run", left.display());
    assert!(swept_after(&profiles, v.id()), "the sweep after the run tells what it did");
}

#[test]
#[cfg(windows)]
fn the_sweep_after_a_run_leaves_the_profiles_of_virtuosos_still_running_alone() {
    // Another virtuoso's (a process still running).
    if chrome().is_none() {
        return;
    }
    let mut other = std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
    let (mut v, profiles, _dir) = web_run(0);
    let theirs = profiles.join(format!("virtuoso-chrome-{}-0", other.id()));
    std::fs::create_dir_all(&theirs).unwrap();
    let ran = v.wait().unwrap().success();
    let swept = swept_after(&profiles, v.id());
    let theirs_there = theirs.is_dir();
    let _ = other.kill();
    let _ = other.wait();
    let _ = std::fs::remove_dir_all(&theirs);
    assert!(ran && swept, "the run and its sweep");
    assert!(theirs_there, "another virtuoso's profile removed");
}

#[test]
fn a_launched_chrome_tells_how_its_start_and_stop_went_when_closed() {
    // The time a run spends outside its flows, phase by phase: CI runs explain their own slow ones.
    // (Chrome's phases: a Firefox's start is not split into them.)
    if chrome().is_none() {
        return;
    }
    let Some(mut d) = device() else { return };
    let ready = std::time::Instant::now();
    let p = d.close().expect("a launched browser's phases");
    let l = p.launch;
    assert!(l.began <= l.spawned && l.spawned <= l.listening && l.listening <= l.connected && l.connected <= ready, "{l:?}");
    assert!(l.profile_tries >= 1);
    // Windows leaves the profile for the sweep after the run.
    assert_eq!(p.close.profile_removed.is_none(), cfg!(windows), "{:?}", p.close);
    let lines = p.lines(None, ready, std::time::Duration::from_millis(1));
    assert!(lines.starts_with("phases: spawn ") && lines.contains("chrome listening"), "{lines}");
    assert!(d.close().is_none(), "closed once");
}

#[test]
fn an_attached_chrome_has_no_phases_of_its_start() {
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    let mut d = WebDevice::attach(&b.ws_url).unwrap();
    assert!(d.close().is_none());
}

#[test]
fn a_web_run_prints_its_phases_once_its_browser_is_closed() {
    if chrome().is_none() {
        return;
    }
    let url = serve(PROBE);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, format!("url: {url}\n---\n- launchApp\n- assertVisible: hello\n")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(["--platform", "web", "test", "--no-ansi"]).arg(&path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    let phases = lines.iter().position(|l| l.starts_with("phases: start ")).unwrap_or_else(|| panic!("no phases line: {text}"));
    assert!(lines[phases].contains(" | close "), "{text}");
    // After the flow's steps, before the run's result; the step lines as they were.
    let passed = lines.iter().position(|l| l.contains("Flow Passed in")).unwrap();
    assert!(lines.iter().position(|l| l.starts_with("Launch app... COMPLETED ")).unwrap() < phases && phases < passed, "{text}");
    // No sweep during the run, and no look at the last one's summary either (Windows: a file the
    // last sweeper just wrote, scanned as it is opened, ~9 ms a run). It is in the profiles'
    // folder, last-sweep.txt, for whoever wants it.
    assert!(!lines.iter().any(|l| l.contains("sweep")), "{text}");
}

#[test]
fn every_launch_prints_its_phases() {
    // Inside the flow, Launch app split: the tab attached to, the page set up, the navigation
    // answered, its load, and the settle after it (the tracker's idle check), so each CI run
    // explains its own slow launches.
    if chrome().is_none() {
        return;
    }
    let url = serve(PROBE);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, format!("url: {url}\n---\n- launchApp\n- assertVisible: hello\n- launchApp\n- tapOn: hello\n")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(["--platform", "web", "test", "--no-ansi"]).arg(&path).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{}{err}", String::from_utf8_lossy(&out.stdout));
    let launches: Vec<&str> = err.lines().filter(|l| l.starts_with("launch: ")).collect();
    assert_eq!(launches.len(), 2, "one line a launch: {err}");
    for l in &launches {
        let fields: Vec<&str> = l["launch: ".len()..].split(", ").collect();
        let names = ["attach ", "set-up ", "navigate answered ", "load ", "settle "];
        assert_eq!(fields.len(), names.len(), "{l}");
        for (f, n) in fields.iter().zip(names) {
            assert!(f.starts_with(n), "{n} in {l}");
        }
        assert!(l.contains(" ms (1 try)"), "{l}");
    }
    // The second launch's settle is seen (the tap's settle reads the tracker's idle check): how
    // long it took and how it ended.
    assert!(launches[1].contains("settle ") && (launches[1].ends_with("(quiet)") || launches[1].ends_with("(capped)")), "{}", launches[1]);
}

#[test]
fn a_launch_keeps_its_split_and_the_next_settle_or_read_sees_its_idle_check() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, true);
    let l = d.last_launch().expect("the launch's split").clone();
    assert!(l.attach.is_some() && l.setup.is_some() && l.navigation.loaded.is_some(), "{l:?}");
    // Seen by the settle after it (the tracker's state brings the idle check), at no extra wait.
    let before = d.round_trips();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(d.round_trips() - before, 1);
    let (took, _) = d.last_launch().unwrap().settle.expect("seen by the settle");
    assert!(took <= std::time::Duration::from_millis(600), "{took:?}");
    // A launch that keeps its page: nothing attached or set up; its idle check seen by a read.
    d.launch_app(&url, &LaunchOptions { stop_app: false, ..Default::default() }).unwrap();
    let l = d.last_launch().unwrap().clone();
    assert!(l.attach.is_none() && l.setup.is_none() && l.settle.is_none(), "{l:?}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while d.last_launch().unwrap().settle.is_none() && std::time::Instant::now() < deadline {
        let screen = d.fresh_screen().unwrap();
        assert!(screen.elements.iter().any(|e| e.text == "hello"), "the read is the page's");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(d.last_launch().unwrap().settle.is_some(), "seen by a read");
}

const PROBE: &str = "<!doctype html><title>Probe</title><p id=greeting>hello</p>";

#[test]
fn a_launched_chrome_is_ready_after_one_wait_on_it() {
    // The tabs it has and the news of new ones, asked together (its version only when asked).
    let Some(d) = device() else { return };
    // Firefox first opens its BiDi session and subscribes to the events: two more.
    assert_eq!(d.round_trips(), if on_firefox() { 3 } else { 1 });
}

#[test]
fn a_first_launch_waits_on_chrome_three_times() {
    // The tab Chrome started with attached to, the page set up (its events, the tracker, its
    // screen) in one go, and the navigation.
    let Some(mut d) = device() else { return };
    d.use_flow_contexts(false);
    let url = serve(PROBE);
    let before = d.round_trips();
    launch(&mut d, &url, true);
    // (Firefox: a tab of the flow's own made and brought to the front first, two more.)
    assert_eq!(d.round_trips() - before, if on_firefox() { 5 } else { 3 });
}

#[test]
fn a_first_launch_in_a_context_of_its_own_waits_on_chrome_five_times() {
    // The context made and a tab made in it first (Windows: a context's cookie store is in
    // memory, a new profile's waited ~730 ms on the disk): two more than taking Chrome's tab.
    // Flow contexts are Chrome's (see `web::flow_contexts`): none on a Firefox run.
    if on_firefox() {
        return;
    }
    let Some(mut d) = device() else { return };
    d.use_flow_contexts(true);
    let url = serve(PROBE);
    let before = d.round_trips();
    launch(&mut d, &url, true);
    assert_eq!(d.round_trips() - before, 5);
}

/// A server whose first answer is 204 No Content (a navigation Chrome reports as
/// net::ERR_ABORTED, as it does one cut short by a load already under way in the tab), then
/// `page` for every request after.
fn serve_aborting_first(page: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut first = true;
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            if request_path(&mut s).is_none() {
                continue;
            }
            if std::mem::take(&mut first) {
                let _ = write!(s, "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
            } else {
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
            }
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_launch_whose_navigation_is_aborted_navigates_again() {
    // On a slow machine the tab Chrome started with can still be loading its own blank page when
    // the flow's goes to it: that first navigation is aborted (net::ERR_ABORTED), not the flow's
    // URL failing.
    let Some(mut d) = device() else { return };
    let url = serve_aborting_first(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    assert_eq!(d.evaluate("document.title").unwrap(), json!("Probe"));
}

#[test]
fn a_launch_whose_url_nothing_answers_fails_on_its_first_try() {
    // Only an aborted navigation or one the machine had no socket for goes again: a refused
    // connection is the flow's URL failing, said at once.
    let Some(mut d) = device() else { return };
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let e = d.launch_app(&format!("http://127.0.0.1:{port}/start"), &LaunchOptions { stop_app: true, ..Default::default() }).unwrap_err().to_string();
    let refused = if on_firefox() { "NS_ERROR_CONNECTION_REFUSED" } else { "net::ERR_CONNECTION_REFUSED" };
    assert!(e.contains("could not open") && e.contains(refused), "{e}");
    assert_eq!(d.last_navigation().unwrap().tries, 1);
}

#[test]
fn launch_app_opens_the_flows_url_in_a_page_once_it_has_loaded() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    assert_eq!(d.evaluate("document.title").unwrap(), json!("Probe"));
    assert_eq!(d.evaluate("location.pathname").unwrap(), json!("/start"));
    assert_eq!(d.evaluate("document.readyState").unwrap(), json!("complete"));
}

#[test]
fn a_link_to_another_place_on_the_open_page_is_there_at_once() {
    // A navigation within the document loads nothing: no load event is coming to wait for.
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    let began = std::time::Instant::now();
    d.open_link(&format!("{url}/start#greeting"), None, false).unwrap();
    assert!(began.elapsed() < std::time::Duration::from_secs(5), "took {:?}", began.elapsed());
    assert_eq!(d.evaluate("location.hash").unwrap(), json!("#greeting"));
    assert_eq!(d.evaluate("document.title").unwrap(), json!("Probe"));
}

#[test]
fn a_page_that_alerts_as_it_loads_is_open_with_its_alert_on_the_screen() {
    // The page's load waits on the alert's answer: the alert is what the flow's next step sees.
    let Some(mut d) = device() else { return };
    let url = serve("<!doctype html><title>Alerting</title><p>before</p><script>alert('Loaded!')</script><p id=after>after</p>");
    let began = std::time::Instant::now();
    launch(&mut d, &format!("{url}/start"), false);
    assert!(began.elapsed() < std::time::Duration::from_secs(5), "took {:?}", began.elapsed());
    let s = d.screen().unwrap();
    assert!(s.elements.iter().any(|e| e.text == "Loaded!"), "the alert's message on screen");
    let (x, y) = centre_of_text(&mut d, "OK");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('after') !== null");
}

#[test]
fn clear_state_forgets_what_the_site_stored_and_a_plain_launch_keeps_it() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    d.evaluate("localStorage.setItem('k', 'v'); document.cookie = 'c=1; path=/'").unwrap();
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!("v"), "kept without clearState");
    assert_eq!(d.evaluate("document.cookie").unwrap(), json!("c=1"));
    launch(&mut d, &url, true);
    assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!(null), "gone after clearState");
    assert_eq!(d.evaluate("document.cookie").unwrap(), json!(""));
}

#[test]
fn a_fresh_browsers_first_clear_state_launch_clears_nothing_there_is_none() {
    // A Chrome virtuoso launched has a new profile and has been nowhere: nothing to forget.
    let url = serve(PROBE);
    let waits = |clear_state: bool| {
        let mut d = device()?;
        let before = d.round_trips();
        launch(&mut d, &url, clear_state);
        Some(d.round_trips() - before)
    };
    let (Some(cleared), Some(plain)) = (waits(true), waits(false)) else { return };
    assert_eq!(cleared, plain, "a clearState launch waited on Chrome {cleared} times, a plain one {plain}");
}

#[test]
fn stop_app_keeps_the_browsers_session_for_the_next_launch() {
    // Closing the flow's last tab must not end the browser's session: its session cookies are
    // the site's, kept unless clearState.
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    d.evaluate("document.cookie = 'c=1; path=/'").unwrap();
    d.stop_app(&url).unwrap();
    d.launch_app(&url, &LaunchOptions { stop_app: false, ..Default::default() }).unwrap();
    assert_eq!(d.evaluate("document.cookie").unwrap(), json!("c=1"));
}

#[test]
fn stop_app_closes_the_page_and_a_launch_after_opens_a_new_one() {
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    d.evaluate("window.marker = 42").unwrap();
    d.stop_app(&url).unwrap();
    assert!(d.evaluate("1").is_err(), "no page after stopApp");
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("window.marker === undefined").unwrap(), json!(true), "a fresh page");
}

#[test]
fn a_restarted_firefox_is_a_fresh_browser_a_launch_opens_its_page_in_and_chrome_is_not_restarted() {
    // A stuck Firefox is started afresh (see `with_one_restart`): what the site stored is gone
    // with the old profile, and a launch after it opens the page as ever.
    let Some(mut d) = device() else { return };
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    if !on_firefox() {
        let e = d.restart_browser().unwrap_err().to_string();
        assert!(e.contains("Firefox"), "{e}");
        return;
    }
    d.evaluate("localStorage.setItem('kept', 'yes')").unwrap();
    d.restart_browser().unwrap();
    assert!(d.evaluate("1").is_err(), "no page in the new browser until a launch");
    d.launch_app(&url, &LaunchOptions::default()).unwrap();
    assert_eq!(d.evaluate("localStorage.getItem('kept')").unwrap(), json!(null), "a new profile");
}

/// The browser's tabs, as Chrome lists them to the page's session.
fn tabs(d: &mut WebDevice) -> Vec<String> {
    let t = d.devtools("Target.getTargets", json!({})).unwrap();
    t["targetInfos"].as_array().unwrap().iter().filter(|t| t["type"] == "page").map(|t| t["url"].as_str().unwrap_or_default().to_string()).collect()
}

/// The browser context the flow's page is in.
fn context(d: &mut WebDevice) -> String {
    let t = d.devtools("Target.getTargetInfo", json!({})).unwrap();
    t["targetInfo"]["browserContextId"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_launched_chrome_opens_the_flows_page_in_the_tab_it_started_with() {
    // Chrome starts on a blank tab of its own: without a context of its own (Linux, macOS), the
    // first launch takes it, rather than opening a second beside it.
    let Some(mut d) = device() else { return };
    d.use_flow_contexts(false);
    let url = serve(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    // (Firefox can discard its first tab's context early: the flow's page is a tab of its own,
    // beside it.)
    let want = if on_firefox() { vec!["about:blank".to_string(), format!("{url}/start")] } else { vec![format!("{url}/start")] };
    assert_eq!(tabs(&mut d), want);
}

#[test]
fn a_launched_chrome_opens_the_flows_page_in_a_context_of_its_own_beside_the_tab_it_started_with() {
    // With a context of its own (Windows; here switched on on any system), as Playwright and
    // e2e do: the flow's tab in it, the blank tab Chrome started with left open in the default.
    // Flow contexts are Chrome's (see `web::flow_contexts`): none on a Firefox run.
    if on_firefox() {
        return;
    }
    let Some(mut d) = device() else { return };
    d.use_flow_contexts(true);
    let url = serve(PROBE);
    launch(&mut d, &format!("{url}/start"), false);
    let mut open = tabs(&mut d);
    open.sort();
    assert_eq!(open, ["about:blank".to_string(), format!("{url}/start")]);
    let own = context(&mut d);
    let all = d.devtools("Target.getTargets", json!({})).unwrap();
    let blank = all["targetInfos"].as_array().unwrap().iter().find(|t| t["type"] == "page" && t["url"] == "about:blank").unwrap();
    assert_ne!(blank["browserContextId"].as_str().unwrap_or_default(), own, "the flow's page is not in the default context");
}

#[test]
fn a_flow_context_is_kept_by_a_plain_launch_and_dropped_by_clear_state() {
    // Where flows run in a context of their own (Windows; here switched on on any system): a plain launch keeps it (what the site stored
    // stays, as Maestro's launchApp keeps an app's data), a clearState launch drops it for a new
    // one, where nothing is stored.
    // Flow contexts are Chrome's (see `web::flow_contexts`): none on a Firefox run.
    if on_firefox() {
        return;
    }
    let Some(mut d) = device() else { return };
    d.use_flow_contexts(true);
    let url = serve(PROBE);
    launch(&mut d, &url, false);
    let first = context(&mut d);
    d.evaluate("localStorage.setItem('k', 'v')").unwrap();
    launch(&mut d, &url, false);
    assert_eq!(context(&mut d), first, "kept by a plain launch");
    launch(&mut d, &url, true);
    let fresh = context(&mut d);
    assert_ne!(fresh, first, "a new context after clearState");
    assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!(null));
    let contexts = d.devtools("Target.getBrowserContexts", json!({}));
    if let Ok(c) = contexts {
        let ids: Vec<&str> = c["browserContextIds"].as_array().map(|a| a.iter().filter_map(serde_json::Value::as_str).collect()).unwrap_or_default();
        assert!(!ids.contains(&first.as_str()), "the old context is disposed: {ids:?}");
    }
    // Closed, the flow's context goes before the browser does.
    assert!(d.close().is_some(), "a launched browser's phases");
}

#[test]
fn a_flow_starts_with_nothing_an_earlier_flow_stored() {
    // Each flow is a device of its own: what one flow's site stored (a cookie, localStorage) is
    // not there for the next, whether flows run in a context of their own or a fresh profile.
    let url = serve(PROBE);
    {
        let Some(mut d) = device() else { return };
        launch(&mut d, &url, false);
        d.evaluate("document.cookie = 'c=1; max-age=3600'; localStorage.setItem('k', 'v'); 0").unwrap();
        assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!("v"));
        d.close();
    }
    let Some(mut d) = device() else { return };
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("document.cookie").unwrap(), json!(""));
    assert_eq!(d.evaluate("localStorage.getItem('k')").unwrap(), json!(null));
}

#[test]
fn flows_run_in_a_context_of_their_own_on_windows_only() {
    assert!(virtuoso::web::flow_contexts("windows"));
    assert!(!virtuoso::web::flow_contexts("linux"));
    assert!(!virtuoso::web::flow_contexts("macos"));
}

#[test]
fn the_platform_is_web() {
    let Some(d) = device() else { return };
    assert_eq!(d.platform(), "web");
}


const SCREEN: &str = r#"<!doctype html><title>Screen</title>
<style>body{margin:0} #box{position:absolute;left:10px;top:20px;width:100px;height:50px}</style>
<button id="by-id">Alpha</button>
<button data-testid="by-testid">Bravo</button>
<button id="both-id" data-testid="both-testid">Golf</button>
<input id="field" placeholder="Delta placeholder">
<input id="filled" value="India value">
<input id="secret" type="password" value="hunter2">
<p id="mixed">Order <b>ready</b> now</p>
<div id="gone" style="display:none">Foxtrot hidden</div>
<div id="invisible" style="visibility:hidden">Juliet invisible</div>
<input id="tick" type="checkbox" checked>
<button id="off" disabled>Kilo</button>
<div id="box">Box</div>
<div id="pointer" style="cursor:pointer">Lima</div>
"#;

fn screen_of(page: &'static str) -> Option<(WebDevice, virtuoso::screen::ScreenTree)> {
    let mut d = device()?;
    let url = serve(page);
    launch(&mut d, &url, false);
    let s = d.screen().unwrap();
    Some((d, s))
}

fn by_id<'a>(s: &'a virtuoso::screen::ScreenTree, id: &str) -> &'a virtuoso::screen::Element {
    s.elements.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no element with id {id} in {:?}", s.elements.iter().map(|e| (&e.id, &e.text)).collect::<Vec<_>>()))
}

#[test]
fn the_screen_has_each_rendered_element_with_its_own_text_and_id() {
    use virtuoso::screen::*;
    let Some((_d, s)) = screen_of(SCREEN) else { return };
    let alpha = by_id(&s, "by-id");
    assert_eq!(alpha.text, "Alpha");
    assert_eq!(&*alpha.class, "button");
    assert!(alpha.has(CLICKABLE) && alpha.has(ENABLED) && alpha.has(VISIBLE));
    assert_eq!(by_id(&s, "by-testid").text, "Bravo", "data-testid stands in for a missing id");
    assert_eq!(by_id(&s, "both-id").text, "Golf", "the id, where it has both");
    assert!(!s.elements.iter().any(|e| e.id == "both-testid"));
    // Text inside inline tags stays theirs: the paragraph has only its own.
    assert_eq!(by_id(&s, "mixed").text, "Order now");
    assert!(s.elements.iter().any(|e| e.text == "ready" && e.depth > by_id(&s, "mixed").depth));
}

#[test]
fn each_element_of_a_screen_is_of_the_pages_origin_its_app() {
    let Some(mut d) = device() else { return };
    let url = serve(SCREEN);
    launch(&mut d, &url, false);
    let s = d.screen().unwrap();
    assert!(!s.elements.is_empty());
    assert!(s.elements.iter().all(|e| *e.package == *url), "{url}: {:?}", s.elements.iter().map(|e| &e.package).collect::<std::collections::HashSet<_>>());
}

#[test]
fn fields_show_their_value_as_text_their_placeholder_as_hint() {
    use virtuoso::screen::*;
    let Some((_d, s)) = screen_of(SCREEN) else { return };
    let field = by_id(&s, "field");
    assert_eq!((field.text.as_str(), field.hint.as_str()), ("", "Delta placeholder"));
    assert!(field.has(EDITABLE) && field.has(CLICKABLE));
    assert_eq!(by_id(&s, "filled").text, "India value");
    assert!(by_id(&s, "secret").has(PASSWORD));
}

#[test]
fn hidden_elements_are_not_on_the_screen() {
    let Some((_d, s)) = screen_of(SCREEN) else { return };
    for id in ["gone", "invisible"] {
        assert!(!s.elements.iter().any(|e| e.id == id), "{id}");
    }
    assert!(!s.elements.iter().any(|e| e.text.contains("Foxtrot") || e.text.contains("Juliet")));
}

#[test]
fn states_bounds_and_the_viewport_are_read() {
    use virtuoso::screen::*;
    let Some((mut d, s)) = screen_of(SCREEN) else { return };
    assert!(by_id(&s, "tick").has(CHECKABLE) && by_id(&s, "tick").has(CHECKED));
    assert!(!by_id(&s, "off").has(ENABLED));
    assert!(by_id(&s, "pointer").has(CLICKABLE), "a pointer cursor marks a clickable div");
    assert!(!by_id(&s, "box").has(CLICKABLE));
    assert_eq!(by_id(&s, "box").bounds, Rect { left: 10, top: 20, right: 110, bottom: 70 });
    let (w, h) = (d.evaluate("innerWidth").unwrap(), d.evaluate("innerHeight").unwrap());
    assert_eq!((json!(s.width), json!(s.height)), (w, h));
    d.evaluate("document.getElementById('field').focus()").unwrap();
    assert!(by_id(&d.screen().unwrap(), "field").has(FOCUSED));
}

const INPUT: &str = r#"<!doctype html><title>Input</title>
<style>body{margin:0} button,input{display:block;width:200px;height:40px;margin:4px}</style>
<script>window.log=[]; const L=(e)=>log.push(e.type+(e.isTrusted?'':'(untrusted)'));</script>
<button id="b" onclick="L(event)" ondblclick="L(event)" onmousedown="window.down=performance.now()" onmouseup="window.held=performance.now()-window.down">Tap me</button>
<form id="f" onsubmit="event.preventDefault(); log.push('submit')"><input id="name" oninput="L(event)"><input id="next"><button id="go">Go</button></form>
"#;

fn centre(d: &mut WebDevice, id: &str) -> (i32, i32) {
    let s = d.screen().unwrap();
    by_id(&s, id).bounds.center()
}

fn log(d: &mut WebDevice) -> Vec<String> {
    serde_json::from_value(d.evaluate("log").unwrap()).unwrap()
}

#[test]
fn a_navigation_keeps_when_it_was_answered_and_when_its_page_then_loaded() {
    // Answered once the page commits (for this text page, once its body begins: 500 ms on),
    // loaded after: the two together are the page's wait.
    let Some(mut d) = device() else { return };
    let url = format!("{}/drip?ms=500", serve_fixtures());
    launch(&mut d, &url, false);
    let n = d.last_navigation().expect("the launch's navigation").clone();
    assert_eq!(n.tries, 1);
    let loaded = n.loaded.expect("loaded");
    assert!(n.answered + loaded >= std::time::Duration::from_millis(450), "{n:?}");
    // (A busy Windows runner's Chrome has been seen to answer a navigation 11 s late: what is
    // checked is that the times are kept, not how quick the machine is.)
    assert!(n.answered + loaded < std::time::Duration::from_secs(30), "{n:?}");
}

#[test]
fn a_tap_is_a_trusted_click_and_two_taps_a_double_click() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    d.tap(x, y).unwrap();
    assert_eq!(log(&mut d), ["click"]);
    d.taps(x, y, 2, std::time::Duration::from_millis(50)).unwrap();
    assert_eq!(log(&mut d), ["click", "click", "click", "dblclick"]);
}

#[test]
fn a_tap_is_one_wait_on_chrome_its_move_press_and_release_sent_together() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    let before = d.round_trips();
    d.tap(x, y).unwrap();
    assert_eq!(d.round_trips() - before, 1);
    assert_eq!(log(&mut d), ["click"]);
}

#[test]
fn typing_is_one_wait_on_chrome_its_keys_sent_together() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    let before = d.round_trips();
    d.input_text("Ada").unwrap();
    assert_eq!(d.round_trips() - before, 1);
    assert_eq!(d.evaluate("document.getElementById('name').value").unwrap(), json!("Ada"));
}

#[test]
fn a_flows_double_tap_on_the_web_is_a_double_click_with_its_clicks_back_to_back() {
    // clickCount 2 makes the double click whatever the gap: the 100 ms a phone's double tap
    // leaves between its taps only adds time.
    use virtuoso::runner::{FlowStatus, RunOptions, Runner};
    let Some(mut d) = device() else { return };
    let page = r#"<!doctype html><title>Double</title><script>window.at=[]; window.log=[]</script>
<button onclick="at.push(performance.now())" ondblclick="log.push('dblclick')">Double-click me</button>"#;
    let flow = virtuoso::flow::parse_flow(&format!("url: {}
---
- launchApp
- doubleTapOn: Double-click me
", serve(page)), None).unwrap();
    let r = Runner::new(&mut d, RunOptions::default()).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(log(&mut d), ["dblclick"]);
    let at: Vec<f64> = serde_json::from_value(d.evaluate("at").unwrap()).unwrap();
    assert_eq!(at.len(), 2);
    assert!(at[1] - at[0] < 50.0, "{} ms between the clicks", at[1] - at[0]);
}

#[test]
fn a_long_press_holds_the_button_down() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    let before = d.round_trips();
    d.long_press(x, y, 600).unwrap();
    assert_eq!(d.round_trips() - before, 2, "the move and the press go together, then the release");
    let held = d.evaluate("window.held").unwrap().as_f64().unwrap();
    // Past the 500 ms a page takes for a long press (held 520, see web_long_press).
    assert!(held >= 505.0, "held {held} ms");
}

#[test]
fn erasing_and_pressing_a_key_are_one_wait_on_chrome_each() {
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.input_text("abcde").unwrap();
    let before = d.round_trips();
    d.erase_text(3).unwrap();
    assert_eq!(d.round_trips() - before, 1);
    assert_eq!(d.evaluate("document.getElementById('name').value").unwrap(), json!("ab"));
    let before = d.round_trips();
    d.press_key(Key::Tab).unwrap();
    assert_eq!(d.round_trips() - before, 1);
    assert_eq!(d.evaluate("document.activeElement.id").unwrap(), json!("next"));
}

#[test]
fn text_is_typed_into_the_focused_field_and_erased() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.input_text("Grace Hopper").unwrap();
    assert_eq!(d.evaluate("document.getElementById('name').value").unwrap(), json!("Grace Hopper"));
    assert!(log(&mut d).contains(&"input".to_string()), "the page heard it: {:?}", log(&mut d));
    d.erase_text(7).unwrap();
    assert_eq!(d.evaluate("document.getElementById('name').value").unwrap(), json!("Grace"));
}

#[test]
fn keys_reach_the_page_as_a_user_presses_them() {
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.press_key(Key::Tab).unwrap();
    assert_eq!(d.evaluate("document.activeElement.id").unwrap(), json!("next"));
    d.press_key(Key::Enter).unwrap();
    assert!(log(&mut d).contains(&"submit".to_string()), "Enter submits the form: {:?}", log(&mut d));
    assert!(d.press_key(Key::VolumeUp).is_err(), "no volume on a page");
}

#[test]
fn back_goes_back_in_the_pages_history() {
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    let url = serve(INPUT);
    launch(&mut d, &format!("{url}/a"), false);
    d.evaluate("location.href = '/b'").unwrap();
    wait_for(&mut d, "location.pathname === '/b'");
    d.press_key(Key::Back).unwrap();
    wait_for(&mut d, "location.pathname === '/a'");
}

fn wait_for(d: &mut WebDevice, condition: &str) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < end {
        if d.evaluate(condition).map(|v| v == json!(true)).unwrap_or(false) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("never true: {condition}");
}

#[test]
fn the_fixture_site_is_served_and_a_form_on_it_filled_in() {
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &url, false);
    assert_eq!(d.evaluate("document.title").unwrap(), json!("Fixture"));
    let (x, y) = centre(&mut d, "nav-form");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.title === 'Form' && document.readyState === 'complete'");
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    d.input_text("Ada").unwrap();
    d.press_key(virtuoso::device::Key::Enter).unwrap();
    wait_for(&mut d, "document.getElementById('status').textContent === 'Signed up Ada'");
}

/// A page with a button that counts its clicks.
const COUNTS_CLICKS: &str = "<!doctype html><body><button id=go style='width:200px;height:80px' onclick='window.n = (window.n || 0) + 1'>Go</button><script>window.n = 0</script></body>";

/// The page `d` is on, as another DevTools client sees it, with Chrome dropping its input until
/// `for_ms` from now (Input.setIgnoreInputEvents, from a second connection): as Chrome on the
/// emulator dropped every input for a second or more after a load (web-android-bench 46082418803).
fn drop_input_for(ws_url: &str, url: &str, for_ms: u64) -> std::thread::JoinHandle<()> {
    // The connection lives on a thread of its own (a Browser stays on the thread it was made
    // on): back once Chrome is dropping the page's input.
    let (ws_url, url) = (ws_url.to_string(), url.to_string());
    let (dropping, started) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let mut other = Browser::attach(&ws_url).unwrap();
        let targets = other.cdp.call(None, "Target.getTargets", json!({})).unwrap();
        let target = targets["targetInfos"].as_array().unwrap().iter().find(|t| t["type"] == "page" && t["url"].as_str().unwrap_or_default().starts_with(&url)).expect("the page")["targetId"].clone();
        let session = other.cdp.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true})).unwrap()["sessionId"].as_str().unwrap().to_string();
        other.cdp.call(Some(&session), "Input.setIgnoreInputEvents", json!({"ignore": true})).unwrap();
        dropping.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(for_ms));
        other.cdp.call(Some(&session), "Input.setIgnoreInputEvents", json!({"ignore": false})).unwrap();
        // Kept a while: the session's end must not be what lets the input through.
        std::thread::sleep(std::time::Duration::from_millis(2000));
    });
    started.recv().unwrap();
    t
}

#[test]
fn on_a_device_the_first_tap_after_a_launch_waits_until_the_page_takes_input() {
    // Chrome on the emulator dropped input for up to 4 s after a load, frames and the first
    // paint already shown (web-android-bench 46082418803): virtuoso's first taps and keys did
    // nothing. There, the first input after a navigation is preceded by a pointer move to its
    // point, sent again until the page has seen one.
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    let mut d = WebDevice::attach(&b.ws_url).unwrap();
    d.confirm_input_delivery(true);
    let url = serve(COUNTS_CLICKS);
    launch(&mut d, &url, false);
    let (x, y) = centre(&mut d, "go");
    let dropping = drop_input_for(&b.ws_url, &url, 800);
    let t = std::time::Instant::now();
    d.tap(x, y).unwrap();
    let took = t.elapsed();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(d.evaluate("window.n").unwrap(), json!(1), "the tap after {took:?}");
    assert!(took < std::time::Duration::from_millis(3000), "{took:?}");
    dropping.join().unwrap();
}

/// A button in a frame, the frame clear of the page's corner.
const BUTTON_IN_A_FRAME: &str = r#"<!doctype html><body style="margin:0"><script>window.n = 0</script><iframe id=f style="position:absolute;left:60px;top:60px;width:300px;height:200px;border:0" srcdoc="<body style='margin:0'><button style='width:200px;height:80px' onclick='parent.n++'>In frame</button></body>"></iframe></body>"#;

#[test]
fn the_check_that_a_page_takes_input_does_not_wait_out_its_cap_over_a_frame() {
    // The check's moves are seen by the page's own window: one over a frame goes to the frame's
    // document, and the check waited its whole 5 s (web-android-bench 46087583020: iframe_form's
    // first tap, 5.2 s). Over a frame, the check moves at the page's corner.
    let Some(mut d) = device() else { return };
    d.confirm_input_delivery(true);
    launch(&mut d, &serve(BUTTON_IN_A_FRAME), false);
    wait_for(&mut d, "document.getElementById('f').contentDocument && document.getElementById('f').contentDocument.readyState === 'complete'");
    let t = std::time::Instant::now();
    d.tap(160, 100).unwrap();
    let took = t.elapsed();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(d.evaluate("window.n").unwrap(), json!(1), "the tap after {took:?}");
    assert!(took < std::time::Duration::from_millis(1500), "{took:?}");
}

/// A button that moves the moment the page first sees the mouse, a decoy taking its place: as a
/// page that rebuilds itself while the check waits for its input (stale_dom's list, each second).
const MOVES_WHEN_THE_MOUSE_COMES: &str = r#"<!doctype html><title>Moves</title><body style="margin:0"><script>window.hits = []</script>
<button id=a onclick="hits.push('a')" style="position:absolute;left:20px;top:20px;width:160px;height:60px">Tap me</button>
<script>addEventListener('mousemove', function once() { removeEventListener('mousemove', once, true); document.getElementById('a').style.top = '220px'; const d = document.createElement('button'); d.textContent = 'Decoy'; d.onclick = () => hits.push('decoy'); d.style.cssText = 'position:absolute;left:20px;top:20px;width:160px;height:60px'; document.body.appendChild(d); }, true)</script></body>"#;

#[test]
fn an_element_that_moves_while_the_check_waits_for_input_is_tapped_where_it_went() {
    // stale_dom on the emulator (web-android-bench 46087583020): the check waited 2.3 s for the
    // page to take input, the list rebuilt meanwhile, and the tap went where the target had been
    // (a decoy: "progress reset"). A check that waited has the element found again.
    use virtuoso::runner::{FlowStatus, RunOptions, Runner};
    let Some(path) = chrome() else { return };
    let b = Browser::launch(&path, true).unwrap();
    let mut d = WebDevice::attach(&b.ws_url).unwrap();
    d.confirm_input_delivery(true);
    let url = serve(MOVES_WHEN_THE_MOUSE_COMES);
    // The page's input dropped from the moment its tab is there, for longer than the launch. The
    // tap waits until it is being dropped: on a slow machine (the CI Mac) the thread attached
    // after the check had already seen its move, and the tap itself was the input dropped.
    let (ws, u) = (b.ws_url.clone(), url.clone());
    let (ignoring_tx, ignoring) = std::sync::mpsc::channel();
    let dropping = std::thread::spawn(move || {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut other = Browser::attach(&ws).unwrap();
        let target = loop {
            let targets = other.cdp.call(None, "Target.getTargets", json!({})).unwrap();
            if let Some(t) = targets["targetInfos"].as_array().unwrap().iter().find(|t| t["type"] == "page" && t["url"].as_str().unwrap_or_default().starts_with(&u)) {
                break t["targetId"].clone();
            }
            assert!(std::time::Instant::now() < end, "no page");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let session = other.cdp.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true})).unwrap()["sessionId"].as_str().unwrap().to_string();
        other.cdp.call(Some(&session), "Input.setIgnoreInputEvents", json!({"ignore": true})).unwrap();
        ignoring_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        other.cdp.call(Some(&session), "Input.setIgnoreInputEvents", json!({"ignore": false})).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2000));
    });
    let launch = virtuoso::flow::parse_flow(&format!("url: {url}\n---\n- launchApp\n"), None).unwrap();
    let r = Runner::new(&mut d, RunOptions::default()).run_flow(&launch);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    ignoring.recv_timeout(std::time::Duration::from_secs(20)).expect("input never dropped");
    let flow = virtuoso::flow::parse_flow(&format!("url: {url}\n---\n- tapOn: Tap me\n"), None).unwrap();
    let r = Runner::new(&mut d, RunOptions::default()).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    assert_eq!(d.evaluate("hits").unwrap(), json!(["a"]));
    dropping.join().unwrap();
}

#[test]
fn a_tap_on_a_page_that_takes_input_is_one_click_with_or_without_the_check() {
    // The check's move is a move to the tap's own point: one click either way, and the hover a
    // tap's move gives the page, no more.
    for check in [false, true] {
        let Some(mut d) = device() else { return };
        d.confirm_input_delivery(check);
        launch(&mut d, &serve(COUNTS_CLICKS), false);
        let (x, y) = centre(&mut d, "go");
        d.tap(x, y).unwrap();
        d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(d.evaluate("window.n").unwrap(), json!(1), "check {check}");
    }
}

/// The settle page, opened and marked, with `button` tapped: how long a settle took and what it said.
fn settle_after(button: &str, no_change_ms: u64, timeout_ms: u64) -> Option<(WebDevice, std::time::Duration, virtuoso::device::Settled)> {
    let mut d = device()?;
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/settle.html"), false);
    let (x, y) = centre(&mut d, button);
    d.mark().unwrap();
    d.tap(x, y).unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(no_change_ms), std::time::Duration::from_millis(timeout_ms)).unwrap();
    Some((d, t.elapsed(), s))
}

fn out(d: &mut WebDevice) -> String {
    d.evaluate("document.getElementById('out').textContent").unwrap().as_str().unwrap_or_default().to_string()
}

#[test]
fn a_settle_waits_for_a_change_that_comes_a_moment_after_the_tap() {
    let Some((mut d, _, s)) = settle_after("later", 150, 5000) else { return };
    assert_eq!(out(&mut d), "Appeared later");
    assert!(s.changed && !s.timed_out, "{s:?}");
}

#[test]
fn a_settle_waits_for_what_an_action_draws_in_its_next_frames() {
    // A change two animation frames after the tap (~33 ms), with nothing changing meanwhile.
    let Some((mut d, took, s)) = settle_after("drawlater", 300, 5000) else { return };
    assert_eq!(out(&mut d), "Drawn", "after {took:?}");
    assert!(s.changed && !s.timed_out, "{s:?}");
}

#[test]
fn a_loop_in_frames_the_page_ran_before_the_action_does_not_hold_its_settle() {
    // settle.html runs one from its load on: each frame asks for the next, none of them the
    // tap's work. The tap's own change ("Appeared later", 300 ms on) is waited for, no more.
    let Some((mut d, took, s)) = settle_after("later", 150, 5000) else { return };
    assert_eq!(out(&mut d), "Appeared later");
    assert!(!s.timed_out, "{s:?}");
    assert!(took < std::time::Duration::from_millis(900), "took {took:?}: held by the loop");
}

#[test]
fn a_settle_waits_for_the_tasks_an_action_schedules() {
    // Six tasks one after another (a scheduler's chunks of work, as React's posts them, ~6 ms
    // each), then the change, with nothing changing meanwhile.
    let Some((mut d, took, s)) = settle_after("scheduled", 300, 5000) else { return };
    assert_eq!(out(&mut d), "Scheduled", "after {took:?}");
    assert!(s.changed && !s.timed_out, "{s:?}");
}

#[test]
fn a_settle_waits_for_a_fetched_answers_body_to_be_read() {
    // The answer's headers end the fetch; its body comes after (300 ms here, a few ms on a busy
    // machine for any answer), and what the page does with it after that.
    let Some((mut d, took, s)) = settle_after("drip", 100, 5000) else { return };
    assert_eq!(out(&mut d), "Dripped ok", "after {took:?}");
    assert!(!s.timed_out);
}

#[test]
fn a_settle_waits_for_fetch_and_xhr_requests() {
    for (button, text) in [("fetch", "Fetched ok"), ("xhr", "Xhr ok")] {
        let Some((mut d, took, s)) = settle_after(button, 100, 5000) else { return };
        assert_eq!(out(&mut d), text, "after {took:?}");
        assert!(!s.timed_out);
    }
}

#[test]
fn a_settle_waits_for_an_animation_to_end() {
    let Some((mut d, took, _)) = settle_after("animate", 100, 5000) else { return };
    assert_eq!(out(&mut d), "Grown", "after {took:?}");
}

#[test]
fn neither_an_endless_animation_nor_a_long_poll_holds_a_settle() {
    // The page has a spinner that never stops; the long poll answers after 15 s.
    let Some((_, took, s)) = settle_after("longpoll", 100, 10_000) else { return };
    assert!(took < std::time::Duration::from_secs(4), "took {took:?}");
    assert!(!s.timed_out);
}

#[test]
fn a_still_page_settles_at_once_and_unchanged() {
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/form.html"), false);
    d.mark().unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(100), std::time::Duration::from_secs(5)).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_millis(600), "took {:?}", t.elapsed());
    assert!(!s.changed && !s.timed_out, "{s:?}");
}

#[test]
fn a_page_that_never_stops_changing_times_out() {
    // It changes every 10 ms, never as long as a quiet, and the interval the tap started is
    // work pending through the settle's second (a ticker counts for 1.5 s, then looks still
    // between ticks: see the ticker and poll tests), however a busy machine spaces its ticks.
    let Some((_, took, s)) = settle_after("forever", 200, 1000) else { return };
    assert!(s.timed_out, "{s:?}");
    assert!(took >= std::time::Duration::from_millis(900) && took < std::time::Duration::from_millis(2500), "took {took:?}");
}

#[test]
fn an_interval_an_action_started_holds_a_settle_between_its_ticks() {
    // It changes the page every 120 ms, longer than the quiet: between ticks the page only
    // looks still. (On a slow runner a 50 ms ticker's late tick looked still the same way.)
    let Some((_, took, s)) = settle_after("ticker", 200, 1200) else { return };
    assert!(s.timed_out, "{s:?} after {took:?}");
}

#[test]
fn an_interval_whose_tick_comes_late_still_holds_the_settle() {
    // A busy runner (or a throttled page) runs a tick well after it was due. The first tick
    // here takes 300 ms and changes nothing, so the next comes late, with the page quiet for
    // longer than the settle asks: the ticker is still the action's work.
    let Some((_, took, s)) = settle_after("lateticker", 200, 1200) else { return };
    assert!(s.timed_out, "{s:?} after {took:?}");
}

#[test]
fn a_settle_after_an_action_on_a_still_page_looks_once_when_its_quiet_is_due() {
    // Right after an action the quiet cannot be up yet: the first look waits for it.
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "next");
    let settle = |d: &mut WebDevice| {
        let before = d.round_trips();
        let s = d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
        assert!(!s.timed_out);
        d.round_trips() - before
    };
    d.tap(x, y).unwrap();
    assert_eq!(settle(&mut d), 1, "after a tap");
    d.press_key(Key::Tab).unwrap();
    assert_eq!(settle(&mut d), 1, "after a key");
}

#[test]
fn a_settle_right_after_a_tap_asks_the_page_at_once_and_the_page_waits_out_the_quiet() {
    // The page's wait measures the quiet from the input as it reached the page (its own
    // listeners mark it), so the settle asks at once: a sleep on the host first oversleeps on a
    // busy machine (a 10 ms one took 45-65 ms on the CI Mac). The quiet is still waited out.
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    d.evaluate("addEventListener('click', () => { window.clickAt = performance.now(); }, true); { const u = __virtuoso.until; __virtuoso.until = (...a) => { window.askedAt = window.askedAt || performance.now(); return u(...a).then((r) => { window.stillAt = window.stillAt || performance.now(); return r; }); }; } 0").unwrap();
    d.tap(x, y).unwrap();
    let s = d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    assert!(!s.timed_out, "{s:?}");
    let mut ms = |k: &str| d.evaluate(&format!("window.{k}")).unwrap().as_f64().unwrap_or(f64::NAN);
    let (click, asked, still) = (ms("clickAt"), ms("askedAt"), ms("stillAt"));
    let quiet = virtuoso::web::web_quiet(false).as_secs_f64() * 1000.0;
    // On the CI Mac a tap's own round trip to Chrome is near the quiet itself (the page was asked
    // 6.4 and 9.3 ms after the click with no sleep on the host), so there the timing cannot tell
    // the two apart; elsewhere it is ~1-3 ms against a sleep of the whole quiet.
    // Nor on Firefox, whose BiDi round trip is longer than half the quiet (7 ms on a Windows runner).
    if !cfg!(target_os = "macos") && !on_firefox() {
        assert!(asked - click < quiet / 2.0, "the page was asked {:.1} ms after the click (quiet {quiet} ms)", asked - click);
    }
    assert!(still - click >= quiet - 1.0, "still {:.1} ms after the click, before the quiet was up", still - click);
}

#[test]
fn a_scrolls_drag_is_not_an_action_whose_timers_hold_its_settle() {
    // scrollUntilVisible looks again after each drag: a load the scroll set going (a feed's
    // next page) is not waited for, as a tap's work is.
    use virtuoso::device::Lift;
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/loadmore.html"), false);
    std::thread::sleep(std::time::Duration::from_millis(1600));
    d.swipe((200, 250), (200, 50), 50, Lift::Cancel).unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::ZERO, std::time::Duration::from_secs(5)).unwrap();
    assert!(!s.timed_out);
    assert!(t.elapsed() < std::time::Duration::from_millis(300), "took {:?}", t.elapsed());
    assert_eq!(out(&mut d), "Nothing yet", "back before the load");
}

#[test]
fn a_ticker_the_page_started_as_it_loaded_does_not_hold_a_taps_settle() {
    // A flow marks nothing itself: the tap is the action. The page's own 1 s ticker, set before
    // the tap, is not work the tap started.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/ticking.html"), false);
    let (x, y) = centre(&mut d, "still");
    d.tap(x, y).unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    assert!(!s.timed_out, "{s:?}");
    assert!(t.elapsed() < std::time::Duration::from_millis(600), "took {:?}", t.elapsed());
}

#[test]
fn an_interval_a_tap_starts_holds_its_settle_with_no_mark_but_the_tap() {
    // Long after the page loaded, and nothing marked but the tap itself.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/settle.html"), false);
    std::thread::sleep(std::time::Duration::from_millis(1700));
    let (x, y) = centre(&mut d, "ticker");
    d.tap(x, y).unwrap();
    let s = d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_millis(1200)).unwrap();
    assert!(s.timed_out, "{s:?}");
}

#[test]
fn a_key_with_typing_next_settles_without_waiting_for_quiet() {
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    let settle = |d: &mut WebDevice| {
        let t = std::time::Instant::now();
        d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
        t.elapsed()
    };
    settle(&mut d);
    // The quickest of five: a busy machine can slow a settle, never speed one up. Timed from
    // the key: the page dates the quiet from when the key reached it (#140), so a settle asked
    // after the key's round trip can end sooner than the quiet after it.
    let quickest = |d: &mut WebDevice, types: bool| {
        (0..5)
            .map(|_| {
                d.next_step_types(types);
                let t = std::time::Instant::now();
                d.press_key(Key::Tab).unwrap();
                settle(d);
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    // Chrome hands the page its keys in order, each handled before the next: no quiet between.
    let quiet = virtuoso::web::web_quiet(false);
    let took = quickest(&mut d, true);
    assert!(took < quiet, "took {took:?}");
    // Anything else next (a lookup, a tap) waits for the quiet as before: counted from the key
    // (the page counts the quiet from the key as it reached it, before the settle began).
    let took = (0..5)
        .map(|_| {
            d.next_step_types(false);
            let t = std::time::Instant::now();
            d.press_key(Key::Tab).unwrap();
            settle(&mut d);
            t.elapsed()
        })
        .min()
        .unwrap();
    assert!(took >= quiet, "took {took:?}");
}

#[test]
fn a_key_with_a_lookup_next_settles_without_waiting_for_quiet() {
    // As with typing next: the page has handled each key before the next goes, and the lookup
    // that follows polls for what it wants itself. (The work the keys start is still waited for.)
    use virtuoso::device::Key;
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    let quiet = virtuoso::web::web_quiet(false);
    let took = (0..5)
        .map(|_| {
            d.next_step_types(false);
            d.next_step_reads(true);
            d.press_key(Key::Tab).unwrap();
            let t = std::time::Instant::now();
            d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
            t.elapsed()
        })
        .min()
        .unwrap();
    assert!(took < quiet, "took {took:?}");
}

#[test]
fn typing_with_a_lookup_next_still_waits_for_the_work_it_starts() {
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/settle.html"), false);
    let (x, y) = centre(&mut d, "search");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    d.next_step_reads(true);
    d.input_text("a").unwrap();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Searched");
}

#[test]
fn typing_with_typing_next_still_waits_for_the_work_it_starts() {
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/settle.html"), false);
    let (x, y) = centre(&mut d, "search");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    d.next_step_types(true);
    d.input_text("a").unwrap();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Searched");
}

#[test]
fn a_swipe_takes_its_own_time_not_its_time_and_the_answers_to_its_moves() {
    // Its moves go at their times (Chrome takes them in order): waiting for each one's answer
    // (10-30 ms, the first move's frame-aligned) before the next one's wait added them all.
    use virtuoso::device::Lift;
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/list.html"), false);
    // The quickest of five (~400 ms before, ~340 after): a busy machine slows one, never speeds it.
    let took = (0..5)
        .map(|_| {
            let t = std::time::Instant::now();
            d.swipe((300, 500), (300, 300), 300, Lift::Fling).unwrap();
            t.elapsed()
        })
        .min()
        .unwrap();
    // (A macOS runner draws a frame every 44 ms: the lift's answer and the two frames after it
    // are 100 ms or so of the swipe there, where they are 40 on a desktop. Late in the whole web
    // suite a runner's quickest of five came to 493-545 ms on 25 November, where the test alone
    // stays well under 480.)
    let limit = if cfg!(target_os = "macos") { 600 } else { 375 };
    assert!(took < std::time::Duration::from_millis(limit), "took {took:?}");
}

#[test]
fn a_settle_on_a_busy_page_waits_in_the_page_one_wait_on_chrome() {
    // The page watches itself until it is still: Chrome is asked once, not every poll.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/settle.html"), false);
    let (x, y) = centre(&mut d, "fetch");
    d.tap(x, y).unwrap();
    let before = d.round_trips();
    let s = d.settle(std::time::Duration::from_millis(100), std::time::Duration::from_secs(5)).unwrap();
    assert!(!s.timed_out && s.changed, "{s:?}");
    assert_eq!(d.round_trips() - before, 1);
    assert_eq!(out(&mut d), "Fetched ok");
}

#[test]
fn a_settle_with_a_lookup_next_brings_the_screen_it_settled_on_and_a_tap_on_it_goes_at_once() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(INPUT), false);
    let (x, y) = centre(&mut d, "b");
    d.next_step_reads(true);
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    let before = d.round_trips();
    let screen = d.screen().unwrap();
    assert_eq!(d.round_trips() - before, 0, "the settle's screen");
    let (x, y) = by_id(&screen, "b").bounds.center();
    // Nothing went to Chrome since that still screen: the tap needs no read to check it.
    assert!(d.tap_if_unchanged(&screen, x, y).unwrap().is_none(), "tapped");
    assert_eq!(d.round_trips() - before, 1);
    assert_eq!(log(&mut d), ["click", "click"]);
    // With anything else next, no screen comes back with the settle (it would be thrown away).
    d.next_step_reads(false);
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    let before = d.round_trips();
    d.screen().unwrap();
    assert_eq!(d.round_trips() - before, 1, "read afresh");
}

#[test]
fn a_poll_re_armed_for_ever_holds_a_settle_only_briefly() {
    let Some((_, took, s)) = settle_after("poll", 200, 10_000) else { return };
    assert!(!s.timed_out, "{s:?}");
    assert!(took < std::time::Duration::from_secs(3), "took {took:?}");
}



fn opened(page: &str) -> Option<WebDevice> {
    let mut d = device()?;
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/{page}"), false);
    Some(d)
}

fn num(d: &mut WebDevice, js: &str) -> f64 {
    d.evaluate(js).unwrap().as_f64().unwrap_or(f64::NAN)
}

#[test]
fn a_swipe_up_scrolls_the_page_down_by_about_its_length() {
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    d.swipe((300, 600), (300, 200), 300, Lift::Still).unwrap();
    let y = num(&mut d, "scrollY");
    assert!((350.0..=450.0).contains(&y), "scrollY {y}");
    d.swipe((300, 200), (300, 600), 300, Lift::Still).unwrap();
    assert_eq!(num(&mut d, "scrollY"), 0.0, "and back");
}

#[test]
fn a_swipe_over_a_scrolling_box_scrolls_the_box_not_the_page() {
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    let (x, y) = centre(&mut d, "inner");
    d.swipe((x, y + 60), (x, y - 60), 300, Lift::Still).unwrap();
    assert!(num(&mut d, "document.getElementById('inner').scrollTop") >= 100.0);
    assert_eq!(num(&mut d, "scrollY"), 0.0);
    let (x, y) = centre(&mut d, "wide");
    d.swipe((x + 100, y), (x - 100, y), 300, Lift::Still).unwrap();
    assert!(num(&mut d, "document.getElementById('wide').scrollLeft") >= 150.0, "sideways too");
}

#[test]
fn a_scroll_drag_its_settle_and_the_next_read_are_two_waits_on_chrome_and_the_read_is_of_where_it_landed() {
    // scrollUntilVisible's drag: the touch, then one call that waits for the frames and brings
    // the tracker's state and the screen with it, which the settle and the next read take.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    let (x, y) = centre(&mut d, "inner");
    let before = d.round_trips();
    d.swipe((x, y + 60), (x, y - 60), 50, Lift::Cancel).unwrap();
    d.settle(std::time::Duration::ZERO, std::time::Duration::from_secs(5)).unwrap();
    let read = d.screen().unwrap();
    assert_eq!(d.round_trips() - before, 2);
    assert!(num(&mut d, "document.getElementById('inner').scrollTop") >= 100.0);
    let fresh = d.screen().unwrap();
    let places = |s: &virtuoso::screen::ScreenTree| s.elements.iter().map(|e| (e.text.clone(), e.bounds)).collect::<Vec<_>>();
    assert_eq!(places(&read), places(&fresh), "the read is of the scrolled box");
}

#[test]
fn a_scroll_drags_read_has_the_rows_the_page_draws_for_where_it_landed() {
    // A virtualized table draws the rows a scroll brings in after the scroll's frame, in a task
    // of its own: read in that frame, the screen still had the rows of before the scroll.
    for phone in [false, true] {
        let Some(mut d) = device() else { return };
        if phone {
            d.emulate(virtuoso::web::emulation("pixel-7").unwrap()).unwrap();
        }
        let url = serve_fixtures();
        launch(&mut d, &format!("{url}/virtual.html"), false);
        for step in 1..=5 {
            d.swipe((200, 440), (200, 56), 0, virtuoso::device::Lift::Cancel).unwrap();
            let top = num(&mut d, "document.getElementById('box').scrollTop");
            let s = d.screen().unwrap();
            let middle = format!("Row {}", ((top + 240.0) / 36.0) as u32);
            assert!(top > 300.0 * step as f64, "phone {phone}, step {step}: scrolled to {top}");
            assert!(s.elements.iter().any(|e| e.text == middle), "phone {phone}, step {step}: no {middle} at {top}");
        }
    }
}

#[test]
#[cfg(not(windows))]
fn a_headless_chromes_frames_are_not_held_to_a_screens_sixty_a_second() {
    // Chrome's own switch (--disable-frame-rate-limit): Firefox draws at its 60 a second.
    if on_firefox() {
        return;
    }
    // Nothing is shown: a frame with something to draw is drawn as soon as it is asked for. At a
    // screen's 60 a second every wait on one (each step of a scroll waits on three) was 17 ms.
    let Some(mut d) = opened("form.html") else { return };
    let ms = num(&mut d, "new Promise(r => { const t = performance.now(); let n = 0; const f = () => { document.body.append(n); ++n < 30 ? requestAnimationFrame(f) : r(performance.now() - t) }; requestAnimationFrame(f); })");
    assert!(ms < 250.0, "30 frames took {ms} ms");
}

#[test]
fn a_look_straight_after_a_read_with_nothing_sent_since_is_that_read() {
    // One step's last read is the next step's first look (an assert after an assert, a tap's
    // lookup after an assert or a scroll): a few ms old, nothing sent to Chrome since. A wait's
    // later looks, and a look once anything was sent or a moment has passed, read again.
    let Some(mut d) = opened("form.html") else { return };
    let first = d.screen().unwrap();
    let before = d.round_trips();
    let again = d.screen().unwrap();
    assert_eq!(d.round_trips(), before, "no wait on Chrome for the second look");
    assert_eq!(again.elements.len(), first.elements.len());
    d.fresh_screen().unwrap();
    assert_eq!(d.round_trips(), before + 1, "a fresh read is a read");
    std::thread::sleep(std::time::Duration::from_millis(60));
    d.screen().unwrap();
    assert_eq!(d.round_trips(), before + 2, "a moment later, the page may have changed on its own");
    d.evaluate("document.body.append('more')").unwrap();
    let after = d.round_trips();
    let changed = d.screen().unwrap();
    assert_eq!(d.round_trips(), after + 1, "something was sent to Chrome since");
    assert!(changed.elements.iter().any(|e| e.text.contains("more")));
}

/// scrollUntilVisible to `row` in the virtualized fixture (with `query`), in-process: the page,
/// for what it saw.
fn scrolled_to_row(query: &str, row: u32) -> Option<WebDevice> {
    scrolled_to_row_as(None, query, row)
}

/// The same, emulating `phone`.
fn scrolled_to_row_as(phone: Option<&str>, query: &str, row: u32) -> Option<WebDevice> {
    use virtuoso::runner::{FlowStatus, RunOptions, Runner};
    let mut d = device()?;
    if let Some(p) = phone {
        d.emulate(virtuoso::web::emulation(p).unwrap()).unwrap();
    }
    let url = serve_fixtures();
    let flow = virtuoso::flow::parse_flow(&format!("url: {url}/virtual.html{query}
---
- launchApp
- scrollUntilVisible:
    element: Row {row}
    timeout: 60000
"), None).unwrap();
    let r = Runner::new(&mut d, RunOptions::default()).run_flow(&flow);
    assert_eq!(r.status, FlowStatus::Passed, "{:?}", r.error);
    Some(d)
}

#[test]
fn a_scroll_through_a_virtualized_list_steps_as_far_as_the_rows_it_keeps_allow() {
    // A list keeps rows beyond its box's edges (5 here, 180 px each side): a step may go the
    // box and both of those, less a row, and every row is still in the page at some landing.
    // At the box's height less its edges (432 px), 32 400 px took 75 wheels.
    let Some(mut d) = scrolled_to_row("?overscan=5", 900) else { return };
    let wheels = num(&mut d, "wheels");
    assert!(wheels <= 45.0, "{wheels} wheels");
    assert_eq!(num(&mut d, "[...drawn].filter(i => i <= 900).length"), 901.0, "every row was in the page");
}

#[test]
fn a_scroll_through_a_list_that_keeps_fewer_rows_than_it_did_misses_none() {
    // The rows kept above the box come and go: a step sized by what is kept now can land past
    // rows that were never in the page. Seen (no row in common with the last landing), the
    // scroll goes back and takes smaller steps.
    let Some(mut d) = scrolled_to_row("?overscan=5&alternate=1", 900) else { return };
    assert_eq!(num(&mut d, "[...drawn].filter(i => i <= 900).length"), 901.0, "every row was in the page");
}

#[test]
fn a_phones_scroll_through_a_virtualized_list_steps_as_far_as_its_kept_rows_allow() {
    // A finger's drag has to fit on the screen: a longer step is several drags in one send
    // (Chrome takes a session's events in order), landed once. 900 rows took 75 drags' landings.
    let Some(d) = device() else { return };
    drop(d);
    let began = std::time::Instant::now();
    let Some(mut d) = scrolled_to_row_as(Some("pixel-7"), "?overscan=5", 900) else { return };
    let landings = num(&mut d, "landings");
    assert!(landings <= 45.0, "{landings} landings in {:?}", began.elapsed());
    assert_eq!(num(&mut d, "[...drawn].filter(i => i <= 900).length"), 901.0, "every row was in the page");
}

#[test]
fn a_phones_scroll_drag_moves_the_list_as_far_as_the_drag() {
    // Chrome keeps the first few pixels of a finger's drag as slop: the list moved 15 px less
    // than each drag, and a scroll took a step more in 30.
    use virtuoso::device::Lift;
    let Some(mut d) = device() else { return };
    d.emulate(virtuoso::web::emulation("pixel-7").unwrap()).unwrap();
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/virtual.html"), false);
    let (x, y) = (200, 440);
    d.swipe((x, y), (x, y - 300), 50, Lift::Cancel).unwrap();
    let top = num(&mut d, "document.getElementById('box').scrollTop");
    assert!((295.0..=305.0).contains(&top), "a 300 px drag scrolled {top} px");
}

#[test]
fn a_scroll_step_is_the_pages_own_scroll_not_a_wheel_or_a_finger() {
    // As Selenium's phone port scrolls, and a step then costs one frame, not the three a wheel's
    // answer and its landing took: the box scrolled by script, on a desktop and on a phone.
    for phone in [None, Some("pixel-7")] {
        let Some(mut d) = scrolled_to_row_as(phone, "?overscan=5", 900) else { return };
        assert_eq!((num(&mut d, "wheels"), num(&mut d, "touches")), (0.0, 0.0), "{phone:?}");
        assert_eq!(num(&mut d, "[...drawn].filter(i => i <= 900).length"), 901.0, "{phone:?}: every row was in the page");
    }
}

#[test]
fn a_scroll_to_a_row_the_list_keeps_below_its_box_goes_straight_to_it() {
    // A row in the page but below the box is one exact step away, not a full one past it.
    let Some(mut d) = scrolled_to_row("?overscan=40", 30) else { return };
    assert!(num(&mut d, "wheels") <= 1.0, "{} wheels", num(&mut d, "wheels"));
    let top = num(&mut d, "document.getElementById('box').scrollTop");
    assert!(top > 0.0 && top < 30.0 * 36.0, "scrolled to {top}");
}

#[test]
fn a_scroll_drag_scrolls_the_box_under_it_as_far_as_the_drag_on_a_desktop_and_a_phone() {
    // The page's own scroll of the box under the drag's start (see
    // a_scroll_step_is_the_pages_own_scroll_not_a_wheel_or_a_finger): the drag's length.
    use virtuoso::device::Lift;
    for phone in [false, true] {
        let Some(mut d) = device() else { return };
        if phone {
            d.emulate(virtuoso::web::emulation("pixel-7").unwrap()).unwrap();
        }
        launch(&mut d, &format!("{}/list.html", serve_fixtures()), false);
        d.evaluate("window.kinds = []; for (const k of ['wheel', 'touchstart']) addEventListener(k, () => kinds.push(k), {capture: true, passive: true})").unwrap();
        let (x, y) = centre(&mut d, "inner");
        d.swipe((x, y + 60), (x, y - 60), 50, Lift::Cancel).unwrap();
        assert_eq!(d.evaluate("kinds").unwrap(), json!([]), "phone: {phone}");
        let top = num(&mut d, "document.getElementById('inner').scrollTop");
        assert_eq!(top, 120.0, "phone: {phone}");
    }
}

#[test]
fn an_element_its_box_cuts_off_is_read_as_clipped_and_one_wholly_inside_is_not() {
    use virtuoso::screen::{CLIPPED, VISIBLE};
    let Some(mut d) = opened("list.html") else { return };
    let s = d.screen().unwrap();
    let inner = by_id(&s, "inner").bounds;
    let rows: Vec<_> = s.elements.iter().filter(|e| e.id.starts_with("inner-")).collect();
    let cut = rows.iter().find(|e| e.bounds.top < inner.bottom && e.bounds.bottom > inner.bottom).expect("a row across the box's bottom edge");
    assert!(cut.has(VISIBLE) && cut.has(CLIPPED), "{:?}", cut.bounds);
    let whole = rows.iter().find(|e| e.bounds.bottom < inner.bottom - 5).expect("a row wholly inside");
    assert!(whole.has(VISIBLE) && !whole.has(CLIPPED), "{:?}", whole.bounds);
}

#[test]
fn a_drag_moves_what_follows_the_pointer() {
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    let before = d.round_trips();
    d.drag((x, y), (x + 200, y + 100), 100, 300).unwrap();
    // Firefox: what is under the pointer asked (a draggable's drag goes as its events), then
    // the whole drag as one sequence of actions.
    assert_eq!(d.round_trips() - before, if on_firefox() { 2 } else { 12 }, "the move and the press together, ten moves, the release");
    assert_eq!(out(&mut d), "Knob at 220,120");
}

#[test]
fn a_drag_drops_an_html5_draggable_on_its_target() {
    let Some(mut d) = opened("drag.html") else { return };
    let from = centre(&mut d, "card");
    let to = centre(&mut d, "zone");
    d.drag(from, to, 100, 300).unwrap();
    assert_eq!(out(&mut d), "Dropped card");
}

/// `virtuoso --platform web test` on a flow written to a temporary folder: its exit status and output.
fn run_flow(flow: &str) -> (Option<i32>, String) {
    run_flow_on(None, flow)
}

/// The same on a device (`--device`), when one is named.
fn run_flow_on(device: Option<&str>, flow: &str) -> (Option<i32>, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, flow).unwrap();
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso"));
    c.args(["--platform", "web"]);
    if let Some(d) = device {
        c.args(["--device", d]);
    }
    let o = c.args(["test", "--no-ansi"]).arg(&path).output().unwrap();
    (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn a_page_still_busy_after_its_load_is_not_typed_into_until_its_scripts_have_attached_their_handlers() {
    // A server-rendered page hydrating (#16): loaded, and nothing changing, but its scripts at
    // work in task after task for 400 ms before the boxes move the focus on. A digit typed then
    // stays in its box: a page is loaded once its task queue has drained after its load.
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/hydrating.html\n---\n- launchApp\n- tapOn:\n    id: otp-0\n- inputText: '4'\n- assertVisible:\n    id: otp-1\n    focused: true\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn a_still_page_is_tapped_straight_after_its_launch_with_no_wait_for_its_scripts() {
    // A page with nothing at work after its load is loaded at once: its first tap waits for no
    // cap. (Waiting for an idle callback did, on the macOS runners: ~1.1 s on a flow's first tap,
    // where Chrome there was slow to call it.) The quickest of three runs.
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let flow = format!("url: {url}/form.html\n---\n- launchApp\n- tapOn:\n    id: name\n");
    let quickest = (0..3)
        .map(|_| {
            let (code, out) = run_flow(&flow);
            assert_eq!(code, Some(0), "{out}");
            let line = out.lines().find(|l| l.starts_with("Tap on")).unwrap_or_default().to_string();
            let ms = line.rsplit("COMPLETED ").next().map(|t| t.trim()).and_then(|t| match t.strip_suffix(" ms") {
                Some(n) => n.parse::<f64>().ok(),
                None => t.strip_suffix(" s").and_then(|n| n.parse::<f64>().ok()).map(|s| s * 1000.0),
            });
            ms.unwrap_or_else(|| panic!("no tap time in {out}"))
        })
        .fold(f64::INFINITY, f64::min);
    assert!(quickest < 500.0, "first tap took {quickest} ms");
}

#[test]
fn with_virtuoso_timings_a_run_says_how_long_the_browser_took_to_start_and_to_stop() {
    // What a flow's own time leaves out, for a runner where every run is slower (Windows).
    if chrome().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.yaml");
    std::fs::write(&path, format!("url: {}
---
- launchApp
", serve(PROBE))).unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso")).env("VIRTUOSO_TIMINGS", "1").args(["--platform", "web", "test", "--no-ansi"]).arg(&path).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    for phase in ["connect", "close"] {
        assert!(err.lines().any(|l| l.starts_with(&format!("timing: {phase} ")) && l.ends_with(" ms")), "{phase} in {err}");
    }
}

#[test]
fn a_button_is_found_by_its_accessible_name_as_maestro_finds_it() {
    // Maestro matches a selector's text against an element's accessibility text as well as its
    // own: a glyph button is found by its aria-label.
    if chrome().is_none() {
        return;
    }
    let url = serve(
        "<!doctype html><title>Menus</title><p id=s>none</p>\
         <button aria-label='More actions for a.pdf' onclick=\"s.textContent='a chosen'\">⋯</button>\
         <button aria-label='More actions for b.pdf' onclick=\"s.textContent='b chosen'\">⋯</button>",
    );
    let (code, out) = run_flow(&format!("url: {url}\n---\n- launchApp\n- tapOn: More actions for b.pdf\n- assertVisible: b chosen\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn a_maestro_flow_runs_on_the_web_from_the_command_line() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!(
        "url: {url}\n---\n- launchApp:\n    clearState: true\n- tapOn: Form\n- tapOn:\n    id: name\n- inputText: Ada\n- pressKey: Enter\n- assertVisible: Signed up Ada\n- assertNotVisible: Not signed up\n"
    ));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_scrolls_a_long_page_to_the_element() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/list.html\n---\n- launchApp\n- scrollUntilVisible:\n    element:\n      id: row-150\n- tapOn: Row 150\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn an_element_under_a_fixed_footer_is_not_visible() {
    use virtuoso::screen::VISIBLE;
    let Some(mut d) = opened("sticky.html") else { return };
    let s = d.screen().unwrap();
    assert!(!by_id(&s, "accept").has(VISIBLE), "in the window but covered by the footer");
    assert!(by_id(&s, "footer").has(VISIBLE));
    assert!(by_id(&s, "state").has(VISIBLE));
}

#[test]
fn an_element_a_scrolling_box_hides_is_not_visible() {
    use virtuoso::screen::VISIBLE;
    let Some(mut d) = opened("clipped.html") else { return };
    let s = d.screen().unwrap();
    assert!(!by_id(&s, "low").has(VISIBLE), "in the window but below the box's bottom");
    assert!(by_id(&s, "top").has(VISIBLE));
}

#[test]
fn scroll_until_visible_drags_inside_the_box_under_the_middle_when_its_usual_drag_would_miss_it() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/shortbox.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Row 100\n- assertVisible: Row 100\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_scrolls_a_box_until_its_element_shows() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/clipped.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Low button\n- tapOn: Low button\n- assertVisible: Pressed\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_waits_for_rows_a_feed_loads_after_each_scroll() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/lazy.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Last row\n    timeout: 60000\n- assertVisible: Last row\n"));
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn scroll_until_visible_brings_an_element_out_from_under_a_fixed_footer() {
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow(&format!("url: {url}/sticky.html\n---\n- launchApp\n- scrollUntilVisible:\n    element: Accept\n- tapOn: Accept\n- assertVisible: Accepted\n"));
    assert_eq!(code, Some(0), "{out}");
}

/// The element with `text` on the screen: its centre.
fn centre_of_text(d: &mut WebDevice, text: &str) -> (i32, i32) {
    let s = d.screen().unwrap();
    s.elements.iter().find(|e| e.text == text).unwrap_or_else(|| panic!("no {text:?} on {:?}", s.elements.iter().map(|e| &e.text).filter(|t| !t.is_empty()).collect::<Vec<_>>())).bounds.center()
}

#[test]
fn an_alert_shows_on_the_screen_over_the_page_and_a_tap_on_ok_closes_it() {
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "alert");
    d.tap(x, y).unwrap();
    let s = d.screen().unwrap();
    let message = s.elements.iter().find(|e| e.text == "Saved!").expect("the alert's message on screen");
    assert!(message.window > 0, "in a window over the page");
    let (x, y) = centre_of_text(&mut d, "OK");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('out').textContent === 'Alert closed'");
}

#[test]
fn a_confirm_is_answered_by_the_button_tapped() {
    for (button, said) in [("OK", "Confirmed"), ("Cancel", "Cancelled")] {
        let Some(mut d) = opened("dialogs.html") else { return };
        let (x, y) = centre(&mut d, "confirm");
        d.tap(x, y).unwrap();
        assert!(d.screen().unwrap().elements.iter().any(|e| e.text == "Delete the draft?"));
        let (x, y) = centre_of_text(&mut d, button);
        d.tap(x, y).unwrap();
        wait_for(&mut d, &format!("document.getElementById('out').textContent === '{said}'"));
    }
}

#[test]
fn a_prompt_takes_the_text_typed_into_it() {
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "prompt");
    d.tap(x, y).unwrap();
    d.input_text("Ada").unwrap();
    let (x, y) = centre_of_text(&mut d, "OK");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('out').textContent === 'Hello Ada'");
}

#[test]
fn a_popup_whose_page_is_slow_to_answer_is_waited_for_not_read_while_it_is_still_blank() {
    // A tab a page opens starts on a blank document of its own until its page answers: a settle
    // then is not on that blank, and the screen after it is the popup's page.
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "slowpopup");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
    let texts = texts(&d.screen().unwrap());
    assert!(texts.iter().any(|t| t.trim() == "ok"), "the popup's page is the screen: {texts:?}");
}

#[test]
fn a_new_tab_is_followed_and_closing_it_comes_back() {
    for opener in ["newtab", "popup"] {
        let Some(mut d) = opened("dialogs.html") else { return };
        let (x, y) = centre(&mut d, opener);
        d.tap(x, y).unwrap();
        d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
        assert!(d.screen().unwrap().elements.iter().any(|e| e.text == "Second tab"), "{opener}: the new tab is the screen");
        let (x, y) = centre(&mut d, "close");
        d.tap(x, y).unwrap();
        d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
        assert!(d.screen().unwrap().elements.iter().any(|e| e.id == "dialogs-marker" || e.text == "Nothing yet"), "{opener}: back on the first tab");
    }
}

#[test]
fn a_tab_the_page_opened_is_taken_up_in_two_waits_on_chrome() {
    // Attached to, then set up (its events, the tracker for its next documents and its first,
    // its screen) in one send.
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "newtab");
    // Chrome's news of the tab comes with the tap's answer: the next call takes the tab up.
    d.tap(x, y).unwrap();
    if on_firefox() {
        // Firefox tells of the tab a moment after the tap's answer: as a flow does, the tap is
        // settled before the screen is read (the round trips are Chrome's).
        d.settle(std::time::Duration::from_millis(10), std::time::Duration::from_secs(5)).unwrap();
        let s = d.screen().unwrap();
        assert!(s.elements.iter().any(|e| e.text == "Second tab"));
        return;
    }
    let before = d.round_trips();
    let s = d.screen().unwrap();
    assert_eq!(d.round_trips() - before, 3, "two to take the tab up, one to read it");
    assert!(s.elements.iter().any(|e| e.text == "Second tab"));
}

/// The frames page, with its second frame served from another origin.
fn frames() -> Option<WebDevice> {
    let mut d = device()?;
    let url = serve_fixtures();
    let other = serve_fixtures();
    let port = other.rsplit(':').next().unwrap().to_string();
    launch(&mut d, &format!("{url}/frames.html?other={port}"), false);
    d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(10)).unwrap();
    Some(d)
}

fn texts(s: &virtuoso::screen::ScreenTree) -> Vec<String> {
    s.elements.iter().map(|e| e.text.clone()).filter(|t| !t.is_empty()).collect()
}

#[test]
fn a_same_origin_frame_is_read_and_typed_into_where_it_is_on_screen() {
    let Some(mut d) = frames() else { return };
    let s = d.screen().unwrap();
    let frame = by_id(&s, "same").bounds;
    let input = s.elements.iter().find(|e| e.hint == "Inner field" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).expect("the frame's field, inside the frame");
    let (x, y) = input.bounds.center();
    d.tap(x, y).unwrap();
    d.input_text("one").unwrap();
    let s = d.screen().unwrap();
    let (x, y) = s.elements.iter().find(|e| e.text == "Inner button" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).unwrap().bounds.center();
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Inner one".to_string()));
}

#[test]
fn a_frame_from_another_origin_is_read_and_typed_into_too() {
    let Some(mut d) = frames() else { return };
    let s = d.screen().unwrap();
    let frame = by_id(&s, "other").bounds;
    let input = s.elements.iter().find(|e| e.hint == "Inner field" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).unwrap_or_else(|| panic!("the other-origin frame's field: {:?}", texts(&s)));
    let (x, y) = input.bounds.center();
    d.tap(x, y).unwrap();
    d.input_text("two").unwrap();
    let s = d.screen().unwrap();
    let (x, y) = s.elements.iter().find(|e| e.text == "Inner button" && frame.contains(e.bounds.left + 1, e.bounds.top + 1)).unwrap().bounds.center();
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Inner two".to_string()), "{:?}", texts(&d.screen().unwrap()));
}

#[test]
fn a_shadow_dom_form_is_read_and_typed_into() {
    let Some(mut d) = frames() else { return };
    let (x, y) = centre(&mut d, "shadow-input");
    d.tap(x, y).unwrap();
    d.input_text("three").unwrap();
    let (x, y) = centre(&mut d, "shadow-button");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Shadow three");
}

#[test]
fn a_screenshot_is_a_png_of_the_viewport() {
    let Some(mut d) = opened("device.html") else { return };
    let png = d.screenshot().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    let (vw, vh) = (num(&mut d, "innerWidth * devicePixelRatio"), num(&mut d, "innerHeight * devicePixelRatio"));
    assert_eq!((w as f64, h as f64), (vw, vh));
}

#[test]
fn set_location_is_where_the_page_finds_itself() {
    let Some(mut d) = opened("device.html") else { return };
    d.set_location(-36.8485, 174.7633).unwrap();
    assert_eq!(d.evaluate("where()").unwrap(), json!([-36.8485, 174.7633]));
}

#[test]
fn permissions_set_before_a_clear_state_launch_hold_in_its_new_context() {
    // Clearing a site's data never took its permissions: the clearState launch's fresh context
    // is given those the flow set.
    // Flow contexts are Chrome's (see `web::flow_contexts`): none on a Firefox run.
    if on_firefox() {
        return;
    }
    let Some(mut d) = opened("device.html") else { return };
    d.use_flow_contexts(true);
    let url = d.evaluate("location.href").unwrap().as_str().unwrap().to_string();
    let origin = d.evaluate("location.origin").unwrap().as_str().unwrap().to_string();
    d.set_permissions(&origin, &[("notifications".into(), "allow".into())]).unwrap();
    let before = context(&mut d);
    launch(&mut d, &url, true);
    assert_ne!(context(&mut d), before, "a new context");
    assert_eq!(d.evaluate("permission('notifications')").unwrap(), json!("granted"));
}

#[test]
fn permissions_are_granted_and_denied_for_the_site() {
    let Some(mut d) = opened("device.html") else { return };
    let url = d.evaluate("location.origin").unwrap().as_str().unwrap().to_string();
    d.set_permissions(&url, &[("notifications".into(), "allow".into()), ("camera".into(), "deny".into())]).unwrap();
    assert_eq!(d.evaluate("permission('notifications')").unwrap(), json!("granted"));
    assert_eq!(d.evaluate("permission('camera')").unwrap(), json!("denied"));
}

#[test]
fn dark_mode_is_the_pages_colour_scheme() {
    let Some(mut d) = opened("device.html") else { return };
    if on_firefox() {
        let e = d.set_dark_mode(Some(true)).unwrap_err().to_string();
        assert!(e.contains("not on Firefox yet"), "{e}");
        return;
    }
    d.set_dark_mode(Some(true)).unwrap();
    assert_eq!(d.evaluate("matchMedia('(prefers-color-scheme: dark)').matches").unwrap(), json!(true));
    assert!(d.dark_mode().unwrap());
    d.set_dark_mode(Some(false)).unwrap();
    assert!(!d.dark_mode().unwrap());
}

#[test]
fn airplane_mode_takes_the_page_offline_and_back() {
    let Some(mut d) = opened("device.html") else { return };
    d.set_airplane_mode(Some(true)).unwrap();
    assert_eq!(d.evaluate("navigator.onLine").unwrap(), json!(false));
    assert_eq!(d.evaluate("fetch('device.html').then(() => 'got', () => 'failed')").unwrap(), json!("failed"));
    d.set_airplane_mode(Some(false)).unwrap();
    assert_eq!(d.evaluate("fetch('device.html').then(() => 'got', () => 'failed')").unwrap(), json!("got"));
}

#[test]
fn orientation_turns_the_viewport() {
    use virtuoso::device::Orientation;
    let Some(mut d) = opened("device.html") else { return };
    let before = d.round_trips();
    d.set_orientation(Orientation::Portrait).unwrap();
    assert_eq!(d.round_trips() - before, 2, "one look at the viewport, one change of it");
    assert!(num(&mut d, "innerHeight") > num(&mut d, "innerWidth"));
    d.set_orientation(Orientation::LandscapeLeft).unwrap();
    assert!(num(&mut d, "innerWidth") > num(&mut d, "innerHeight"));
}

#[test]
fn open_link_goes_to_the_page_and_hide_keyboard_waits_for_nothing() {
    let Some(mut d) = opened("device.html") else { return };
    let url = d.evaluate("location.origin").unwrap().as_str().unwrap().to_string();
    d.open_link(&format!("{url}/form.html"), None, false).unwrap();
    wait_for(&mut d, "document.title === 'Form'");
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    let t = std::time::Instant::now();
    d.await_keyboard(std::time::Duration::from_secs(5), std::time::Duration::from_millis(100)).unwrap();
    d.hide_keyboard().unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(1), "took {:?}", t.elapsed());
}

#[test]
fn a_recording_is_a_video_of_the_page() {
    let Some(mut d) = opened("list.html") else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.mp4");
    if on_firefox() {
        let e = d.start_recording(&path).unwrap_err().to_string();
        assert!(e.contains("not on Firefox yet"), "{e}");
        return;
    }
    d.start_recording(&path).unwrap();
    for _ in 0..5 {
        d.swipe((300, 600), (300, 300), 200, virtuoso::device::Lift::Still).unwrap();
        d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(5)).unwrap();
    }
    d.stop_recording().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 1000 && &bytes[4..8] == b"ftyp", "an mp4 of {} bytes", bytes.len());
}

fn virtuoso_web(args: &[&str]) -> (Option<i32>, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso")).args(["--platform", "web"]).args(args).output().unwrap();
    (o.status.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn a_session_drives_a_page_one_command_at_a_time() {
    if chrome().is_none() {
        return;
    }
    /// Stops the session however the test ends: a session left running holds the test's pipes.
    struct Stop;
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = virtuoso_web(&["session", "stop"]);
        }
    }
    let url = serve_fixtures();
    let _ = virtuoso_web(&["session", "stop"]);
    let (code, out) = virtuoso_web(&["session", "start", "--app", &url]);
    let _stop = Stop;
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = virtuoso_web(&["do", "launchApp", "tapOn: Form"]);
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = virtuoso_web(&["look"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Sign up"), "the form page: {out}");
}

#[test]
fn mcp_runs_flow_commands_on_a_page() {
    use std::io::{BufRead, BufReader, Write as _};
    if chrome().is_none() {
        return;
    }
    let url = serve_fixtures();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_virtuoso"))
        .args(["--platform", "web", "mcp"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "run", "arguments": {"appId": url, "commands": "- launchApp\n- assertVisible: Fixture"}}});
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
    writeln!(stdin, "{call}").unwrap();
    drop(stdin);
    let lines: Vec<String> = BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok).collect();
    child.wait().unwrap();
    assert!(lines[0].contains("web page"), "the instructions say it drives web pages too: {}", lines[0]);
    let reply: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
    assert_ne!(reply["result"]["isError"], json!(true), "{}", lines[1]);
}

#[test]
fn a_closed_shadow_root_is_read_and_tapped_into_too() {
    let Some(mut d) = frames() else { return };
    let s = d.screen().unwrap();
    assert_eq!(by_id(&s, "closed-hint").text, "Closed code: CLOSED-7");
    let (x, y) = centre(&mut d, "closed-button");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Closed pressed");
}

#[test]
fn a_settle_on_the_web_needs_no_more_than_100_ms_of_quiet() {
    // The tracker sees requests, timers and animations itself: the 300 ms a phone needs, to be
    // sure its accessibility events have stopped, would only slow every web step.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/form.html"), false);
    d.mark().unwrap();
    let t = std::time::Instant::now();
    let s = d.settle(std::time::Duration::from_millis(300), std::time::Duration::from_secs(5)).unwrap();
    let took = t.elapsed();
    assert!(!s.timed_out);
    assert!(took < std::time::Duration::from_millis(230), "took {took:?}");
}

#[test]
fn a_page_taller_than_the_window_is_a_scrolling_list() {
    use virtuoso::screen::SCROLLABLE;
    let Some(mut d) = opened("list.html") else { return };
    let s = d.screen().unwrap();
    assert!(s.elements[0].has(SCROLLABLE) && &*s.elements[0].class == "html", "the page scrolls: {:?}", s.elements[0]);
    let row = s.elements.iter().position(|e| e.id == "row-20").unwrap();
    assert_eq!(s.vertical_list_around(row), Some(0), "its rows are in it");
    let Some(mut d) = opened("form.html") else { return };
    assert!(!d.screen().unwrap().elements[0].has(SCROLLABLE), "a page that fits does not");
}


#[test]
fn a_swipe_on_a_drag_target_is_its_whole_time_to_the_page_and_a_fraction_of_it_to_wait_for() {
    // What takes pointer drags (touch-action: none) scrolls nothing, so nothing has to be given
    // the swipe's time to move: its events carry the times of a swipe that long (as far apart as
    // a finger's would be), and are sent one after another.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    // On a page just opened (an event cannot be older than its page: the first swipe waits
    // until the page is as old as the swipe is long), and on one that has been open a while:
    // each event as soon as the page has handled the one before it, not a frame or 16 ms apart
    // (130-150 ms a swipe on a desktop then; a macOS runner's frames are 44 ms apart).
    // A macOS runner answers the start and the lift, and draws the frames waited for, so slowly
    // that the swipe takes about as long there as one sent in its own time (518 ms against
    // about 505): no quicker, no slower, and to the page the same swipe. No time is asked of it.
    // Firefox takes no times of a swipe's events but its own: a drag target's swipe is sent in
    // real time there: about its 400 ms to the page (its press lands a round trip late; on a
    // busy Windows runner, as much as 135 ms late: 265 ms seen).
    let quick = if cfg!(target_os = "macos") || on_firefox() { 2000 } else { 70 };
    let seen = if on_firefox() { 200.0..=650.0 } else { 390.0..=410.0 };
    for (n, at_most) in [(1, 2000), (2, quick)] {
        let began = std::time::Instant::now();
        d.swipe((x, y), (x + 200, y + 100), 400, Lift::Fling).unwrap();
        // The swipe and its settle (with the quiet a web flow asks for): the step's time.
        d.settle(virtuoso::web::web_quiet(false), std::time::Duration::from_secs(5)).unwrap();
        let took = began.elapsed();
        assert_eq!(out(&mut d), "Knob at 220,120");
        assert!(took < std::time::Duration::from_millis(at_most), "swipe {n} took {took:?}");
        let lasted = num(&mut d, "seen.up - seen.down");
        assert!(seen.contains(&lasted), "swipe {n}: the page saw a swipe of {lasted} ms");
        // (A slow machine draws fewer frames than there are moves, and Chrome hands the page
        // the moves of one frame as one.)
        assert!(num(&mut d, "seen.moves") >= 1.0, "swipe {n}: its moves reach the page");
        d.evaluate("knob.style.left = knob.style.top = '20px'").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// The text of `out` on a read.
fn shown(s: &virtuoso::screen::ScreenTree) -> String {
    by_id(s, "out").text.clone()
}

#[test]
fn a_waits_look_comes_back_with_the_page_as_it_changed_not_at_its_next_poll() {
    // A wait looked again every 10 ms, and a look within 30 ms of the last read was that read:
    // the page was read every 30 ms, and a change seen up to 30 ms after it was made. A look
    // now waits in the page for its next change, and is the read of it. Here the page changes
    // twice, 15 ms apart: every time, the wait has the first before the second is made. On the
    // CI Mac a look's round trip alone can be longer than that, or than 150 ms (runs 46074451248,
    // 46105796125: both changes came between two looks, and the next look had the second at
    // once): there only that the change is seen is checked. Linux and Windows show the timing.
    // Firefox too: its look is a BiDi script call with no in-page wait for a change (run
    // 46149497951 had the second at once), so only that the change is seen is checked there.
    let mac = cfg!(target_os = "macos") || on_firefox();
    let gap = if mac { 150 } else { 15 };
    let ms = std::time::Duration::from_millis;
    let Some(mut d) = opened("drag.html") else { return };
    for round in 0..6 {
        // (The round before's second change has been made.)
        std::thread::sleep(ms(gap + 45));
        d.evaluate(&format!("out('Nothing yet'); void setTimeout(() => {{ out('first'); setTimeout(() => {{ out('second'); }}, {gap}); }}, {})", 90 + round * 7)).unwrap();
        let mut s = d.screen().unwrap();
        let began = std::time::Instant::now();
        while shown(&s) == "Nothing yet" {
            assert!(began.elapsed() < ms(3000), "round {round}: the change was never seen");
            s = d.screen_after_change(s, ms(200)).unwrap();
        }
        if mac {
            assert!(shown(&s) == "first" || shown(&s) == "second", "round {round}: {}", shown(&s));
        } else {
            assert_eq!(shown(&s), "first", "round {round}");
        }
    }
}

#[test]
fn a_waits_look_at_a_quiet_page_is_one_wait_on_chrome_of_30_ms() {
    // A page nothing changes on (or changing what no DOM change tells of: a field's value, a
    // style's animation) is still read as often as it was: every 30 ms, and no more.
    let ms = std::time::Duration::from_millis;
    let Some(mut d) = opened("drag.html") else { return };
    // As a flow's lookup begins: the settle before it brought its first read.
    d.next_step_reads(true);
    d.settle(virtuoso::web::web_quiet(false), ms(5000)).unwrap();
    let s = d.screen().unwrap();
    // The first look after a read of another kind is the old one: 10 ms, then a read.
    let mut s = d.screen_after_change(s, ms(200)).unwrap();
    let before = d.round_trips();
    for _ in 0..3 {
        let began = std::time::Instant::now();
        s = d.screen_after_change(s, ms(200)).unwrap();
        let took = began.elapsed();
        // (The CI Mac's round trip alone is near these: run 46105796125 took 128 ms.)
        if !cfg!(target_os = "macos") {
            assert!(took >= ms(25) && took < ms(120), "a look at a quiet page took {took:?}");
        }
    }
    assert_eq!(d.round_trips() - before, 3, "one wait on Chrome a look");
    // No longer than the wait has left.
    let began = std::time::Instant::now();
    d.screen_after_change(s, ms(4)).unwrap();
    if !cfg!(target_os = "macos") {
        assert!(began.elapsed() < ms(25), "a look with 4 ms left took {:?}", began.elapsed());
    }
    // A value typed by script changes no DOM: seen at the next 30 ms look.
    d.evaluate("const i = document.body.appendChild(document.createElement('input')); i.id = 'typed'; void setTimeout(() => { i.value = 'hello'; }, 60)").unwrap();
    let began = std::time::Instant::now();
    let mut s = d.screen().unwrap();
    while by_id(&s, "typed").text != "hello" {
        assert!(began.elapsed() < ms(1000), "the value was never seen");
        s = d.screen_after_change(s, ms(200)).unwrap();
    }
}

#[test]
fn a_waits_looks_at_a_page_that_keeps_changing_are_no_more_often_than_they_were() {
    // A page changing all the time (a ticker) would otherwise be read at every change: a look
    // that found the page changed, and not what the wait is for, comes back 30 ms later.
    let ms = std::time::Duration::from_millis;
    let Some(mut d) = opened("drag.html") else { return };
    d.evaluate("void setInterval(() => { out('tick ' + performance.now()); }, 2)").unwrap();
    let mut s = d.screen().unwrap();
    let (began, before) = (std::time::Instant::now(), d.round_trips());
    while began.elapsed() < ms(600) {
        s = d.screen_after_change(s, ms(200)).unwrap();
    }
    let looks = d.round_trips() - before;
    assert!(shown(&s).starts_with("tick"), "{}", shown(&s));
    // No more often than every 30 ms (and, off the CI Mac, whose round trips are slow, about that).
    let least = if cfg!(target_os = "macos") { 1 } else { 8 };
    assert!((least..=22).contains(&looks), "{looks} looks in 600 ms");
}

#[test]
fn a_waits_look_ends_with_a_dialog_the_page_opens_or_a_page_it_goes_to() {
    let ms = std::time::Duration::from_millis;
    let Some(mut d) = opened("drag.html") else { return };
    let s = d.screen().unwrap();
    let mut s = d.screen_after_change(s, ms(200)).unwrap();
    d.evaluate("void setTimeout(() => alert('Saved!'), 40)").unwrap();
    let began = std::time::Instant::now();
    while !s.elements.iter().any(|e| e.text == "Saved!") {
        assert!(began.elapsed() < ms(3000), "the alert was never seen");
        s = d.screen_after_change(s, ms(200)).unwrap();
    }
    let (x, y) = centre_of_text(&mut d, "OK");
    d.tap(x, y).unwrap();
    // A page gone to another while a look waits in it: the look is of the new one.
    let url = serve_fixtures();
    let mut s = d.screen().unwrap();
    d.evaluate(&format!("void setTimeout(() => {{ location.href = '{url}/form.html'; }}, 40)")).unwrap();
    let began = std::time::Instant::now();
    while s.elements.iter().any(|e| e.id == "knob") {
        assert!(began.elapsed() < ms(5000), "the new page was never seen");
        s = d.screen_after_change(s, ms(200)).unwrap_or(s_empty());
    }
}

fn s_empty() -> virtuoso::screen::ScreenTree {
    virtuoso::screen::ScreenTree::new(Vec::new(), 0, 0)
}

#[test]
fn a_swipe_on_a_young_page_begins_at_once_and_ends_when_the_page_is_as_old_as_the_swipe_is_long() {
    // A page not yet as old as the swipe is long was first given the rest of that, doing
    // nothing, and only then its press, its moves and its lift, each waited for (kanban_board's
    // first swipe: 650-756 ms on a macOS runner against 113-372 for its second). The events
    // whose times have come go at once and the rest as theirs come: the press and the first
    // moves are handled while the page ages, and none is dated before its page or ahead of it.
    use virtuoso::device::Lift;
    // Chrome's: a Firefox swipe is not backdated (its drags go through BiDi input actions).
    if on_firefox() {
        return;
    }
    let Some(mut d) = opened("drag.html") else { return };
    d.evaluate("knob.addEventListener('pointerdown', (e) => { window.at = { down: performance.now(), downStamp: e.timeStamp }; }); knob.addEventListener('pointerup', (e) => { at.up = performance.now(); at.upStamp = e.timeStamp; })").unwrap();
    let young = num(&mut d, "performance.now()");
    // The swipe longer than the page is old by 500 ms or more, whatever the runner: a busy one
    // opened the page in 3.1 s (windows-latest), older than a 2 s swipe.
    let ms = (young + 500.0).max(2000.0).ceil();
    d.swipe((60, 60), (260, 160), ms as u64, Lift::Fling).unwrap();
    d.settle(virtuoso::web::web_quiet(false), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Knob at 220,120");
    let at = d.evaluate("at").unwrap();
    let n = |k: &str| at[k].as_f64().unwrap_or(f64::NAN);
    assert!(n("down") < young + 500.0, "the press reached the page at {} ms, the page {young} ms old when the swipe was asked for", n("down"));
    assert!(n("downStamp") >= 0.0 && n("downStamp") <= n("down") + 1.0, "the press is dated {} and was handled at {}", n("downStamp"), n("down"));
    assert!(n("upStamp") <= n("up") + 1.0, "the lift is dated {} and was handled at {}", n("upStamp"), n("up"));
    let lasted = n("upStamp") - n("downStamp");
    assert!((ms - 10.0..=ms + 10.0).contains(&lasted), "the page saw a swipe of {lasted} ms, of {ms}");
    // No sooner done than a swipe that long can be on a page that young, and little later.
    assert!(n("up") >= ms && n("up") < ms + 600.0, "the lift reached the page at {} ms, the swipe {ms} ms", n("up"));
    assert!(num(&mut d, "seen.moves") >= 2.0, "moves whose times came apart reach the page apart");
}

#[test]
fn a_swipe_on_a_drag_target_sent_one_event_at_a_time_still_drags_it() {
    // Chrome on an Android device takes its events one by one, a frame after each group (no
    // batched sends): the same swipe, to the page.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("drag.html") else { return };
    d.set_batched_drags(false);
    let (x, y) = centre(&mut d, "knob");
    d.swipe((x, y), (x + 200, y + 100), 400, Lift::Fling).unwrap();
    d.settle(virtuoso::web::web_quiet(false), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Knob at 220,120");
    assert!(num(&mut d, "seen.moves") >= 1.0, "its moves reach the page");
}

#[test]
fn only_a_page_a_drag_target_was_swiped_on_counts_its_pointer_events() {
    // A listener on every pointer move hands each of a finger's moves to the page's main thread:
    // on a slow machine every swipe answers later. Counted only where a compressed swipe needs
    // the count.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    d.swipe((300, 500), (300, 300), 300, Lift::Fling).unwrap();
    assert_eq!(d.evaluate("!!__virtuoso.counting").unwrap(), serde_json::json!(false), "a scroll's swipe");
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    d.swipe((x, y), (x + 200, y + 100), 400, Lift::Fling).unwrap();
    // (Firefox swipes a drag target in real time: nothing to count.)
    assert_eq!(d.evaluate("!!__virtuoso.counting").unwrap(), serde_json::json!(!on_firefox()), "a drag target's swipe");
}

#[test]
fn a_swipe_on_what_scrolls_still_takes_its_time() {
    // A scroller's fling follows from how fast the finger really moved.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("list.html") else { return };
    let began = std::time::Instant::now();
    d.swipe((300, 500), (300, 300), 300, Lift::Fling).unwrap();
    assert!(began.elapsed() >= std::time::Duration::from_millis(300), "took {:?}", began.elapsed());
}

#[test]
fn a_swipe_on_something_that_takes_pointer_drags_drags_it_as_maestro_does() {
    // Maestro's web swipe is a finger's: it drags what takes pointer drags (touch-action: none)
    // and scrolls the rest.
    use virtuoso::device::Lift;
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    d.swipe((x, y), (x + 200, y + 100), 400, Lift::Still).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(out(&mut d), "Knob at 220,120");
}

#[test]
fn back_on_a_tab_with_no_history_closes_it_and_comes_back_as_mobile_chrome_does() {
    use virtuoso::device::Key;
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "popup");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(10)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Second tab".to_string()));
    d.press_key(Key::Back).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(10)).unwrap();
    assert!(texts(&d.screen().unwrap()).contains(&"Nothing yet".to_string()), "back on the first tab");
}

#[test]
fn back_on_a_tab_with_history_goes_back_in_it_in_one_wait() {
    use virtuoso::device::Key;
    let Some(mut d) = opened("dialogs.html") else { return };
    let (x, y) = centre(&mut d, "popup");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(150), std::time::Duration::from_secs(10)).unwrap();
    d.evaluate("location.hash = 'on'").unwrap();
    wait_for(&mut d, "history.length === 2");
    let before = d.round_trips();
    d.press_key(Key::Back).unwrap();
    assert_eq!(d.round_trips() - before, 1);
    wait_for(&mut d, "location.hash === ''");
    assert!(texts(&d.screen().unwrap()).contains(&"Second tab".to_string()), "still the second tab");
}

const BOXES: &str = r#"<!doctype html><title>Boxes</title>
<input id="b0" maxlength="1"><input id="b1" maxlength="1"><input id="b2" maxlength="1"><p id="keys"></p>
<script>
const b = [0, 1, 2].map((i) => document.getElementById('b' + i));
b.forEach((el, i) => el.addEventListener('input', () => { if (el.value && b[i + 1]) b[i + 1].focus(); }));
addEventListener('keydown', (e) => document.getElementById('keys').textContent += e.key);
</script>"#;

#[test]
fn text_is_typed_a_key_at_a_time_as_a_keyboard_types_it() {
    let Some(mut d) = device() else { return };
    launch(&mut d, &serve(BOXES), false);
    let (x, y) = centre(&mut d, "b0");
    d.tap(x, y).unwrap();
    d.input_text("123").unwrap();
    assert_eq!(d.evaluate("b.map((el) => el.value).join(',')").unwrap(), json!("1,2,3"), "focus moved with each key");
    assert_eq!(d.evaluate("document.getElementById('keys').textContent").unwrap(), json!("123"), "each a key the page heard");
}

#[test]
fn the_screens_size_is_the_size_a_read_of_it_has_on_a_desktop_and_a_phone() {
    // Asked of the page alone (for where a point goes), not by reading every element.
    for phone in [false, true] {
        let Some(mut d) = device() else { return };
        if phone {
            d.emulate(virtuoso::web::emulation("pixel-7").unwrap()).unwrap();
        }
        launch(&mut d, &serve(INPUT), false);
        let read = d.screen().unwrap().screen_rect();
        assert_eq!(d.screen_rect().unwrap(), read, "phone: {phone}");
    }
    // Under a dialog (the page runs no script until it is answered): the last read's.
    let Some(mut d) = opened("dialogs.html") else { return };
    let read = d.screen().unwrap().screen_rect();
    let (x, y) = centre(&mut d, "alert");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(100), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(d.screen_rect().unwrap(), read, "under a dialog");
}

#[test]
fn an_emulated_phone_has_its_size_touch_and_user_agent_in_every_tab() {
    // Firefox as a phone (its touch, its pinch zoom) comes with its own pull request.
    if on_firefox() {
        return;
    }
    let Some(mut d) = device() else { return };
    d.emulate(virtuoso::web::emulation("pixel-7").unwrap()).unwrap();
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/dialogs.html"), false);
    let check = |d: &mut WebDevice, tab: &str| {
        assert_eq!((num(d, "innerWidth"), num(d, "innerHeight")), (412.0, 839.0), "{tab}");
        assert!(d.evaluate("navigator.userAgent").unwrap().as_str().unwrap().contains("Pixel 7"), "{tab}");
        assert!(num(d, "navigator.maxTouchPoints") > 0.0, "{tab}: touch");
        assert_eq!(d.evaluate("matchMedia('(pointer: coarse)').matches").unwrap(), json!(true), "{tab}: a finger, not a mouse");
    };
    check(&mut d, "first tab");
    let (x, y) = centre(&mut d, "newtab");
    d.tap(x, y).unwrap();
    d.settle(std::time::Duration::from_millis(200), std::time::Duration::from_secs(10)).unwrap();
    assert!(d.screen().unwrap().elements.iter().any(|e| e.text == "Second tab"));
    check(&mut d, "a tab the page opened");
}

#[test]
fn a_launched_chrome_has_a_desktop_window_of_1280_by_800() {
    let Some(mut d) = opened("form.html") else { return };
    assert_eq!((num(&mut d, "outerWidth"), num(&mut d, "outerHeight")), (1280.0, 800.0));
}

/// The size of the window the page's tab is in, as Chrome has it (not as the page is told).
fn window_size(d: &mut WebDevice) -> (i64, i64) {
    let w = d.devtools("Browser.getWindowForTarget", json!({})).unwrap();
    (w["bounds"]["width"].as_i64().unwrap(), w["bounds"]["height"].as_i64().unwrap())
}

#[test]
fn a_chrome_launched_as_a_phone_has_a_window_the_phones_size_and_the_page_its_screen() {
    // Headless, a desktop window behind a phone's screen is drawn for nothing: the window is the
    // phone's size. The page sees the phone emulated, exactly as in a desktop's window.
    let Some(path) = chrome() else { return };
    let phone = virtuoso::web::emulation("pixel-7").unwrap();
    let url = serve_fixtures();
    let mut seen = Vec::new();
    for as_phone in [true, false] {
        let mut d = if as_phone {
            WebDevice::launch_as(&path, true, Some(phone.clone())).unwrap()
        } else {
            let mut d = WebDevice::launch(&path, true).unwrap();
            d.emulate(phone.clone()).unwrap();
            d
        };
        launch(&mut d, &format!("{url}/form.html"), false);
        let window = window_size(&mut d);
        // The phone's size, or as near as Chrome goes (on Windows, a window is 516 wide at least).
        if as_phone {
            assert!(window.0 >= 412 && window.0 <= 516 && window.1 == 839, "{window:?}");
        } else {
            assert_eq!(window, (1280, 800));
        }
        let page = d.evaluate("[innerWidth, innerHeight, devicePixelRatio, screen.width, screen.height, matchMedia('(max-width: 412px)').matches, matchMedia('(pointer: coarse)').matches, navigator.userAgent]").unwrap();
        seen.push(page);
    }
    assert_eq!(seen[0], seen[1], "the page sees the same phone in either window");
    // The phone's screen (the page, with no viewport of its own, lays out 980 wide, as a phone's).
    assert_eq!((&seen[0][2], &seen[0][3], &seen[0][4]), (&json!(2.625), &json!(412), &json!(839)));
}

/// Chrome on the Android device VIRTUOSO_WEB_ANDROID names (a serial), else no test.
fn android_chrome() -> Option<WebDevice> {
    let serial = std::env::var("VIRTUOSO_WEB_ANDROID").ok().filter(|s| !s.is_empty())?;
    Some(virtuoso::web::connect(&format!("android:{serial}")).unwrap())
}

#[test]
fn chrome_on_an_android_device_opens_a_page_this_machine_serves_and_takes_taps() {
    let Some(mut d) = android_chrome() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/index.html"), true);
    assert!(d.evaluate("navigator.userAgent").unwrap().as_str().unwrap().contains("Android"), "the device's own Chrome");
    // A tap on the link (by touch on the phone's own screen) opens the form.
    let (x, y) = centre(&mut d, "nav-form");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "location.pathname.endsWith('/form.html')");
}

#[test]
fn the_part_of_the_page_the_phones_keyboard_covers_is_not_on_screen() {
    let Some(mut d) = android_chrome() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/form.html"), true);
    let full = d.screen().unwrap().height;
    let (x, y) = centre(&mut d, "name");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "visualViewport.height < innerHeight - 100");
    let s = d.screen().unwrap();
    assert!(s.height < full - 100, "the keyboard's part is off the screen: {} of {full}", s.height);
}

#[test]
fn on_a_phone_a_tap_after_typing_is_a_click_on_its_target() {
    // A phone's tap is a finger's: a mouse press there takes the typed field's focus, the page
    // shifts back from it, and the release and its click fall elsewhere. And Chrome makes the
    // click of a touch a little after it: a tap is done once the page has had it, or what the
    // flow does next comes first and the click is lost.
    let Ok(serial) = std::env::var("VIRTUOSO_WEB_ANDROID") else { return };
    if serial.is_empty() {
        return;
    }
    let url = serve_fixtures();
    let (code, out) = run_flow_on(
        Some(&format!("android:{serial}")),
        &format!(
            "url: {url}/focusshift.html\n---\n- launchApp:\n    clearState: true\n- tapOn: Full name\n- pressKey: Tab\n- inputText: Maria Novak\n- pressKey: Tab\n- inputText: maria.novak@example.com\n- pressKey: Tab\n- inputText: +48 601 222 333\n- tapOn: Next\n- assertVisible: Pressed\n"
        ),
    );
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn with_the_visible_part_of_the_page_moved_a_tap_lands_where_the_screen_says() {
    // Firefox as a phone (its touch, its pinch zoom) comes with its own pull request.
    if on_firefox() {
        return;
    }
    // A phone's keyboard moves what shows of the page (the visual viewport) to keep its field in
    // sight; a pinch zoom moves it the same way, on any Chrome.
    let Some(mut d) = device() else { return };
    let url = serve_fixtures();
    launch(&mut d, &format!("{url}/keyboard.html"), true);
    d.devtools("Input.synthesizePinchGesture", json!({"x": 640, "y": 700, "scaleFactor": 2.0, "relativeSpeed": 800})).unwrap();
    wait_for(&mut d, "visualViewport.offsetTop > 50");
    let (x, y) = centre(&mut d, "go");
    d.tap(x, y).unwrap();
    wait_for(&mut d, "document.getElementById('status').textContent === 'Pressed'");
}

#[test]
fn zz_probe_counts() {
    use virtuoso::device::Lift;
    let Some(mut d) = opened("drag.html") else { return };
    let (x, y) = centre(&mut d, "knob");
    std::thread::sleep(std::time::Duration::from_millis(500));
    let t = std::time::Instant::now();
    d.evaluate("window.tl=[]; for (const e of ['touchstart','pointerdown','pointermove','pointerup','touchend']) addEventListener(e, ()=>tl.push(e+'@'+Math.round(performance.now())), true); const f=()=>{tl.push('raf@'+Math.round(performance.now())); if(tl.length<40) requestAnimationFrame(f)}; requestAnimationFrame(f); tl.push('start@'+Math.round(performance.now()))").unwrap();
    d.swipe((x, y), (x + 200, y + 100), 400, Lift::Fling).unwrap();
    eprintln!("PROBE took {:?} counts {} tl {}", t.elapsed(), d.evaluate("JSON.stringify([__virtuoso.downs, __virtuoso.moves, __virtuoso.from, seen])").unwrap(), d.evaluate("tl.join(' ')").unwrap());
}

#[test]
fn zz_probe_handled() {
    let Some(mut d) = opened("drag.html") else { return };
    std::thread::sleep(std::time::Duration::from_millis(300));
    for e in ["__virtuoso.handled('downs', 0, 200)", "new Promise(r => __virtuoso.after(() => __virtuoso.after(r)))", "new Promise(r => setTimeout(r, 1))", "1"] {
        let t = std::time::Instant::now();
        d.evaluate(e).unwrap();
        eprintln!("PROBE {e}: {:?}", t.elapsed());
    }
}
