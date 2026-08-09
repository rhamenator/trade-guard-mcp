//! `TradeIntent` — the only object that may become an order. Produced by
//! `smart-dynamic-hedge`, independently re-validated and priced by this
//! crate, which owns final instrument resolution, price refresh, and
//! policy enforcement. Hand-transcribed from
//! `market-system-contracts/schemas/2.0.0/trade-intent.schema.json`.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::instrument::InstrumentId;
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Buy,
    Sell,
    SellShort,
    BuyToCover,
}

impl Side {
    /// `true` for sides that increase or open a long-direction position
    /// (`Buy`, `BuyToCover`); `false` for sides that reduce or open short
    /// (`Sell`, `SellShort`). Used by the paper simulator to decide
    /// whether a fill executes at the simulated ask (buying pressure) or
    /// bid (selling pressure).
    pub fn is_buy_side(self) -> bool {
        matches!(self, Side::Buy | Side::BuyToCover)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionEffect {
    Open,
    Close,
    Reduce,
    Increase,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
    MarketOnClose,
    LimitOnClose,
    Peg,
    TrailingStop,
    MultiLeg,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntentMode {
    Observe,
    Advisory,
    Paper,
    PaperAutonomous,
    GuardedLive,
    GuardedLiveAutonomous,
}

impl IntentMode {
    /// Whether this mode requests any actual paper-simulator side effect
    /// at all — `Observe`/`Advisory` are read-only/preview modes that must
    /// never reach `execution::authorize_and_submit_paper_order`.
    pub fn requests_paper_execution(self) -> bool {
        matches!(self, IntentMode::Paper | IntentMode::PaperAutonomous)
    }

    /// Whether this mode requests live execution. This crate implements
    /// **no live path at all** — every live-mode intent is rejected by
    /// `execution::authorize_and_submit_paper_order` before it reaches
    /// any provider, not merely left untested. See
    /// `docs/CAPABILITY_STATUS.md` for why live execution is out of scope
    /// for this vertical slice.
    pub fn requests_live_execution(self) -> bool {
        matches!(
            self,
            IntentMode::GuardedLive | IntentMode::GuardedLiveAutonomous
        )
    }
}

/// A `TradeIntent` must contain a unique intent ID, strategy/decision IDs,
/// normalized instrument, side, order type, quantity, prices,
/// time-in-force, expiry, maximum slippage, evidence/decision hashes,
/// model identity, caller identity, mode, and idempotency key. Never
/// accept a natural-language-only order — every field here is typed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TradeIntent {
    pub schema_version: String,
    pub intent_id: String,
    pub strategy_id: String,
    pub decision_id: String,
    pub account_alias: String,
    pub instrument: InstrumentId,
    pub side: Side,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_effect: Option<PositionEffect>,
    pub order_type: OrderType,
    pub quantity: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<Decimal>,
    pub time_in_force: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default)]
    pub extended_hours: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_slippage_bps: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UtcTimestamp>,
    pub decision_time: UtcTimestamp,
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub signal_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_bundle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_bundle_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic_input_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_id: Option<String>,
    pub mode: IntentMode,
    pub idempotency_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::InstrumentId;

    fn sample_intent() -> TradeIntent {
        TradeIntent {
            schema_version: "2.0.0".into(),
            intent_id: "intent-1".into(),
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
            idempotency_key: "idem-1".into(),
        }
    }

    #[test]
    fn side_buy_side_classification() {
        assert!(Side::Buy.is_buy_side());
        assert!(Side::BuyToCover.is_buy_side());
        assert!(!Side::Sell.is_buy_side());
        assert!(!Side::SellShort.is_buy_side());
    }

    #[test]
    fn mode_classification_is_mutually_exclusive() {
        for mode in [
            IntentMode::Observe,
            IntentMode::Advisory,
            IntentMode::Paper,
            IntentMode::PaperAutonomous,
            IntentMode::GuardedLive,
            IntentMode::GuardedLiveAutonomous,
        ] {
            assert!(!(mode.requests_paper_execution() && mode.requests_live_execution()));
        }
        assert!(IntentMode::Paper.requests_paper_execution());
        assert!(IntentMode::PaperAutonomous.requests_paper_execution());
        assert!(IntentMode::GuardedLive.requests_live_execution());
        assert!(IntentMode::GuardedLiveAutonomous.requests_live_execution());
        assert!(!IntentMode::Observe.requests_paper_execution());
        assert!(!IntentMode::Observe.requests_live_execution());
    }

    #[test]
    fn serde_round_trips_with_kebab_case_fields() {
        let intent = sample_intent();
        let json = serde_json::to_value(&intent).unwrap();
        assert!(json.get("intent-id").is_some());
        assert!(json.get("idempotency-key").is_some());
        assert_eq!(json["mode"], "paper");
        let back: TradeIntent = serde_json::from_value(json).unwrap();
        assert_eq!(back.intent_id, intent.intent_id);
        assert_eq!(back.quantity, intent.quantity);
    }

    #[test]
    fn missing_optional_fields_deserialize_to_none() {
        let json = r#"{
            "schema-version": "2.0.0",
            "intent-id": "intent-2",
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
            "decision-time": "2026-07-19T00:00:00Z",
            "confidence": 0.8,
            "signal-ids": [],
            "mode": "paper",
            "idempotency-key": "idem-2"
        }"#;
        let intent: TradeIntent = serde_json::from_str(json).unwrap();
        assert!(intent.limit_price.is_none());
        assert!(!intent.extended_hours);
        assert!(intent.evidence_bundle_id.is_none());
    }
}
