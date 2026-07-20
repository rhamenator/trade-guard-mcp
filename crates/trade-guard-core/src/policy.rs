//! Deterministic pre-trade policy: `validate_trade_intent` (schema-shape
//! and sanity checks) and `check_evidence_eligibility` (verifies a signed
//! `EvidenceBundle` from `market-intelligence-mcp` before any intent is
//! authorized — the model cannot waive this check). Buying-power/
//! notional risk checks live in `risk.rs`; this module is the
//! evidence/intent-shape gate that runs before risk is even evaluated.

use crate::account::AccountSnapshot;
use crate::decimal::Decimal;
use crate::evidence::EvidenceBundle;
use crate::policy_decision::{PolicyDecision, PolicyOutcome};
use crate::trade_intent::TradeIntent;
use crate::utc_timestamp::UtcTimestamp;

/// Schema-shape and sanity validation independent of account state,
/// evidence, or risk — a `TradeIntent` that fails this can never become
/// an order regardless of buying power or evidence.
pub fn validate_trade_intent(intent: &TradeIntent, now: UtcTimestamp) -> PolicyOutcome {
    if intent.quantity.is_zero() || intent.quantity.is_negative() {
        return PolicyOutcome::reject(PolicyDecision::Reject, "quantity-must-be-positive");
    }
    if intent.idempotency_key.trim().is_empty() {
        return PolicyOutcome::reject(PolicyDecision::Reject, "idempotency-key-required");
    }
    if !(0.0..=1.0).contains(&intent.confidence) || intent.confidence.is_nan() {
        return PolicyOutcome::reject(PolicyDecision::Reject, "confidence-out-of-range");
    }
    use crate::trade_intent::OrderType;
    match intent.order_type {
        OrderType::Limit | OrderType::LimitOnClose => {
            match &intent.limit_price {
                Some(p) if !p.is_negative() && !p.is_zero() => {}
                _ => return PolicyOutcome::reject(PolicyDecision::Reject, "limit-price-required-for-limit-order"),
            }
        }
        OrderType::Stop => match &intent.stop_price {
            Some(p) if !p.is_negative() && !p.is_zero() => {}
            _ => return PolicyOutcome::reject(PolicyDecision::Reject, "stop-price-required-for-stop-order"),
        },
        OrderType::StopLimit => {
            match &intent.limit_price {
                Some(p) if !p.is_negative() && !p.is_zero() => {}
                _ => return PolicyOutcome::reject(PolicyDecision::Reject, "limit-price-required-for-stop-limit-order"),
            }
            match &intent.stop_price {
                Some(p) if !p.is_negative() && !p.is_zero() => {}
                _ => return PolicyOutcome::reject(PolicyDecision::Reject, "stop-price-required-for-stop-limit-order"),
            }
        }
        _ => {}
    }
    if intent.decision_time.saturating_diff_seconds(&now) > 0 {
        return PolicyOutcome::reject(PolicyDecision::Reject, "decision-time-in-the-future");
    }
    if let Some(expires_at) = &intent.expires_at
        && expires_at.saturating_diff_seconds(&now) < 0
    {
        return PolicyOutcome::reject(PolicyDecision::Expired, "intent-already-expired");
    }
    PolicyOutcome::allow()
}

/// Verifies a signed `EvidenceBundle` before any intent citing it is
/// authorized. `bundle` is `None` when the intent cites no evidence at
/// all (`evidence_bundle_id: null`) — acceptable for paper/observe/
/// advisory modes, never for a live-execution mode.
pub fn check_evidence_eligibility(intent: &TradeIntent, bundle: Option<&EvidenceBundle>, now: UtcTimestamp) -> PolicyOutcome {
    let requires_evidence = intent.mode.requests_live_execution();

    let bundle = match bundle {
        Some(b) => b,
        None => {
            return if requires_evidence {
                PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "live-execution-requires-evidence-bundle")
            } else {
                PolicyOutcome::allow()
            };
        }
    };

    if !bundle.is_internally_consistent() {
        return PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "evidence-bundle-internally-inconsistent");
    }
    if bundle.quarantine_count > 0 {
        return PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "evidence-bundle-has-quarantined-records");
    }
    if bundle.created_at.saturating_diff_seconds(&intent.decision_time) > 0 {
        return PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "evidence-bundle-created-after-decision");
    }
    for record in &bundle.records {
        if let Some(public_time) = &record.public_availability_time
            && public_time.saturating_diff_seconds(&now) > 0
        {
            return PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "evidence-record-future-public-availability-time");
        }
    }

    if requires_evidence && !bundle.is_live_execution_eligible() {
        return PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "evidence-bundle-not-live-execution-eligible");
    }

    PolicyOutcome::allow()
}

/// Requires that `estimated_notional` fits within the account's current
/// buying power. Only a buy-side order actually consumes cash in this
/// vertical slice's model (a sell-side order against an existing long
/// position raises cash rather than requiring it — see
/// `execution::estimate_notional_cost`, which decides which side needs
/// this check in the first place); this function itself is side-agnostic
/// and just compares two amounts.
pub fn check_buying_power(account: &AccountSnapshot, estimated_notional: Decimal) -> PolicyOutcome {
    if estimated_notional > account.buying_power() {
        return PolicyOutcome::reject(PolicyDecision::BlockedByRisk, "insufficient-buying-power");
    }
    PolicyOutcome::allow()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::Decimal;
    use crate::evidence::{BundlePurpose, EvidenceRecordRef};
    use crate::instrument::InstrumentId;
    use crate::legal_status::MnpiClassification;
    use crate::trade_intent::{IntentMode, OrderType, Side};

    fn intent(mode: IntentMode) -> TradeIntent {
        TradeIntent {
            schema_version: "2.0.0".into(),
            intent_id: "intent-1".into(),
            strategy_id: "strat-1".into(),
            decision_id: "decision-1".into(),
            account_alias: "paper-default".into(),
            instrument: InstrumentId::equity("inst-1", "SPY"),
            side: Side::Buy,
            position_effect: None,
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
            mode,
            idempotency_key: "idem-1".into(),
        }
    }

    fn eligible_bundle(purpose: BundlePurpose) -> EvidenceBundle {
        EvidenceBundle {
            evidence_bundle_id: "b1".into(),
            created_at: UtcTimestamp::UNIX_EPOCH,
            decision_time: None,
            records: vec![EvidenceRecordRef {
                source_record_id: "rec-1".into(),
                content_hash: None,
                source_use_decision_id: None,
                mnpi_classification: MnpiClassification::PublicConfirmed,
                public_availability_time: Some(UtcTimestamp::UNIX_EPOCH),
                freshness_seconds: Some(1),
                execution_eligible: true,
            }],
            bundle_purpose: purpose,
            all_records_execution_eligible: true,
            quarantine_count: 0,
            service_attestation: None,
            canonical_hash: "sha256:0".into(),
        }
    }

    #[test]
    fn validate_rejects_nonpositive_quantity() {
        let mut i = intent(IntentMode::Paper);
        i.quantity = Decimal::ZERO;
        assert_eq!(validate_trade_intent(&i, UtcTimestamp::UNIX_EPOCH).decision, PolicyDecision::Reject);
    }

    #[test]
    fn validate_rejects_limit_order_without_limit_price() {
        let mut i = intent(IntentMode::Paper);
        i.order_type = OrderType::Limit;
        assert_eq!(validate_trade_intent(&i, UtcTimestamp::UNIX_EPOCH).decision, PolicyDecision::Reject);
        i.limit_price = Some(Decimal::parse("100").unwrap());
        assert!(validate_trade_intent(&i, UtcTimestamp::UNIX_EPOCH).is_allowed());
    }

    #[test]
    fn validate_rejects_already_expired_intent() {
        let mut i = intent(IntentMode::Paper);
        i.expires_at = Some(UtcTimestamp::UNIX_EPOCH);
        let now = UtcTimestamp::from_unix(1000, 0);
        assert_eq!(validate_trade_intent(&i, now).decision, PolicyDecision::Expired);
    }

    #[test]
    fn validate_rejects_future_decision_time() {
        let mut i = intent(IntentMode::Paper);
        i.decision_time = UtcTimestamp::from_unix(1000, 0);
        assert_eq!(validate_trade_intent(&i, UtcTimestamp::UNIX_EPOCH).decision, PolicyDecision::Reject);
    }

    #[test]
    fn validate_rejects_out_of_range_confidence() {
        let mut i = intent(IntentMode::Paper);
        i.confidence = 1.5;
        assert_eq!(validate_trade_intent(&i, UtcTimestamp::UNIX_EPOCH).decision, PolicyDecision::Reject);
    }

    #[test]
    fn evidence_check_allows_paper_mode_with_no_bundle_at_all() {
        let i = intent(IntentMode::Paper);
        assert!(check_evidence_eligibility(&i, None, UtcTimestamp::UNIX_EPOCH).is_allowed());
    }

    #[test]
    fn evidence_check_rejects_live_mode_with_no_bundle() {
        let i = intent(IntentMode::GuardedLive);
        let outcome = check_evidence_eligibility(&i, None, UtcTimestamp::UNIX_EPOCH);
        assert_eq!(outcome.decision, PolicyDecision::BlockedBySourcePolicy);
    }

    /// Required test #1 from `03-create-trade-guard-mcp.md`: a
    /// research-only bundle is rejected for live.
    #[test]
    fn research_only_bundle_is_rejected_for_live_mode() {
        let i = intent(IntentMode::GuardedLive);
        let bundle = eligible_bundle(BundlePurpose::Research);
        let outcome = check_evidence_eligibility(&i, Some(&bundle), UtcTimestamp::UNIX_EPOCH);
        assert_eq!(outcome.decision, PolicyDecision::BlockedBySourcePolicy);
    }

    #[test]
    fn research_only_bundle_is_allowed_for_paper_mode() {
        let i = intent(IntentMode::Paper);
        let bundle = eligible_bundle(BundlePurpose::Research);
        assert!(check_evidence_eligibility(&i, Some(&bundle), UtcTimestamp::UNIX_EPOCH).is_allowed());
    }

    #[test]
    fn live_execution_purpose_bundle_is_allowed_for_live_mode() {
        let i = intent(IntentMode::GuardedLive);
        let bundle = eligible_bundle(BundlePurpose::LiveExecution);
        assert!(check_evidence_eligibility(&i, Some(&bundle), UtcTimestamp::UNIX_EPOCH).is_allowed());
    }

    #[test]
    fn quarantined_record_blocks_even_paper_mode() {
        let i = intent(IntentMode::Paper);
        let mut bundle = eligible_bundle(BundlePurpose::Research);
        bundle.quarantine_count = 1;
        let outcome = check_evidence_eligibility(&i, Some(&bundle), UtcTimestamp::UNIX_EPOCH);
        assert_eq!(outcome.decision, PolicyDecision::BlockedBySourcePolicy);
    }

    /// Required test #3: a future public-availability timestamp is rejected.
    #[test]
    fn future_public_availability_time_is_rejected() {
        let i = intent(IntentMode::Paper);
        let mut bundle = eligible_bundle(BundlePurpose::Research);
        bundle.records[0].public_availability_time = Some(UtcTimestamp::from_unix(10_000, 0));
        let now = UtcTimestamp::UNIX_EPOCH;
        let outcome = check_evidence_eligibility(&i, Some(&bundle), now);
        assert_eq!(outcome.decision, PolicyDecision::BlockedBySourcePolicy);
    }

    #[test]
    fn bundle_created_after_decision_time_is_rejected() {
        let mut i = intent(IntentMode::Paper);
        i.decision_time = UtcTimestamp::UNIX_EPOCH;
        let mut bundle = eligible_bundle(BundlePurpose::Research);
        bundle.created_at = UtcTimestamp::from_unix(10_000, 0);
        let outcome = check_evidence_eligibility(&i, Some(&bundle), UtcTimestamp::from_unix(20_000, 0));
        assert_eq!(outcome.decision, PolicyDecision::BlockedBySourcePolicy);
    }

    #[test]
    fn buying_power_check_allows_a_notional_within_available_cash() {
        let acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(1000), UtcTimestamp::UNIX_EPOCH);
        assert!(check_buying_power(&acct, Decimal::from_i64(500)).is_allowed());
    }

    #[test]
    fn buying_power_check_rejects_a_notional_exceeding_available_cash() {
        let acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(1000), UtcTimestamp::UNIX_EPOCH);
        let outcome = check_buying_power(&acct, Decimal::from_i64(1001));
        assert_eq!(outcome.decision, PolicyDecision::BlockedByRisk);
    }

    #[test]
    fn buying_power_check_accounts_for_existing_reservations() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(1000), UtcTimestamp::UNIX_EPOCH);
        acct.reserved_cash = Decimal::from_i64(600);
        assert!(check_buying_power(&acct, Decimal::from_i64(400)).is_allowed());
        assert!(!check_buying_power(&acct, Decimal::from_i64(401)).is_allowed());
    }
}
