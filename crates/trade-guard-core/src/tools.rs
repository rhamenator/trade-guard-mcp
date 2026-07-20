//! Tool implementations dispatched by `mcp.rs`. A deliberately small,
//! honest subset of `03-create-trade-guard-mcp.md`'s ~90-tool catalog —
//! matching this vertical slice's own scope (paper-only, single
//! in-memory account, no resting order book, no remote transport):
//!
//! ```text
//! health, capabilities, tool-catalog, self-test
//! validate-trade-intent, check-evidence-eligibility
//! get-account-snapshot, get-positions, get-open-orders
//! authorize-and-submit-paper-order
//! list-recent-decisions, replay-decision, audit-integrity
//! ```
//!
//! No live tool exists at all — not "present but disabled", genuinely
//! absent from this source file — satisfying "live tools are absent from
//! discovery unless all [hard gates] are true" trivially, since none of
//! the gates exist to satisfy yet. `cancel-paper-order`/`replace-paper-order`
//! are deliberately **not** implemented: `providers::PaperSimulator` keeps
//! no persistent open-order book across calls (an "open" limit order from
//! one `authorize-and-submit-paper-order` call is not retrievable or
//! mutable in a later call), so a cancel tool would have nothing real to
//! act on — pretending otherwise would be dishonest rather than merely
//! incomplete. See `providers.rs`'s module doc comment for the same
//! limitation stated from the simulator's side.

use serde_json::{json, Value};

use crate::decimal::Decimal;
use crate::evidence::EvidenceBundle;
use crate::execution::authorize_and_submit_paper_order;
use crate::policy::{check_evidence_eligibility, validate_trade_intent};
use crate::sha256::sha256_hex;
use crate::state::GuardState;
use crate::trade_intent::TradeIntent;
use crate::utc_timestamp::UtcTimestamp;

/// MCP `tools/list` entries: name, description, JSON Schema input shape.
pub fn tool_definitions() -> Value {
    json!([
        {"name": "health", "description": "Service health; proves no live-execution or broker-order capability is compiled in.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "capabilities", "description": "What this service can and cannot do: paper-only, single in-memory account, no resting order book.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "tool-catalog", "description": "The full list of tools this server exposes, with why any tool is absent.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "self-test", "description": "Runs internal invariant checks (decimal/hash primitives, audit-chain integrity) and reports pass/fail.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "validate-trade-intent", "description": "Schema-shape and sanity validation for a TradeIntent, independent of account state or evidence.", "inputSchema": {"type": "object", "properties": {"intent": {"type": "object"}}, "required": ["intent"], "additionalProperties": false}},
        {"name": "check-evidence-eligibility", "description": "Verifies a signed EvidenceBundle against a TradeIntent before it may back an order.", "inputSchema": {"type": "object", "properties": {"intent": {"type": "object"}, "evidence": {"type": ["object", "null"]}}, "required": ["intent"], "additionalProperties": false}},
        {"name": "get-account-snapshot", "description": "The current in-memory paper account: cash, reserved cash, positions.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "get-positions", "description": "Current paper positions only.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "get-open-orders", "description": "Non-terminal orders found in the recent audit window (best-effort; see tool description limitations).", "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "default": 50}}, "additionalProperties": false}},
        {"name": "authorize-and-submit-paper-order", "description": "The atomic authorize-and-submit protocol against the internal paper simulator.", "inputSchema": {"type": "object", "properties": {"intent": {"type": "object"}, "evidence": {"type": ["object", "null"]}}, "required": ["intent"], "additionalProperties": false}},
        {"name": "list-recent-decisions", "description": "Recent audit records, newest first.", "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "default": 10}}, "additionalProperties": false}},
        {"name": "replay-decision", "description": "Reads one stored audit record by event ID without mutating any state.", "inputSchema": {"type": "object", "properties": {"event_id": {"type": "string"}}, "required": ["event_id"], "additionalProperties": false}},
        {"name": "audit-integrity", "description": "Verifies the audit hash chain end to end; reports the first tampering point found, if any.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
    ])
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("Value serialization is infallible")
}

pub fn health(state: &GuardState) -> Result<String, String> {
    Ok(pretty(&json!({
        "status": "ok",
        "mode": "paper",
        "live_execution_allowed": false,
        "broker_order_endpoint_present": false,
        "account_alias": state.account.account_alias,
        "audit_store_path": state.audit.path().display().to_string(),
    })))
}

pub fn capabilities() -> Result<String, String> {
    Ok(pretty(&json!({
        "modes_supported": ["observe", "advisory", "paper", "paper-autonomous"],
        "modes_rejected": ["guarded-live", "guarded-live-autonomous"],
        "providers": ["paper-sim"],
        "asset_classes": ["equity", "etf"],
        "resting_order_book": false,
        "market_abuse_surveillance": false,
        "reconciliation": false,
        "live_arming": false,
    })))
}

pub fn tool_catalog() -> Result<String, String> {
    Ok(pretty(&json!({
        "tools": tool_definitions(),
        "live_tools_absent_because": "not compiled into this vertical slice at all — see tools.rs module doc comment",
    })))
}

pub fn self_test(state: &GuardState) -> Result<String, String> {
    let mut checks: Vec<(&'static str, bool)> = Vec::new();

    checks.push(("sha256_empty_string_matches_nist_vector", sha256_hex(b"") == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));

    let decimal_round_trip = Decimal::parse("42.5").map(|d| d.to_decimal_string() == "42.5").unwrap_or(false);
    checks.push(("decimal_round_trip", decimal_round_trip));

    let timestamp_round_trip = UtcTimestamp::parse_rfc3339("2026-07-19T00:00:00Z")
        .map(|t| t.to_rfc3339() == "2026-07-19T00:00:00Z")
        .unwrap_or(false);
    checks.push(("utc_timestamp_round_trip", timestamp_round_trip));

    let audit_integrity = state.audit.verify_integrity().map(|r| r.is_valid()).unwrap_or(false);
    checks.push(("audit_chain_integrity", audit_integrity));

    let all_passed = checks.iter().all(|(_, ok)| *ok);
    let result = json!({
        "status": if all_passed { "PASS" } else { "FAIL" },
        "checks": checks.into_iter().map(|(name, ok)| json!({"name": name, "passed": ok})).collect::<Vec<_>>(),
    });
    if all_passed {
        Ok(pretty(&result))
    } else {
        Err(pretty(&result))
    }
}

fn parse_intent(arguments: &Value) -> Result<TradeIntent, String> {
    let raw = arguments.get("intent").ok_or_else(|| "\"intent\" is required".to_string())?;
    serde_json::from_value(raw.clone()).map_err(|e| format!("invalid trade intent: {e}"))
}

fn parse_optional_evidence(arguments: &Value) -> Result<Option<EvidenceBundle>, String> {
    match arguments.get("evidence") {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => serde_json::from_value(raw.clone()).map(Some).map_err(|e| format!("invalid evidence bundle: {e}")),
    }
}

pub fn validate_trade_intent_tool(arguments: &Value) -> Result<String, String> {
    let intent = parse_intent(arguments)?;
    let outcome = validate_trade_intent(&intent, UtcTimestamp::now());
    Ok(pretty(&serde_json::to_value(&outcome).expect("PolicyOutcome serialization is infallible")))
}

pub fn check_evidence_eligibility_tool(arguments: &Value) -> Result<String, String> {
    let intent = parse_intent(arguments)?;
    let evidence = parse_optional_evidence(arguments)?;
    let outcome = check_evidence_eligibility(&intent, evidence.as_ref(), UtcTimestamp::now());
    Ok(pretty(&serde_json::to_value(&outcome).expect("PolicyOutcome serialization is infallible")))
}

pub fn get_account_snapshot(state: &GuardState) -> Result<String, String> {
    Ok(pretty(&serde_json::to_value(&state.account).expect("AccountSnapshot serialization is infallible")))
}

pub fn get_positions(state: &GuardState) -> Result<String, String> {
    Ok(pretty(&serde_json::to_value(&state.account.positions).expect("positions serialization is infallible")))
}

/// Best-effort: scans the recent audit window for order records whose
/// state is not terminal. Not a true persistent open-order index — see
/// this module's doc comment for why one does not exist yet.
pub fn get_open_orders(state: &GuardState, arguments: &Value) -> Result<String, String> {
    let limit = arguments.get("limit").and_then(Value::as_i64).unwrap_or(50);
    let recent = state.audit.list_recent(limit).map_err(|e| e.to_string())?;
    let open: Vec<Value> = recent
        .into_iter()
        .filter_map(|r| r.order)
        .filter(|o| !o.is_terminal())
        .map(|o| serde_json::to_value(&o).expect("Order serialization is infallible"))
        .collect();
    Ok(pretty(&json!({ "open_orders": open })))
}

pub fn authorize_and_submit_paper_order_tool(state: &mut GuardState, arguments: &Value) -> Result<String, String> {
    let intent = parse_intent(arguments)?;
    let evidence = parse_optional_evidence(arguments)?;
    let now = UtcTimestamp::now();
    let result = authorize_and_submit_paper_order(&mut state.account, &state.simulator, &state.audit, intent, evidence.as_ref(), now)
        .map_err(|e| e.to_string())?;

    let body = json!({
        "policy_outcome": result.policy_outcome,
        "order": result.order,
        "was_duplicate": result.was_duplicate,
    });
    if result.policy_outcome.is_allowed() {
        Ok(pretty(&body))
    } else {
        Err(pretty(&body))
    }
}

pub fn list_recent_decisions(state: &GuardState, arguments: &Value) -> Result<String, String> {
    let limit = arguments.get("limit").and_then(Value::as_i64).unwrap_or(10);
    let recent = state.audit.list_recent(limit).map_err(|e| e.to_string())?;
    Ok(pretty(&serde_json::to_value(&recent).expect("records serialization is infallible")))
}

pub fn replay_decision(state: &GuardState, arguments: &Value) -> Result<String, String> {
    let event_id = arguments.get("event_id").and_then(Value::as_str).unwrap_or("");
    if event_id.is_empty() {
        return Err("event_id is required".to_string());
    }
    match state.audit.get(event_id).map_err(|e| e.to_string())? {
        Some(record) => Ok(pretty(&serde_json::to_value(&record).expect("record serialization is infallible"))),
        None => Err(format!("no audit record found for event_id {event_id:?}")),
    }
}

pub fn audit_integrity(state: &GuardState) -> Result<String, String> {
    let report = state.audit.verify_integrity().map_err(|e| e.to_string())?;
    let is_valid = report.is_valid();
    let body = json!({
        "valid": is_valid,
        "records_checked": report.records_checked,
        "failure": report.failure.map(|f| format!("{f:?}")),
    });
    if is_valid {
        Ok(pretty(&body))
    } else {
        Err(pretty(&body))
    }
}

/// Dispatches a `tools/call` request's `name`/`arguments` to the matching
/// tool. `Err` (both "unknown tool" and any tool-reported failure)
/// becomes an MCP `isError: true` result, not a JSON-RPC protocol error —
/// same convention `smart-dynamic-hedge`'s `smart-hedge-mcp` uses.
pub fn call_tool(state: &mut GuardState, name: &str, arguments: &Value) -> Result<String, String> {
    match name {
        "health" => health(state),
        "capabilities" => capabilities(),
        "tool-catalog" => tool_catalog(),
        "self-test" => self_test(state),
        "validate-trade-intent" => validate_trade_intent_tool(arguments),
        "check-evidence-eligibility" => check_evidence_eligibility_tool(arguments),
        "get-account-snapshot" => get_account_snapshot(state),
        "get-positions" => get_positions(state),
        "get-open-orders" => get_open_orders(state, arguments),
        "authorize-and-submit-paper-order" => authorize_and_submit_paper_order_tool(state, arguments),
        "list-recent-decisions" => list_recent_decisions(state, arguments),
        "replay-decision" => replay_decision(state, arguments),
        "audit-integrity" => audit_integrity(state),
        other => Err(format!("unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountSnapshot;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_state() -> (GuardState, std::path::PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("trade-guard-tools-test-{}-{n}.sqlite3", std::process::id()));
        let account = AccountSnapshot::new("paper-default", "USD", Decimal::from_i64(100_000), UtcTimestamp::now());
        (GuardState::new(account, &path).unwrap(), path)
    }

    fn valid_intent_json(idempotency_key: &str) -> Value {
        json!({
            "schema-version": "2.0.0",
            "intent-id": format!("intent-{idempotency_key}"),
            "strategy-id": "strat-1",
            "decision-id": "decision-1",
            "account-alias": "paper-default",
            "instrument": {
                "schema-version": "2.0.0",
                "instrument-id": "inst-1",
                "asset-class": "equity",
                "symbol": "SPY"
            },
            "side": "buy",
            "order-type": "market",
            "quantity": "10",
            "time-in-force": "day",
            "decision-time": "2026-01-01T00:00:00Z",
            "confidence": 0.8,
            "signal-ids": [],
            "mode": "paper",
            "idempotency-key": idempotency_key
        })
    }

    #[test]
    fn health_reports_no_live_execution() {
        let (state, path) = test_state();
        let text = health(&state).unwrap();
        assert!(text.contains("\"live_execution_allowed\": false"));
        assert!(text.contains("\"broker_order_endpoint_present\": false"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tool_catalog_never_lists_a_live_tool() {
        let names: Vec<String> = tool_definitions().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for forbidden in ["authorize-and-submit-live-order", "arm-live-execution", "disable-kill-switch"] {
            assert!(!names.contains(&forbidden.to_string()));
        }
    }

    #[test]
    fn self_test_passes_on_a_fresh_store() {
        let (state, path) = test_state();
        assert!(self_test(&state).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn validate_trade_intent_tool_accepts_a_well_formed_intent() {
        let args = json!({"intent": valid_intent_json("idem-1")});
        assert!(validate_trade_intent_tool(&args).is_ok());
    }

    #[test]
    fn validate_trade_intent_tool_reports_missing_intent_argument() {
        let result = validate_trade_intent_tool(&json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn authorize_and_submit_paper_order_tool_fills_a_market_order() {
        let (mut state, path) = test_state();
        let args = json!({"intent": valid_intent_json("idem-fill")});
        let result = authorize_and_submit_paper_order_tool(&mut state, &args).unwrap();
        assert!(result.contains("\"was_duplicate\": false"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn authorize_and_submit_paper_order_tool_second_call_is_a_duplicate() {
        let (mut state, path) = test_state();
        let args = json!({"intent": valid_intent_json("idem-dup")});
        authorize_and_submit_paper_order_tool(&mut state, &args).unwrap();
        let second = authorize_and_submit_paper_order_tool(&mut state, &args).unwrap();
        assert!(second.contains("\"was_duplicate\": true"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn get_positions_reflects_a_filled_order() {
        let (mut state, path) = test_state();
        let args = json!({"intent": valid_intent_json("idem-pos")});
        authorize_and_submit_paper_order_tool(&mut state, &args).unwrap();
        let text = get_positions(&state).unwrap();
        assert!(text.contains("\"inst-1\""));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn list_recent_decisions_and_replay_decision_round_trip() {
        let (mut state, path) = test_state();
        let args = json!({"intent": valid_intent_json("idem-replay")});
        authorize_and_submit_paper_order_tool(&mut state, &args).unwrap();

        let recent = list_recent_decisions(&state, &json!({})).unwrap();
        assert!(recent.contains("idem-replay"));

        let replayed = replay_decision(&state, &json!({"event_id": "evt-idem-replay"})).unwrap();
        assert!(replayed.contains("idem-replay"));

        assert!(replay_decision(&state, &json!({"event_id": "nonexistent"})).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audit_integrity_is_valid_after_normal_use() {
        let (mut state, path) = test_state();
        let args = json!({"intent": valid_intent_json("idem-integrity")});
        authorize_and_submit_paper_order_tool(&mut state, &args).unwrap();
        assert!(audit_integrity(&state).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn call_tool_returns_an_error_for_an_unknown_tool_name() {
        let (mut state, path) = test_state();
        assert!(call_tool(&mut state, "authorize-and-submit-live-order", &json!({})).is_err());
        let _ = std::fs::remove_file(path);
    }
}
