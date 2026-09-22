use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn exchange(allow: bool, messages: Vec<Value>) -> Vec<Value> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_sanc"));
    cmd.arg("mcp");
    if allow {
        cmd.arg("--allow-exec");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        for m in messages {
            writeln!(input, "{m}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}", out);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn init() -> Vec<Value> {
    vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ]
}
#[test]
fn default_tools_are_read_only_and_handshake_is_required() {
    let out = exchange(
        false,
        vec![json!({"jsonrpc":"2.0","id":0,"method":"tools/list"})],
    );
    assert_eq!(out[0]["error"]["code"], -32000);
    let mut messages = init();
    messages.push(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let out = exchange(false, messages);
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-11-25");
    let tools = out[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 4);
    assert!(tools
        .iter()
        .all(|t| t["annotations"]["readOnlyHint"] == true));
}
#[test]
fn mcp_exec_shares_hard_sudo_rejection() {
    let mut messages = init();
    messages.push(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"session":"s","request_id":"r","command":" sudo id"}}}));
    let out = exchange(true, messages);
    assert_eq!(out[1]["result"]["isError"], true);
    assert!(out[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("approval_required"));
}
