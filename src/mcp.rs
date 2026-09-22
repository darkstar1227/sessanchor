//! Minimal newline-framed stdio MCP adapter; no network listener.
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};

fn tool(name: &str, description: &str, properties: Value, required: &[&str], read: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":true}})
}

fn tools(allow_exec: bool) -> Vec<Value> {
    let string = json!({"type":"string"});
    let integer = json!({"type":"integer","minimum":1});
    let mut tools = vec![
        tool(
            "devices",
            "Cached device observations; does not connect.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "sessions",
            "List sessions and short descriptions.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "task",
            "Inspect task. Unknown is not permission to replay.",
            json!({"id":integer}),
            &["id"],
            true,
        ),
        tool(
            "output",
            "Read bounded output; reuse next_cursor. UTF-8 or hex.",
            json!({"id":integer,"cursor":{"type":"integer","minimum":0},"stream":{"type":"string","enum":["stdout","stderr"]}}),
            &["id"],
            true,
        ),
    ];
    if allow_exec {
        tools.push(tool(
            "session_create",
            "Create independent-command session, not interactive shell.",
            json!({"id":string,"device":string}),
            &["id", "device"],
            false,
        ));
        tools.push(tool(
            "session_describe",
            "Set short handoff description.",
            json!({"id":string,"description":{"type":"string","maxLength":1024}}),
            &["id", "description"],
            false,
        ));
        tools.push(tool("exec","Submit once; reuse request_id on retry. STOP on approval_required. No remote persistence on disconnect yet.",json!({"session":string,"request_id":string,"command":string,"shell":{"type":"string","enum":["default","powershell"]}}),&["session","request_id","command"],false));
    }
    tools
}

fn arguments(name: &str, a: &Value, allow_exec: bool) -> Result<Vec<String>, &'static str> {
    let schema = tools(allow_exec)
        .into_iter()
        .find(|t| t["name"] == name)
        .ok_or("tool_not_available")?;
    let object = a.as_object().ok_or("invalid_tool_arguments")?;
    let properties = schema["inputSchema"]["properties"]
        .as_object()
        .ok_or("invalid_tool_arguments")?;
    if object.keys().any(|key| !properties.contains_key(key)) {
        return Err("invalid_tool_arguments");
    }
    let s = |key: &str| {
        a.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or("invalid_tool_arguments")
    };
    let id = || {
        a.get("id")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .ok_or("invalid_tool_arguments")
    };
    let args = match name {
        "devices" => vec!["device".into(), "list".into()],
        "sessions" => vec!["session".into(), "list".into()],
        "task" => vec!["task".into(), id()?],
        "output" => vec![
            "output".into(),
            id()?,
            "--cursor".into(),
            a.get("cursor")
                .unwrap_or(&json!(0))
                .as_u64()
                .ok_or("invalid_tool_arguments")?
                .to_string(),
            "--stream".into(),
            a.get("stream")
                .unwrap_or(&json!("stdout"))
                .as_str()
                .filter(|s| ["stdout", "stderr"].contains(s))
                .ok_or("invalid_tool_arguments")?
                .to_owned(),
        ],
        "session_create" if allow_exec => vec![
            "session".into(),
            "create".into(),
            s("id")?,
            "--device".into(),
            s("device")?,
        ],
        "session_describe" if allow_exec => vec![
            "session".into(),
            "describe".into(),
            s("id")?,
            s("description")?,
        ],
        "exec" if allow_exec => vec![
            "exec".into(),
            s("session")?,
            "--shell".into(),
            if a.get("shell").is_some() {
                s("shell")?
            } else {
                "default".into()
            },
            "--request-id".into(),
            s("request_id")?,
            "--command".into(),
            s("command")?,
        ],
        _ => return Err("tool_not_available"),
    };
    Ok(args)
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn serve(state_dir: Option<PathBuf>, allow_exec: bool) -> io::Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut initialized = false;
    let mut negotiated = false;
    loop {
        let mut line = Vec::new();
        let n = input
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        if n > 1024 * 1024 {
            return Err(io::Error::other("MCP message too large"));
        }
        let request: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                writeln!(out, "{}", error(Value::Null, -32700, "Parse error"))?;
                out.flush()?;
                continue;
            }
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if request.get("id").is_none() {
            if negotiated && method == "notifications/initialized" {
                initialized = true;
            }
            continue;
        }
        let id = request["id"].clone();
        let response = if request["jsonrpc"] != "2.0" || !id.is_string() && !id.is_number() {
            error(Value::Null, -32600, "Invalid request")
        } else {
            match method {
                "initialize" if !negotiated => {
                    negotiated = true;
                    json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"sessanchor","version":env!("CARGO_PKG_VERSION")},"instructions":"Reuse request_id and output cursors. Stop on approval_required. Never replay unknown tasks. Remote disconnect persistence is unavailable in this preview."}})
                }
                "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
                _ if !initialized => error(id, -32000, "Initialize first"),
                "tools/list" => {
                    json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools(allow_exec)}})
                }
                "tools/call" => {
                    let name = request["params"]["name"].as_str().unwrap_or("");
                    let a = request["params"]
                        .get("arguments")
                        .cloned()
                        .unwrap_or(json!({}));
                    match arguments(name, &a, allow_exec) {
                        Err(code) => error(id, -32602, code),
                        Ok(args) => {
                            let (value, is_error) = match crate::cli::invoke(
                                args,
                                state_dir.clone(),
                            ) {
                                Ok(value) => (value, false),
                                Err(code) => (
                                    json!({"error":code,"next_action":if code=="approval_required"{"STOP. Ask user; do not rewrite or retry."}else{"Inspect status. Never replay unknown tasks."}}),
                                    true,
                                ),
                            };
                            json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":value.to_string()}],"isError":is_error}})
                        }
                    }
                }
                _ => error(id, -32601, "Method not found"),
            }
        };
        writeln!(out, "{response}")?;
        out.flush()?;
    }
}
