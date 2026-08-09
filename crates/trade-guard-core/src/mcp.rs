//! MCP stdio JSON-RPC 2.0 transport. Same shape as `smart-dynamic-hedge`'s
//! `smart_hedge_mcp` crate (hand-rolled, no MCP SDK dependency, no
//! framework): newline-delimited JSON-RPC 2.0 messages on stdin, one
//! response line per request on stdout, blank lines and notifications
//! (no `id`) produce no response at all.
//!
//! `03-create-trade-guard-mcp.md` asks for Streamable HTTP and legacy
//! WebSocket transports too, gated behind OAuth 2.1/mTLS for remote use.
//! Neither exists in this vertical slice — stdio-only, same
//! trusted-local-caller assumption `auth.rs` documents. Adding a remote
//! transport is future work, not a silent gap: it needs real
//! authentication (`auth::CallerRole::resolve_stdio_caller` would no
//! longer be a safe default) which this slice deliberately has not built.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use crate::state::GuardState;
use crate::tools::{call_tool, tool_definitions};

pub const PROTOCOL_VERSION_DEFAULT: &str = "2024-11-05";
pub const SERVER_NAME: &str = "trade-guard-mcp";
pub const SERVER_VERSION: &str = "0.1.0";
pub const SERVER_INSTRUCTIONS: &str = "Paper-only account/policy/execution guard. No live-execution tool exists in this build. Every order this service can submit is a simulated paper fill; it is never a real broker order.";

fn initialize_result(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION_DEFAULT);
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
        "instructions": SERVER_INSTRUCTIONS,
    })
}

fn success_envelope(id: Value, result: Value) -> String {
    serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
        .expect("Value serialization is infallible")
}

fn error_envelope(id: Value, code: i32, message: &str) -> String {
    serde_json::to_string(
        &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    )
    .expect("Value serialization is infallible")
}

/// Handles one line of the stdio transport. Returns `None` for a blank
/// line or a notification (no `id`). Never panics on malformed input.
pub fn handle_line(state: &mut GuardState, line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Some(error_envelope(Value::Null, -32700, "Parse error")),
    };

    let id = parsed.get("id").cloned()?;
    let method = parsed
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let params = parsed.get("params").cloned().unwrap_or(Value::Null);

    let outcome: Result<Value, (i32, String)> = match method.as_str() {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(state, name, &arguments) {
                Ok(text) => Ok(json!({"content": [{"type": "text", "text": text}]})),
                Err(message) => {
                    Ok(json!({"content": [{"type": "text", "text": message}], "isError": true}))
                }
            }
        }
        other => Err((-32601, format!("Method not found: {other}"))),
    };

    Some(match outcome {
        Ok(result) => success_envelope(id, result),
        Err((code, message)) => error_envelope(id, code, &message),
    })
}

/// Reads newline-delimited JSON-RPC 2.0 messages from stdin and writes
/// responses to stdout, one line each.
pub fn run_stdio(state: &mut GuardState) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if let Some(response) = handle_line(state, &line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountSnapshot;
    use crate::decimal::Decimal;
    use crate::utc_timestamp::UtcTimestamp;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_state() -> (GuardState, std::path::PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "trade-guard-mcp-test-{}-{n}.sqlite3",
            std::process::id()
        ));
        let account = AccountSnapshot::new(
            "paper-default",
            "USD",
            Decimal::from_i64(100_000),
            UtcTimestamp::now(),
        );
        (GuardState::new(account, &path).unwrap(), path)
    }

    #[test]
    fn a_blank_line_produces_no_response() {
        let (mut state, path) = test_state();
        assert_eq!(handle_line(&mut state, ""), None);
        assert_eq!(handle_line(&mut state, "   "), None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn malformed_json_returns_a_parse_error_with_null_id() {
        let (mut state, path) = test_state();
        let response = handle_line(&mut state, "{not valid json").unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["error"]["code"], -32700);
        assert_eq!(value["id"], Value::Null);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_notification_with_no_id_produces_no_response() {
        let (mut state, path) = test_state();
        let response = handle_line(
            &mut state,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert_eq!(response, None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn initialize_echoes_the_requested_protocol_version() {
        let (mut state, path) = test_state();
        let response = handle_line(&mut state, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-01-01"}}"#).unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["result"]["protocolVersion"], "2025-01-01");
        assert_eq!(value["result"]["serverInfo"]["name"], SERVER_NAME);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tools_list_contains_no_live_execution_tool() {
        let (mut state, path) = test_state();
        let response = handle_line(
            &mut state,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        let names: Vec<&str> = value["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"authorize-and-submit-paper-order"));
        for forbidden in ["authorize-and-submit-live-order", "arm-live-execution"] {
            assert!(!names.contains(&forbidden));
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tools_call_health_is_not_an_error() {
        let (mut state, path) = test_state();
        let response = handle_line(&mut state, r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"health","arguments":{}}}"#).unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert!(value["result"].get("isError").is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tools_call_with_an_unknown_tool_name_is_an_error_result_not_a_protocol_error() {
        let (mut state, path) = test_state();
        let response = handle_line(&mut state, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"authorize-and-submit-live-order","arguments":{}}}"#).unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert!(
            value.get("error").is_none(),
            "should be a tool-level error, not a JSON-RPC error: {value}"
        );
        assert_eq!(value["result"]["isError"], true);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_unknown_top_level_method_is_a_jsonrpc_protocol_error() {
        let (mut state, path) = test_state();
        let response = handle_line(
            &mut state,
            r#"{"jsonrpc":"2.0","id":7,"method":"bogus/method"}"#,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["error"]["code"], -32601);
        assert_eq!(value["id"], 7);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ping_is_answered() {
        let (mut state, path) = test_state();
        let response =
            handle_line(&mut state, r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#).unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["result"], json!({}));
        let _ = std::fs::remove_file(path);
    }
}
