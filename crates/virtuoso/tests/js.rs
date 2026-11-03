//! `${...}` expressions and flow scripts.

use virtuoso::js::Js;

#[test]
fn text_without_expressions_is_unchanged() {
    let js = Js::new("android");
    assert_eq!(js.expand("plain text").unwrap(), "plain text");
}

#[test]
fn variables_expand_inside_text() {
    let js = Js::new("android");
    js.set_var("USER", "alice");
    assert_eq!(js.expand("hello ${USER}!").unwrap(), "hello alice!");
}

#[test]
fn expressions_are_javascript() {
    let js = Js::new("android");
    js.set_var("N", "4");
    assert_eq!(js.expand("${parseInt(N) * 2} items").unwrap(), "8 items");
    assert_eq!(js.expand("${'a' + 'b'}").unwrap(), "ab");
}

#[test]
fn braces_inside_an_expression_do_not_end_it() {
    let js = Js::new("android");
    assert_eq!(js.expand("${JSON.stringify({a: 1})}").unwrap(), "{\"a\":1}");
}

#[test]
fn output_persists_between_scripts() {
    let js = Js::new("android");
    js.run_script("output.token = 'abc'", &[]).unwrap();
    assert_eq!(js.expand("${output.token}").unwrap(), "abc");
}

#[test]
fn script_env_is_visible_to_that_script() {
    let js = Js::new("android");
    js.run_script("output.greeting = 'hi ' + NAME", &[("NAME".into(), "bob".into())]).unwrap();
    assert_eq!(js.expand("${output.greeting}").unwrap(), "hi bob");
}

#[test]
fn maestro_object_reports_platform_and_copied_text() {
    let js = Js::new("android");
    js.set_copied_text("clip");
    assert_eq!(js.expand("${maestro.platform}/${maestro.copiedText}").unwrap(), "android/clip");
}

#[test]
fn truthiness_follows_javascript() {
    let js = Js::new("android");
    assert!(js.truthy("1 + 1 == 2").unwrap());
    assert!(!js.truthy("''").unwrap());
    assert!(js.truthy("${true}").unwrap());
    assert!(!js.truthy("${false}").unwrap());
    assert!(!js.truthy("false").unwrap());
}

#[test]
fn a_script_error_is_reported_with_its_message() {
    let js = Js::new("android");
    let err = js.run_script("throw new Error('boom')", &[]).unwrap_err();
    assert!(err.to_string().contains("boom"), "{err}");
}

#[test]
fn console_log_is_collected() {
    let js = Js::new("android");
    js.run_script("console.log('one', 2)", &[]).unwrap();
    assert_eq!(js.take_logs(), vec!["one 2"]);
}

#[test]
fn json_helper_parses_text() {
    let js = Js::new("android");
    assert_eq!(js.expand("${json('{\"a\": 5}').a}").unwrap(), "5");
}

#[test]
fn faker_and_random_values_have_the_right_shape() {
    let js = Js::new("android");
    let email = js.expand("${faker.internet().emailAddress()}").unwrap();
    assert!(email.contains('@') && email.contains('.'), "{email}");
    let name = js.expand("${faker.name().fullName()}").unwrap();
    assert!(name.contains(' '), "{name}");
}

/// A one-request HTTP server: returns the raw request it received.
fn capture_one_request() -> (u16, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_millis(500))).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got);
            if let Some(head) = text.find("\r\n\r\n") {
                let len = text[..head].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                if got.len() >= head + 4 + len {
                    break;
                }
            }
        }
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        String::from_utf8_lossy(&got).to_string()
    });
    (port, handle)
}

#[test]
fn http_sends_a_multipart_form_with_fields_and_files() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.json");
    std::fs::write(&file, "{\"k\":1}").unwrap();
    let (port, server) = capture_one_request();
    let js = Js::new("android");
    js.set_var("FILE", &file.to_string_lossy().replace('\\', "/"));
    js.run_script(
        &format!("output.r = http.post('http://127.0.0.1:{port}/up', {{ multipartForm: {{ kind: 'import', data: {{ filePath: FILE, mediaType: 'application/json' }} }} }});"),
        &[],
    )
    .unwrap();
    assert_eq!(js.expand("${output.r.status}").unwrap(), "200");
    let req = server.join().unwrap();
    assert!(req.to_ascii_lowercase().contains("content-type: multipart/form-data"), "{req}");
    assert!(req.contains("name=\"kind\"") && req.contains("import"), "{req}");
    assert!(req.contains("name=\"data\"; filename=\"data.json\"") && req.contains("application/json") && req.contains("{\"k\":1}"), "{req}");
}

#[test]
fn the_engine_starts_only_when_a_flow_uses_javascript() {
    let js = Js::new("android");
    js.set_var("NAME", "Ada");
    js.set_copied_text("copied");
    assert_eq!(js.expand("plain text").unwrap(), "plain text");
    assert!(!js.started());
    assert_eq!(js.expand("${NAME} ${maestro.copiedText} ${maestro.platform}").unwrap(), "Ada copied android");
    assert!(js.started());
}

#[test]
fn scripts_get_the_built_ins_flows_use_and_not_the_rest() {
    // Proxy, typed arrays, BigInt, WeakRef and performance are left out of the engine (~70 KB).
    let js = Js::new("android");
    js.run_script(
        "var m = new Map([['a', 1]]); var s = new Set([1, 1, 2]);
         output.ok = m.get('a') === 1 && s.size === 2 && JSON.parse(JSON.stringify({x: [1]})).x[0] === 1
           && /^ab+c$/i.test('ABBC') && new Date(0).getTime() === 0 && eval('1 + 1') === 2
           && typeof Promise.resolve === 'function' && Math.max(1, 2) === 2",
        &[],
    )
    .unwrap();
    assert!(js.truthy("output.ok").unwrap());
    for gone in ["Proxy", "Uint8Array", "BigInt", "WeakRef"] {
        assert!(js.truthy(&format!("typeof {gone} === 'undefined'")).unwrap(), "{gone} is still there");
    }
}

#[test]
fn an_unclosed_expression_stays_as_text_as_in_maestro() {
    // A ` #` in an unquoted YAML value starts a comment, which can cut `${...}` short.
    let js = Js::new("android");
    assert_eq!(js.expand("Hi ${name").unwrap(), "Hi ${name");
    assert_eq!(js.expand("${1 + 1} and ${maestro.copiedText == 'Order").unwrap(), "2 and ${maestro.copiedText == 'Order");
    assert!(js.truthy("${maestro.copiedText == 'Order").unwrap());
}

#[test]
fn a_comparison_with_a_variable_is_false_when_it_does_not_hold() {
    let js = Js::new("android");
    js.set_var("GREETING", "Hello");
    assert!(js.truthy("${GREETING == 'Hello'}").unwrap());
    assert!(!js.truthy("${GREETING == 'Bye'}").unwrap());
}
