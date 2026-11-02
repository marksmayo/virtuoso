//! `virtuoso mcp`: the Model Context Protocol server, against a fake device.

use virtuoso::mcp::McpServer;
use virtuoso::testing::{node, FakeDevice, FakeScreen};
use serde_json::{json, Value};

fn server() -> McpServer {
    McpServer::new(Box::new(|| {
        let mut d = FakeDevice::new();
        d.add_screen("home", FakeScreen::new(vec![node("Sign in", [100, 100, 300, 160]).goes_to("welcome").id("com.app:id/login")]));
        d.add_screen("welcome", FakeScreen::new(vec![node("Welcome", [100, 100, 300, 160])]));
        d.show("home");
        Ok(Box::new(d))
    }))
}

fn call(s: &mut McpServer, id: i64, method: &str, params: Value) -> Value {
    s.handle(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).expect("a reply")
}

fn tool_text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn initialize_names_the_server_and_offers_tools() {
    let mut s = server();
    let r = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}));
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["serverInfo"]["name"], "virtuoso");
    assert!(r["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn notifications_get_no_reply() {
    let mut s = server();
    assert!(s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
}

#[test]
fn tools_list_describes_look_run_and_screenshot() {
    let mut s = server();
    let r = call(&mut s, 2, "tools/list", json!({}));
    let names: Vec<&str> = r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["look", "run", "screenshot"]);
    for t in r["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn look_lists_what_is_on_screen_compactly() {
    let mut s = server();
    let text = tool_text(&call(&mut s, 3, "tools/call", json!({"name": "look", "arguments": {}})));
    assert!(text.contains("\"Sign in\""), "{text}");
    assert!(text.contains("#login"), "{text}");
}

#[test]
fn run_executes_flow_commands_and_reports_each_step_and_the_new_screen() {
    let mut s = server();
    let r = call(&mut s, 4, "tools/call", json!({"name": "run", "arguments": {"commands": "- tapOn: Sign in\n- assertVisible: Welcome"}}));
    assert_eq!(r["result"]["isError"], false);
    let text = tool_text(&r);
    assert!(text.contains("passed") && text.contains("Tap on \"Sign in\""), "{text}");
    assert!(text.contains("\"Welcome\""), "the screen after the commands: {text}");
}

#[test]
fn the_device_stays_connected_between_calls() {
    let mut s = server();
    call(&mut s, 5, "tools/call", json!({"name": "run", "arguments": {"commands": "- tapOn: Sign in"}}));
    let text = tool_text(&call(&mut s, 6, "tools/call", json!({"name": "look", "arguments": {}})));
    assert!(text.contains("Welcome"), "{text}");
}

#[test]
fn a_failing_command_is_a_tool_error_with_its_message() {
    let mut s = server();
    let r = call(&mut s, 7, "tools/call", json!({"name": "run", "arguments": {"commands": "- tapOn: Nowhere", "timeoutMs": 100}}));
    assert_eq!(r["result"]["isError"], true);
    assert!(tool_text(&r).contains("Nowhere"));
}

#[test]
fn invalid_yaml_is_a_tool_error_not_a_crash() {
    let mut s = server();
    let r = call(&mut s, 8, "tools/call", json!({"name": "run", "arguments": {"commands": "- flyTo: moon"}}));
    assert_eq!(r["result"]["isError"], true);
    assert!(tool_text(&r).contains("flyTo"));
}

#[test]
fn screenshot_returns_a_png_image() {
    let mut s = server();
    let r = call(&mut s, 9, "tools/call", json!({"name": "screenshot", "arguments": {}}));
    assert_eq!(r["result"]["content"][0]["type"], "image");
    assert_eq!(r["result"]["content"][0]["mimeType"], "image/png");
}

#[test]
fn an_unknown_method_is_a_json_rpc_error() {
    let mut s = server();
    let r = call(&mut s, 10, "prompts/get", json!({}));
    assert_eq!(r["error"]["code"], -32601);
}
