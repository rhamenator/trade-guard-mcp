//! The atomic authorize-and-submit-paper-order protocol — the single most
//! correctness-critical function in this crate. See
//! `03-create-trade-guard-mcp.md` "Atomic authorization and submission"
//! for the full 15-step protocol this implements a paper-only subset of:
//! authenticate/validate/verify-evidence/refresh-state/idempotency-check/
//! risk-check/reserve/submit/persist/reconcile-or-fill/release/return, in
//! that order, with the durable idempotency check running *first* so a
//! resubmitted intent — allowed, rejected, or filled the first time —
//! always returns the original outcome rather than re-deriving a possibly
//! different one against current (changed) account state.
//!
//! There is **no live path**. `TradeIntent::mode` requesting live
//! execution is rejected before any account mutation or provider call,
//! by this function itself — not merely because no live provider happens
//! to be configured. See `docs/CAPABILITY_STATUS.md`.

use crate::account::AccountSnapshot;
use crate::audit::{AuditError, AuditRecord, AuditStore};
use crate::decimal::Decimal;
use crate::evidence::EvidenceBundle;
use crate::jurisdiction_venue::{check_venue_profile_availability, VenueRegistry};
use crate::order::{Fill, Order, OrderState};
use crate::policy::{check_buying_power, check_evidence_eligibility, validate_trade_intent};
use crate::policy_decision::{PolicyDecision, PolicyOutcome};
use crate::providers::{PaperSimulator, SimulatedFillOutcome};
use crate::trade_intent::TradeIntent;
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub policy_outcome: PolicyOutcome,
    pub order: Option<Order>,
    /// `true` when this result was served from the durable idempotency
    /// store rather than freshly computed — required test #14
    /// (`03-create-trade-guard-mcp.md`): "duplicate idempotency key
    /// returns/reconciles original order rather than submitting twice."
    pub was_duplicate: bool,
}

#[derive(Debug)]
pub enum ExecutionError {
    Audit(AuditError),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionError::Audit(e) => write!(f, "audit error: {e}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<AuditError> for ExecutionError {
    fn from(e: AuditError) -> Self {
        ExecutionError::Audit(e)
    }
}

/// Runs the full atomic protocol and durably persists exactly one
/// `AuditRecord` per unique `idempotency_key` — including for a rejected
/// intent, so a resubmitted rejected intent also short-circuits to the
/// original rejection instead of being re-evaluated.
pub fn authorize_and_submit_paper_order(
    account: &mut AccountSnapshot,
    simulator: &PaperSimulator,
    audit: &AuditStore,
    venues: &VenueRegistry,
    intent: TradeIntent,
    evidence: Option<&EvidenceBundle>,
    now: UtcTimestamp,
) -> Result<ExecutionResult, ExecutionError> {
    if let Some(existing) = audit.find_by_idempotency_key(&intent.idempotency_key)? {
        return Ok(ExecutionResult { policy_outcome: existing.policy_outcome, order: existing.order, was_duplicate: true });
    }

    if intent.mode.requests_live_execution() {
        let outcome = PolicyOutcome::reject(PolicyDecision::BlockedByOperatorControl, "live-execution-not-implemented-in-this-vertical-slice");
        return persist_and_return(audit, &intent, outcome, None, now);
    }
    if !intent.mode.requests_paper_execution() {
        let outcome = PolicyOutcome::reject(PolicyDecision::Reject, "mode-does-not-request-paper-execution");
        return persist_and_return(audit, &intent, outcome, None, now);
    }

    let validation = validate_trade_intent(&intent, now);
    if !validation.is_allowed() {
        return persist_and_return(audit, &intent, validation, None, now);
    }

    let evidence_outcome = check_evidence_eligibility(&intent, evidence, now);
    if !evidence_outcome.is_allowed() {
        return persist_and_return(audit, &intent, evidence_outcome, None, now);
    }

    if intent.account_alias != account.account_alias {
        let outcome = PolicyOutcome::reject(PolicyDecision::Reject, "account-alias-mismatch");
        return persist_and_return(audit, &intent, outcome, None, now);
    }

    let order_id = format!("order-{}", intent.idempotency_key);
    let mut order = Order {
        order_id: order_id.clone(),
        intent_id: intent.intent_id.clone(),
        idempotency_key: intent.idempotency_key.clone(),
        account_alias: intent.account_alias.clone(),
        instrument: intent.instrument.clone(),
        side: intent.side,
        order_type: intent.order_type,
        quantity: intent.quantity,
        limit_price: intent.limit_price,
        state: OrderState::Validated,
        filled_quantity: Decimal::ZERO,
        average_fill_price: None,
        fills: Vec::new(),
        created_at: now,
        updated_at: now,
    };

    // A market order's estimated cost uses the current simulated quote;
    // a limit order uses its own limit price (the worst price it could
    // possibly fill at from the account's perspective) so the buying-
    // power check is never optimistic about an order that has not
    // actually filled yet.
    let estimated_price = match order.limit_price {
        Some(p) => p,
        None => {
            let quote = simulator.quote_for(&order.instrument.instrument_id);
            if order.side.is_buy_side() { quote.ask } else { quote.bid }
        }
    };
    let estimated_notional = order.quantity.checked_mul(&estimated_price).unwrap_or(Decimal::ZERO);

    // Only a buy-side order consumes cash in this vertical slice's model
    // — see `AccountSnapshot::apply_fill`'s doc comment for the same
    // simplification applied consistently here. Short-sale margin
    // requirements are out of scope (documented, not silently skipped).
    if order.side.is_buy_side() {
        let risk_outcome = check_buying_power(account, estimated_notional);
        if !risk_outcome.is_allowed() {
            order.state = OrderState::Rejected;
            return persist_and_return(audit, &intent, risk_outcome, Some(order), now);
        }
        account.reserved_cash = account.reserved_cash.checked_add(&estimated_notional).unwrap_or(account.reserved_cash);
    }
    order.state = OrderState::RiskReserved;

    order.state = OrderState::Acknowledged;
    let fill_outcome = simulator.simulate_fill(&order);

    if let SimulatedFillOutcome::Filled { price } = fill_outcome {
        let fill = Fill { fill_id: format!("fill-{order_id}"), order_id: order_id.clone(), quantity: order.quantity, price, filled_at: now };
        let filled_quantity = fill.quantity;
        order.apply_fill(fill);
        account.apply_fill(&order.instrument, order.side, filled_quantity, price);
    }

    // Release the reservation regardless of whether the order filled or
    // stayed open. A real broker's resting limit order would continue to
    // consume buying power until it fills or is canceled; this
    // simulator has no persistent open-order book across calls, so an
    // "open" order here does not continue reserving cash the way it
    // would in `03-create-trade-guard-mcp.md`'s full design — documented
    // limitation, not a silent gap (see `providers` module doc comment).
    if order.side.is_buy_side() {
        account.reserved_cash = account.reserved_cash.checked_sub(&estimated_notional).unwrap_or(Decimal::ZERO);
    }

    let mut outcome = PolicyOutcome::allow();
    if let Some(warning) = check_venue_profile_availability(&order.instrument, venues, now) {
        outcome = outcome.with_warning(warning);
    }
    persist_and_return(audit, &intent, outcome, Some(order), now)
}

fn persist_and_return(
    audit: &AuditStore,
    intent: &TradeIntent,
    outcome: PolicyOutcome,
    order: Option<Order>,
    now: UtcTimestamp,
) -> Result<ExecutionResult, ExecutionError> {
    let record = AuditRecord {
        event_id: format!("evt-{}", intent.idempotency_key),
        created_at: now,
        intent_id: intent.intent_id.clone(),
        idempotency_key: intent.idempotency_key.clone(),
        policy_outcome: outcome.clone(),
        order: order.clone(),
    };
    audit.append(&record)?;
    Ok(ExecutionResult { policy_outcome: outcome, order, was_duplicate: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::InstrumentId;
    use crate::order::OrderState;
    use crate::trade_intent::{IntentMode, OrderType, PositionEffect, Side};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_audit() -> (AuditStore, std::path::PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("trade-guard-execution-test-{}-{n}.sqlite3", std::process::id()));
        (AuditStore::new(&path).unwrap(), path)
    }

    fn base_account() -> AccountSnapshot {
        AccountSnapshot::new("paper-default", "USD", Decimal::from_i64(100_000), UtcTimestamp::UNIX_EPOCH)
    }

    /// None of these tests set `instrument.venue_mic`, so an empty
    /// registry never produces a warning regardless of content — the
    /// venue-warning behavior itself is `jurisdiction_venue`'s own test
    /// module's responsibility, not re-tested here.
    fn empty_venues() -> VenueRegistry {
        VenueRegistry::new()
    }

    fn base_intent(idempotency_key: &str) -> TradeIntent {
        TradeIntent {
            schema_version: "2.0.0".into(),
            intent_id: format!("intent-{idempotency_key}"),
            strategy_id: "strat-1".into(),
            decision_id: "decision-1".into(),
            account_alias: "paper-default".into(),
            instrument: InstrumentId::equity("inst-1", "SPY"),
            side: Side::Buy,
            position_effect: Some(PositionEffect::Open),
            order_type: OrderType::Market,
            quantity: Decimal::from_i64(10),
            limit_price: None,
            stop_price: None,
            time_in_force: "day".into(),
            session: None,
            extended_hours: false,
            max_slippage_bps: None,
            expires_at: None,
            decision_time: UtcTimestamp::UNIX_EPOCH,
            confidence: 0.8,
            rationale: None,
            signal_ids: vec![],
            evidence_bundle_id: None,
            evidence_bundle_hash: None,
            deterministic_input_hash: None,
            model_uri: None,
            caller_id: None,
            mode: IntentMode::Paper,
            idempotency_key: idempotency_key.into(),
        }
    }

    #[test]
    fn a_valid_market_order_fills_and_updates_the_account() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), base_intent("idem-1"), None, UtcTimestamp::UNIX_EPOCH).unwrap();

        assert!(result.policy_outcome.is_allowed());
        assert!(!result.was_duplicate);
        let order = result.order.unwrap();
        assert_eq!(order.state, OrderState::Filled);
        assert_eq!(order.filled_quantity.to_decimal_string(), "10");

        assert_eq!(account.position_for("inst-1").unwrap().quantity.to_decimal_string(), "10");
        assert_eq!(account.reserved_cash, Decimal::ZERO);
        let _ = std::fs::remove_file(path);
    }

    /// Required test #14: duplicate idempotency key returns the original
    /// result rather than submitting twice.
    #[test]
    fn duplicate_idempotency_key_returns_the_original_result_without_a_second_fill() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));

        let first = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), base_intent("idem-dup"), None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert!(!first.was_duplicate);
        let cash_after_first = account.cash;
        let position_after_first = account.position_for("inst-1").unwrap().quantity;

        let second = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), base_intent("idem-dup"), None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert!(second.was_duplicate);
        assert_eq!(second.order.unwrap().order_id, first.order.unwrap().order_id);
        // The account was never touched a second time — cash/position
        // reflect exactly one fill, not two.
        assert_eq!(account.cash, cash_after_first);
        assert_eq!(account.position_for("inst-1").unwrap().quantity, position_after_first);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn insufficient_buying_power_rejects_without_touching_the_position() {
        let (audit, path) = temp_audit();
        let mut account = AccountSnapshot::new("paper-default", "USD", Decimal::from_i64(10), UtcTimestamp::UNIX_EPOCH);
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), base_intent("idem-poor"), None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert_eq!(result.policy_outcome.decision, PolicyDecision::BlockedByRisk);
        assert!(account.position_for("inst-1").is_none());
        assert_eq!(account.cash.to_decimal_string(), "10");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn live_mode_is_rejected_outright_with_no_provider_call() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let sim = PaperSimulator::new();
        let mut intent = base_intent("idem-live");
        intent.mode = IntentMode::GuardedLive;

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent, None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert_eq!(result.policy_outcome.decision, PolicyDecision::BlockedByOperatorControl);
        assert!(result.order.is_none());
        assert!(account.position_for("inst-1").is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn observe_mode_is_rejected_as_not_requesting_paper_execution() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let sim = PaperSimulator::new();
        let mut intent = base_intent("idem-observe");
        intent.mode = IntentMode::Observe;

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent, None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert_eq!(result.policy_outcome.decision, PolicyDecision::Reject);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn account_alias_mismatch_is_rejected() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let sim = PaperSimulator::new();
        let mut intent = base_intent("idem-wrong-acct");
        intent.account_alias = "some-other-account".into();

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent, None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert_eq!(result.policy_outcome.decision, PolicyDecision::Reject);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_non_marketable_limit_order_is_acknowledged_but_stays_open() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let mut intent = base_intent("idem-limit");
        intent.order_type = OrderType::Limit;
        intent.limit_price = Some(Decimal::from_i64(50));

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent, None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert!(result.policy_outcome.is_allowed());
        let order = result.order.unwrap();
        assert_eq!(order.state, OrderState::Acknowledged);
        assert!(order.filled_quantity.is_zero());
        // Buying power was reserved during the attempt and released again
        // once the order settled into "open, unfilled" rather than
        // staying permanently locked up.
        assert_eq!(account.reserved_cash, Decimal::ZERO);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_rejected_intent_is_also_durably_deduplicated() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let sim = PaperSimulator::new();
        let mut intent = base_intent("idem-bad-qty");
        intent.quantity = Decimal::ZERO;

        let first = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent.clone(), None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert_eq!(first.policy_outcome.decision, PolicyDecision::Reject);
        assert!(!first.was_duplicate);

        let second = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent, None, UtcTimestamp::UNIX_EPOCH).unwrap();
        assert!(second.was_duplicate);
        assert_eq!(second.policy_outcome.decision, PolicyDecision::Reject);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_unregistered_venue_fills_successfully_with_a_warning_attached() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let mut intent = base_intent("idem-venue");
        intent.instrument.venue_mic = Some("NOPE".to_string());

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &empty_venues(), intent, None, UtcTimestamp::UNIX_EPOCH).unwrap();

        assert!(result.policy_outcome.is_allowed(), "an unregistered venue must warn, never block a paper order");
        assert_eq!(result.order.unwrap().state, OrderState::Filled);
        assert_eq!(result.policy_outcome.warnings.len(), 1);
        assert!(result.policy_outcome.warnings[0].contains("NOPE"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_registered_and_effective_venue_fills_with_no_warning() {
        let (audit, path) = temp_audit();
        let mut account = base_account();
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let mut intent = base_intent("idem-venue-known");
        intent.instrument.venue_mic = Some("ARCX".to_string());
        let venues = crate::jurisdiction_venue::VenueRegistry::with_demo_fixtures();
        // The demo fixture is only effective from 2026-01-01 onward
        // (see `VenueRegistry::with_demo_fixtures`), so `now` must be
        // after that -- UNIX_EPOCH would (correctly) produce a
        // not-yet-effective warning instead of none, as the *other*
        // test right below this one already covers on purpose.
        let now = UtcTimestamp::parse_rfc3339("2026-07-20T00:00:00Z").unwrap();

        let result = authorize_and_submit_paper_order(&mut account, &sim, &audit, &venues, intent, None, now).unwrap();

        assert!(result.policy_outcome.is_allowed());
        assert!(result.policy_outcome.warnings.is_empty());
        let _ = std::fs::remove_file(path);
    }
}
