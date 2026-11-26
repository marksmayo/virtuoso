//! Safari over safaridriver: W3C WebDriver classic, HTTP and JSON on a local port.
//! safaridriver ships with macOS (`/usr/bin/safaridriver`), allows one session at a time, and
//! needs `safaridriver --enable` once (with sudo) before it drives anything.

use crate::classic::{Driver, WdError};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// safaridriver, or `VIRTUOSO_SAFARIDRIVER`.
pub fn find_safaridriver() -> PathBuf {
    safaridriver_at(std::env::var("VIRTUOSO_SAFARIDRIVER").ok())
}

/// safaridriver at the path `VIRTUOSO_SAFARIDRIVER` names, or the system's.
pub fn safaridriver_at(named: Option<String>) -> PathBuf {
    PathBuf::from(named.unwrap_or_else(|| "/usr/bin/safaridriver".into()))
}

/// A port nothing listens on now (on 127.0.0.1).
pub fn free_port() -> Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// The request text for `method` on `path` at `addr`, with a JSON body if there is one.
pub fn request_text(method: &str, path: &str, addr: SocketAddr, body: Option<&Value>) -> String {
    let mut text = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    match body {
        Some(b) => {
            let b = b.to_string();
            text.push_str(&format!("Content-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{b}", b.len()));
        }
        None => text.push_str("\r\n"),
    }
    text
}

/// One line, without its line end: an error at the end of the input.
fn line(r: &mut dyn BufRead) -> Result<String> {
    let mut l = String::new();
    if r.read_line(&mut l)? == 0 {
        bail!("the HTTP response ended early");
    }
    Ok(l.trim_end_matches(['\r', '\n']).to_string())
}

/// Reads one HTTP/1.1 response: its status and body.
pub fn read_response(r: &mut dyn BufRead) -> Result<(u16, String)> {
    let first = line(r)?;
    let status = first
        .strip_prefix("HTTP/1.")
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("not an HTTP response: {first}"))?;
    let (mut length, mut chunked) = (None, false);
    loop {
        let l = line(r)?;
        if l.is_empty() {
            break;
        }
        let Some((name, value)) = l.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "content-length" {
            length = Some(value.parse::<usize>().with_context(|| format!("a Content-Length of {value}"))?);
        } else if name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked") {
            chunked = true;
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let size = line(r)?;
            let size = size.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size, 16).with_context(|| format!("a chunk size of {size}"))?;
            if size == 0 {
                // The trailers, if any, to the blank line (or the end).
                while matches!(line(r).as_deref(), Ok(l) if !l.is_empty()) {}
                break;
            }
            let at = body.len();
            body.resize(at + size, 0);
            r.read_exact(&mut body[at..])?;
            line(r)?;
        }
    } else if let Some(n) = length {
        body.resize(n, 0);
        r.read_exact(&mut body).context("the HTTP response's body ended early")?;
    } else {
        r.read_to_end(&mut body)?;
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// One request to a local HTTP server, on a connection of its own: its status and body.
pub fn http(addr: SocketAddr, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<(u16, String)> {
    let mut s = TcpStream::connect_timeout(&addr, timeout).with_context(|| format!("nothing answers on {addr}"))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(request_text(method, path, addr, body).as_bytes())?;
    read_response(&mut BufReader::new(s)).with_context(|| format!("{method} {path} on {addr}"))
}

/// A WebDriver reply's `value`, or its error (`{"value":{"error":…,"message":…}}`) as one.
pub fn webdriver_value(status: u16, body: &str) -> Result<Value> {
    webdriver_reply(status, body).map_err(|e| if e.error == NOT_WEBDRIVER { anyhow!("{}", e.message) } else { anyhow!("safaridriver: {e}") })
}

/// The code an answer that is not WebDriver's is given.
const NOT_WEBDRIVER: &str = "unknown error";

/// As `webdriver_value`, its error kept as WebDriver's code and message.
pub fn webdriver_reply(status: u16, body: &str) -> Result<Value, WdError> {
    let not_webdriver = || WdError::new(NOT_WEBDRIVER, &format!("safaridriver answered {status}: {body}"));
    let mut reply: Value = serde_json::from_str(body).map_err(|_| not_webdriver())?;
    let value = reply.get_mut("value").map(Value::take).ok_or_else(not_webdriver)?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(WdError::new(error, value.get("message").and_then(Value::as_str).unwrap_or_default()));
    }
    if !(200..300).contains(&status) {
        return Err(not_webdriver());
    }
    Ok(value)
}

/// The session's commands, for the classic transport (`classic`).
impl Driver for WebDriver {
    fn command(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, WdError> {
        let path = self.in_session(path).map_err(|e| WdError::new("invalid session id", &e.to_string()))?;
        let (status, reply) = http(self.addr, method, &path, body, self.timeout).map_err(|e| WdError::new(NOT_WEBDRIVER, &format!("{e:#}")))?;
        webdriver_reply(status, &reply)
    }
}

/// What `POST /session` is sent for these capabilities.
pub fn new_session_body(always_match: Value) -> Value {
    json!({"capabilities": {"alwaysMatch": always_match}})
}

/// Safari on the Mac: dialogs left open for the flow to answer (a session dismisses them by
/// default), and a script given as long as a settle's wait in the page can take.
pub fn desktop_capabilities() -> Value {
    json!({"browserName": "safari", "unhandledPromptBehavior": "ignore", "timeouts": {"script": 120_000}})
}

/// Safari in the iOS Simulator with this UDID (safaridriver's own capabilities: see
/// `man safaridriver`).
pub fn ios_simulator_capabilities(udid: &str) -> Value {
    json!({"browserName": "safari", "platformName": "iOS", "safari:useSimulator": true, "safari:deviceUDID": udid})
}

/// A WebDriver server's client, and the session it has open, if any.
pub struct WebDriver {
    pub addr: SocketAddr,
    pub session: Option<String>,
    /// How long a command may take (a navigation waits for the page's load; a simulator's
    /// session for Safari to start in it).
    pub timeout: Duration,
}

impl WebDriver {
    pub fn new(addr: SocketAddr) -> WebDriver {
        WebDriver { addr, session: None, timeout: Duration::from_secs(120) }
    }

    /// One command: its reply's value.
    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let (status, reply) = http(self.addr, method, path, body, self.timeout)?;
        webdriver_value(status, &reply)
    }

    /// The open session's path with `rest` after it.
    fn in_session(&self, rest: &str) -> Result<String> {
        let id = self.session.as_deref().ok_or_else(|| anyhow!("no Safari session is open"))?;
        Ok(format!("/session/{id}{rest}"))
    }

    /// Waits up to `timeout` for the server to say it is ready (`GET /status`).
    pub fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let end = Instant::now() + timeout;
        let mut last = String::from("nothing");
        while Instant::now() < end {
            match http(self.addr, "GET", "/status", None, Duration::from_secs(2)).and_then(|(s, b)| webdriver_value(s, &b)) {
                Ok(v) if v["ready"] == true => return Ok(()),
                Ok(v) => last = v.to_string(),
                Err(e) => last = format!("{e:#}"),
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        bail!("safaridriver on {} did not answer ready within {timeout:?} (last: {last})", self.addr)
    }

    /// Opens a session with these capabilities: the capabilities the server gave it.
    pub fn new_session(&mut self, always_match: Value) -> Result<Value> {
        let mut v = self.call("POST", "/session", Some(&new_session_body(always_match)))?;
        let id = v["sessionId"].as_str().ok_or_else(|| anyhow!("safaridriver opened a session with no id: {v}"))?;
        self.session = Some(id.to_string());
        Ok(v["capabilities"].take())
    }

    pub fn navigate(&self, url: &str) -> Result<()> {
        self.call("POST", &self.in_session("/url")?, Some(&json!({"url": url}))).map(drop)
    }

    /// Runs `script` (a function body) with `args`: what it returns.
    pub fn execute(&self, script: &str, args: Value) -> Result<Value> {
        self.call("POST", &self.in_session("/execute/sync")?, Some(&json!({"script": script, "args": args})))
    }

    /// Runs `script` with `args` and a callback as its last argument: what is passed to it.
    pub fn execute_async(&self, script: &str, args: Value) -> Result<Value> {
        self.call("POST", &self.in_session("/execute/async")?, Some(&json!({"script": script, "args": args})))
    }

    /// Ends the session, if one is open.
    pub fn delete_session(&mut self) -> Result<()> {
        let Some(id) = self.session.take() else { return Ok(()) };
        self.call("DELETE", &format!("/session/{id}"), None).map(drop)
    }
}

/// A Safari being driven, with its session: the session deleted and safaridriver stopped when
/// dropped.
pub struct Safari {
    pub driver: WebDriver,
    /// What the session was given.
    pub capabilities: Value,
    process: Option<Child>,
}

/// How long a launch waits for safaridriver to answer.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

impl Safari {
    /// safaridriver on a free port, with a Safari session on the Mac.
    pub fn launch() -> Result<Safari> {
        Safari::launch_with(&find_safaridriver(), desktop_capabilities())
    }

    /// safaridriver at `path` on a free port, with a session for these capabilities.
    pub fn launch_with(path: &Path, always_match: Value) -> Result<Safari> {
        Safari::start(path, always_match, READY_TIMEOUT)
    }

    /// `launch_with`, waiting up to `ready` for safaridriver to answer.
    pub fn start(path: &Path, always_match: Value, ready: Duration) -> Result<Safari> {
        let port = free_port()?;
        let child = Command::new(path)
            .args(["--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .with_context(|| format!("could not start safaridriver at {} (macOS only; `sudo safaridriver --enable` once)", path.display()))?;
        Safari::over(Some(child), SocketAddr::from(([127, 0, 0, 1], port)), always_match, ready)
    }

    /// A session for these capabilities on the server at `addr`, which `process` (if any) is.
    pub fn over(process: Option<Child>, addr: SocketAddr, always_match: Value, ready: Duration) -> Result<Safari> {
        // From here a failure drops the Safari, which stops the process.
        let mut s = Safari { driver: WebDriver::new(addr), capabilities: Value::Null, process };
        s.driver.wait_ready(ready)?;
        s.capabilities = s.driver.new_session(always_match)?;
        Ok(s)
    }
}

impl Drop for Safari {
    fn drop(&mut self) {
        // One session at a time: a session left open keeps the next launch from opening one.
        let _ = self.driver.delete_session();
        if let Some(mut c) = self.process.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A stand-in server: answers each connection with the next of `replies`, and sends each
    /// request's text (head and body) down the channel.
    fn stand_in(replies: Vec<String>) -> (SocketAddr, mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((s, _)) = l.accept() else { return };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut head = String::new();
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let _ = tx.send(head + &String::from_utf8(body).unwrap());
                let mut s = s;
                let _ = s.write_all(reply.as_bytes());
            }
        });
        (addr, rx)
    }

    fn ok(value: Value) -> String {
        let body = json!({ "value": value }).to_string();
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn error(status: u16, error: &str, message: &str) -> String {
        let body = json!({"value": {"error": error, "message": message, "stacktrace": ""}}).to_string();
        format!("HTTP/1.1 {status} Error\r\nContent-Length: {}\r\n\r\n{body}", body.len())
    }

    fn addr() -> SocketAddr {
        "127.0.0.1:4444".parse().unwrap()
    }

    #[test]
    fn safaridriver_is_the_systems_unless_named() {
        // The variable is read at the call: neither set nor cleared here (tests run in parallel).
        let found = find_safaridriver();
        match std::env::var("VIRTUOSO_SAFARIDRIVER") {
            Ok(p) => assert_eq!(found, PathBuf::from(p)),
            Err(_) => assert_eq!(found, PathBuf::from("/usr/bin/safaridriver")),
        }
        assert_eq!(safaridriver_at(Some("/opt/sd".into())), PathBuf::from("/opt/sd"));
        assert_eq!(safaridriver_at(None), PathBuf::from("/usr/bin/safaridriver"));
    }

    #[test]
    fn a_request_without_a_body_is_a_head_alone() {
        assert_eq!(request_text("GET", "/status", addr(), None), "GET /status HTTP/1.1\r\nHost: 127.0.0.1:4444\r\nConnection: close\r\n\r\n");
    }

    #[test]
    fn a_request_with_a_body_says_its_type_and_length() {
        let text = request_text("POST", "/session/a/url", addr(), Some(&json!({"url": "http://x/é"})));
        let body = r#"{"url":"http://x/é"}"#;
        assert_eq!(
            text,
            format!("POST /session/a/url HTTP/1.1\r\nHost: 127.0.0.1:4444\r\nConnection: close\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}", body.len())
        );
    }

    #[test]
    fn a_response_is_read_to_its_length() {
        let mut r = "HTTP/1.1 200 OK\r\ncontent-length: 5\r\nX: y\r\n\r\nhelloEXTRA".as_bytes();
        assert_eq!(read_response(&mut r).unwrap(), (200, "hello".to_string()));
    }

    #[test]
    fn a_response_without_a_length_is_read_to_its_end() {
        let mut r = "HTTP/1.1 404 Not Found\r\n\r\nno such thing".as_bytes();
        assert_eq!(read_response(&mut r).unwrap(), (404, "no such thing".to_string()));
    }

    #[test]
    fn a_chunked_response_is_put_back_together() {
        let mut r = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\nA;x=1\r\n0123456789\r\n0\r\n\r\n".as_bytes();
        assert_eq!(read_response(&mut r).unwrap(), (200, "abc0123456789".to_string()));
    }

    #[test]
    fn what_is_not_http_is_an_error() {
        assert!(read_response(&mut "".as_bytes()).is_err());
        assert!(read_response(&mut "SSH-2.0-OpenSSH\r\n\r\n".as_bytes()).is_err());
        assert!(read_response(&mut "HTTP/1.1 abc\r\n\r\n".as_bytes()).is_err());
        assert!(read_response(&mut "HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort".as_bytes()).is_err());
        assert!(read_response(&mut "HTTP/1.1 200 OK\r\nContent-Length: x\r\n\r\n".as_bytes()).is_err());
        assert!(read_response(&mut "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n".as_bytes()).is_err());
        assert!(read_response(&mut "HTTP/1.1 200 OK\r\nX: y".as_bytes()).is_err());
    }

    #[test]
    fn a_request_goes_to_the_server_and_its_answer_comes_back() {
        let (a, rx) = stand_in(vec![ok(json!({"ready": true}))]);
        let (status, body) = http(a, "POST", "/x", Some(&json!({"k": 1})), Duration::from_secs(5)).unwrap();
        assert_eq!(status, 200);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"value": {"ready": true}}));
        let sent = rx.recv().unwrap();
        assert!(sent.starts_with("POST /x HTTP/1.1\r\n"), "{sent}");
        assert!(sent.ends_with("\r\n\r\n{\"k\":1}"), "{sent}");
    }

    #[test]
    fn no_server_is_an_error() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        assert!(http(a, "GET", "/status", None, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn a_replys_value_is_its_result() {
        assert_eq!(webdriver_value(200, r#"{"value":"Title"}"#).unwrap(), json!("Title"));
        assert_eq!(webdriver_value(200, r#"{"value":null}"#).unwrap(), Value::Null);
    }

    #[test]
    fn a_webdriver_error_is_named_with_its_message() {
        let e = webdriver_value(404, r#"{"value":{"error":"no such window","message":"The window was closed.","stacktrace":""}}"#).unwrap_err();
        assert_eq!(e.to_string(), "safaridriver: no such window: The window was closed.");
        let e = webdriver_value(500, r#"{"value":{"error":"session not created"}}"#).unwrap_err();
        assert_eq!(e.to_string(), "safaridriver: session not created");
    }

    #[test]
    fn a_reply_that_is_not_webdriver_json_is_an_error() {
        let e = webdriver_value(502, "Bad Gateway").unwrap_err();
        assert!(e.to_string().contains("502"), "{e}");
        assert!(e.to_string().contains("Bad Gateway"), "{e}");
        assert!(webdriver_value(200, r#"{"other":1}"#).is_err());
        // A failure status whose value is not an error is one still.
        assert!(webdriver_value(500, r#"{"value":"x"}"#).is_err());
    }

    #[test]
    fn a_session_asks_for_its_capabilities_as_ones_to_match() {
        assert_eq!(new_session_body(json!({"browserName": "safari"})), json!({"capabilities": {"alwaysMatch": {"browserName": "safari"}}}));
        assert_eq!(
            ios_simulator_capabilities("ABC-123"),
            json!({"browserName": "safari", "platformName": "iOS", "safari:useSimulator": true, "safari:deviceUDID": "ABC-123"})
        );
    }

    #[test]
    fn a_session_is_opened_driven_and_ended() {
        let (a, rx) = stand_in(vec![
            ok(json!({"sessionId": "S1", "capabilities": {"browserName": "Safari", "browserVersion": "26.1"}})),
            ok(Value::Null),
            ok(json!("Page")),
            ok(json!(42)),
            ok(Value::Null),
        ]);
        let mut d = WebDriver::new(a);
        assert_eq!(d.new_session(desktop_capabilities()).unwrap()["browserVersion"], "26.1");
        assert_eq!(d.session.as_deref(), Some("S1"));
        assert!(rx.recv().unwrap().starts_with("POST /session HTTP/1.1"));
        d.navigate("http://127.0.0.1:1/").unwrap();
        let sent = rx.recv().unwrap();
        assert!(sent.starts_with("POST /session/S1/url HTTP/1.1") && sent.ends_with(r#"{"url":"http://127.0.0.1:1/"}"#), "{sent}");
        assert_eq!(d.execute("return document.title", json!([])).unwrap(), json!("Page"));
        let sent = rx.recv().unwrap();
        assert!(sent.starts_with("POST /session/S1/execute/sync HTTP/1.1") && sent.ends_with(r#"{"args":[],"script":"return document.title"}"#), "{sent}");
        assert_eq!(d.execute_async("arguments[1](arguments[0])", json!([42])).unwrap(), json!(42));
        assert!(rx.recv().unwrap().starts_with("POST /session/S1/execute/async HTTP/1.1"));
        d.delete_session().unwrap();
        assert!(rx.recv().unwrap().starts_with("DELETE /session/S1 HTTP/1.1"));
        assert_eq!(d.session, None);
        // Once ended, there is nothing to end, and nothing to drive.
        d.delete_session().unwrap();
        assert!(d.navigate("http://x/").is_err());
    }

    #[test]
    fn a_sessions_commands_are_the_classic_transports_and_its_errors_keep_their_code() {
        use crate::classic::{Driver, WdError};
        let (a, rx) = stand_in(vec![ok(json!({"sessionId": "S2", "capabilities": {}})), ok(json!(["w1"])), error(400, "unexpected alert open", ""), "garbage".to_string()]);
        let mut d = WebDriver::new(a);
        d.new_session(desktop_capabilities()).unwrap();
        rx.recv().unwrap();
        assert_eq!(Driver::command(&mut d, "GET", "/window/handles", None), Ok(json!(["w1"])));
        assert!(rx.recv().unwrap().starts_with("GET /session/S2/window/handles HTTP/1.1"));
        assert_eq!(Driver::command(&mut d, "POST", "/execute/sync", Some(&json!({}))), Err(WdError::new("unexpected alert open", "")));
        // What is not WebDriver's answer is an error of its own.
        let e = Driver::command(&mut d, "GET", "/url", None).unwrap_err();
        assert_eq!(e.error, "unknown error");
        // As is a session that is not open.
        d.session = None;
        assert_eq!(Driver::command(&mut d, "GET", "/url", None).unwrap_err().error, "invalid session id");
    }

    #[test]
    fn a_desktop_safari_leaves_dialogs_open_and_lets_scripts_run_long() {
        // Dialogs left open for the flow to answer (a session dismisses them by default); a
        // settle's wait in the page is a script up to its stretch long.
        let c = desktop_capabilities();
        assert_eq!(c["unhandledPromptBehavior"], json!("ignore"));
        assert!(c["timeouts"]["script"].as_u64().unwrap() >= 60_000);
    }

    #[test]
    fn a_session_the_server_refuses_is_its_error() {
        let (a, _rx) = stand_in(vec![error(500, "session not created", "Could not create a session: The Safari instance is already paired with another WebDriver session.")]);
        let mut d = WebDriver::new(a);
        let e = d.new_session(desktop_capabilities()).unwrap_err();
        assert!(e.to_string().contains("already paired"), "{e}");
        assert_eq!(d.session, None);
    }

    #[test]
    fn a_session_with_no_id_is_an_error() {
        let (a, _rx) = stand_in(vec![ok(json!({"capabilities": {}}))]);
        assert!(WebDriver::new(a).new_session(desktop_capabilities()).is_err());
    }

    #[test]
    fn readiness_is_waited_for() {
        // Not ready, then ready.
        let (a, _rx) = stand_in(vec![ok(json!({"ready": false, "message": "busy"})), ok(json!({"ready": true}))]);
        WebDriver::new(a).wait_ready(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn a_server_that_never_answers_is_given_up_on() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        let t = Instant::now();
        let e = WebDriver::new(a).wait_ready(Duration::from_millis(300)).unwrap_err();
        assert!(e.to_string().contains("did not answer"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn safari_over_a_server_opens_a_session_and_ends_it_when_dropped() {
        let (a, rx) = stand_in(vec![ok(json!({"ready": true})), ok(json!({"sessionId": "S9", "capabilities": {"browserName": "Safari"}})), ok(Value::Null)]);
        let s = Safari::over(None, a, desktop_capabilities(), Duration::from_secs(5)).unwrap();
        assert_eq!(s.capabilities["browserName"], "Safari");
        assert!(rx.recv().unwrap().starts_with("GET /status "));
        assert!(rx.recv().unwrap().starts_with("POST /session "));
        drop(s);
        assert!(rx.recv().unwrap().starts_with("DELETE /session/S9 "));
    }

    #[test]
    fn a_driver_that_is_not_there_does_not_launch() {
        let e = Safari::launch_with(Path::new("/no/such/safaridriver"), desktop_capabilities()).err().unwrap();
        assert!(e.to_string().contains("/no/such/safaridriver"), "{e}");
    }

    #[test]
    fn a_driver_that_never_answers_is_stopped() {
        // This test binary stands in for safaridriver: it does not know `--port`, and never listens.
        let me = std::env::current_exe().unwrap();
        let t = Instant::now();
        let e = Safari::start(&me, desktop_capabilities(), Duration::from_millis(300)).err().unwrap();
        assert!(e.to_string().contains("did not answer"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn safari_is_not_launched_where_there_is_no_safaridriver() {
        if std::env::var("VIRTUOSO_SAFARIDRIVER").is_err() {
            assert!(Safari::launch().is_err());
        }
    }

    #[test]
    fn a_free_port_is_one_nothing_listens_on() {
        let p = free_port().unwrap();
        assert_ne!(p, 0);
        TcpListener::bind(("127.0.0.1", p)).unwrap();
    }
}
