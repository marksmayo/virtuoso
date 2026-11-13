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

#[test]
fn faker_has_datafakers_common_providers() {
    let js = Js::new("android");
    let v = |e: &str| js.expand(&format!("${{{e}}}")).unwrap_or_else(|err| panic!("{e}: {err}")).into_owned();
    // Selector patterns: whole-text matches, case-insensitive (so case is checked on its own).
    let re = |e: &str, pattern: &str| {
        let got = v(e);
        assert!(virtuoso::select::Pattern::new(pattern).matches(&got), "{e} gave {got:?}");
        got
    };
    let capitalised = |s: String| assert!(s.starts_with(|c: char| c.is_ascii_uppercase()), "{s}");
    capitalised(re("faker.name().firstName()", "[A-Z][A-Za-z'-]+"));
    capitalised(re("faker.name().lastName()", "[A-Z][A-Za-z' -]+"));
    re("faker.name().username()", "[a-z0-9]+\\.[a-z0-9]+");
    re("faker.name().prefix()", "[A-Z][a-z]+\\.?");
    re("faker.internet().emailAddress('ann')", "ann@[a-z]+\\.[a-z]+");
    re("faker.internet().safeEmailAddress()", "[a-z0-9.]+@example\\.com");
    re("faker.internet().url()", "https://www\\.[a-z0-9-]+\\.[a-z]+/");
    re("faker.internet().uuid()", "[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}");
    re("faker.internet().ipV4Address()", "\\d{1,3}(\\.\\d{1,3}){3}");
    re("faker.internet().password(10, 12)", "[a-z0-9]{10,12}");
    capitalised(re("faker.address().country()", "[A-Z].+"));
    assert!(re("faker.address().countryCode()", "[A-Z]{2}").chars().all(|c| c.is_ascii_uppercase()));
    re("faker.address().zipCode()", "\\d{5}");
    re("faker.address().stateAbbr()", "[A-Z]{2}");
    re("faker.address().streetAddress()", "\\d+ [A-Z][A-Za-z' -]+ [A-Z][a-z]+");
    re("faker.address().fullAddress()", ".+, .+, [A-Z]{2} \\d{5}");
    re("faker.address().latitude()", "-?\\d+\\.\\d{8}");
    re("faker.number().randomNumber(6)", "[1-9]\\d{5}");
    re("faker.number().numberBetween(3, 5)", "[34]");
    re("faker.number().randomDigitNotZero()", "[1-9]");
    re("faker.number().digits(4)", "\\d{4}");
    re("faker.number().randomDouble(2, 1, 2)", "1(\\.\\d{1,2})?|2");
    re("faker.lorem().words(4).length", "4");
    capitalised(re("faker.lorem().sentence()", "[A-Z][a-z ]+\\."));
    re("faker.lorem().paragraph()", "([A-Z][a-z ]+\\. ?)+");
    re("faker.lorem().characters(7)", "[a-z0-9]{7}");
    re("faker.text().text(5)", "[a-z]{5}");
    re("faker.phoneNumber().cellPhone()", "\\([2-9]\\d\\d\\) [2-9]\\d\\d-\\d{4}");
    re("faker.color().name()", "[a-z ]+");
    assert!(!re("faker.color().hex()", "#[0-9A-F]{6}").chars().any(|c| c.is_ascii_lowercase()));
    re("faker.company().name()", ".+");
    re("faker.company().industry()", ".+");
    re("faker.company().profession()", "[a-z ]+");
    re("faker.bool().bool()", "true|false");
    // Datafaker's lists, not a handful of names: 20 draws are not all the same few.
    let names: std::collections::HashSet<String> = (0..20).map(|_| v("faker.name().firstName()")).collect();
    assert!(names.len() > 12, "{names:?}");
    // A provider or method it does not have fails the step (no Proxy to name the call).
    assert!(js.expand("${faker.name().nickname()}").is_err());
    assert!(js.expand("${faker.pokemon().name()}").is_err());
}

#[test]
fn faker_words_come_back_whole() {
    // Spaces, punctuation and non-ASCII letters inside a word are the word's own.
    let js = Js::new("android");
    let v = |e: &str| js.expand(&format!("${{{e}}}")).unwrap().into_owned();
    let address = v("faker.address().secondaryAddress()");
    assert!(virtuoso::select::Pattern::new("(Apt\\.|Suite) Ø\\d\\d").matches(&address), "{address}");
    for e in ["faker.address().country()", "faker.company().profession()", "faker.company().industry()"] {
        let words: std::collections::HashSet<String> = (0..300).map(|_| v(e)).collect();
        assert!(words.iter().any(|w| w.contains(' ')), "{e}: {words:?}");
        assert!(words.iter().all(|w| !w.is_empty() && w.trim() == w && !w.contains(['|', '"', '\n', ':'])), "{e}: {words:?}");
    }
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
fn a_variable_is_expanded_without_starting_the_engine() {
    // Every TAM flow opens with `tapOn: id: ${NAME}`: that alone started QuickJS (~0.5 MB).
    let js = Js::new("android");
    js.set_var("SCREEN", "home");
    js.set_var("SIZE", "9");
    js.set_var("SCREEN", "cart");
    assert_eq!(js.expand("lab-${SCREEN} size-${ SIZE }").unwrap(), "lab-cart size-9", "the latest value");
    assert!(!js.started());
    // A name that is a literal, not a variable, means what the engine makes of it.
    let js = Js::new("android");
    js.set_var("null", "y");
    js.set_var("true", "z");
    assert_eq!(js.expand("${null} ${true}").unwrap(), "null true");
    // Anything else is the engine's, which then has every variable set before it.
    let js = Js::new("android");
    js.set_var("A", "1");
    assert!(js.expand("${A}${B}").unwrap_err().to_string().contains("B"));
    assert!(js.started());
    assert_eq!(js.expand("${A + 1}").unwrap(), "11");
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
