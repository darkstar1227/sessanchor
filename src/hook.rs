use serde_json::{json, Value};
use std::io::{self, Read};

pub(crate) fn response(event: &Value) -> Value {
    if event["hook_event_name"] != "PreToolUse" || event["tool_name"] != "mcp__sessanchor__exec" {
        return json!({});
    }
    let args = &event["tool_input"];
    let command = args["command"].as_str().unwrap_or("");
    let reason = if sessanchor::check_command_policy(command).is_err() {
        Some("STOP: sudo requires user review. Do not rewrite or retry.")
    } else if args["request_id"].as_str().is_none_or(str::is_empty) {
        Some("Provide request_id; reuse it for retries. Never replay unknown tasks.")
    } else {
        None
    };
    match reason {
        Some(reason) => {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":reason}})
        }
        None => json!({}),
    }
}

pub fn serve() -> io::Result<()> {
    let mut bytes = Vec::new();
    io::stdin().take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("hook event too large"));
    }
    let event: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    println!("{}", response(&event));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_for_valid_calls_and_unrelated_context_mode_tools() {
        assert_eq!(
            response(
                &json!({"hook_event_name":"PreToolUse","tool_name":"mcp__context_mode__ctx_execute","tool_input":{"command":"sudo id"}})
            ),
            json!({})
        );
        assert_eq!(
            response(
                &json!({"hook_event_name":"PreToolUse","tool_name":"mcp__sessanchor__exec","tool_input":{"command":"id","request_id":"r"}})
            ),
            json!({})
        );
    }
    #[test]
    fn sudo_is_denied_without_echoing_command_or_rewriting_input() {
        let result = response(
            &json!({"hook_event_name":"PreToolUse","tool_name":"mcp__sessanchor__exec","tool_input":{"command":"sudo secret-marker","request_id":"r"}}),
        );
        assert_eq!(result["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(!result.to_string().contains("secret-marker"));
        assert!(result["hookSpecificOutput"].get("updatedInput").is_none());
    }
}
