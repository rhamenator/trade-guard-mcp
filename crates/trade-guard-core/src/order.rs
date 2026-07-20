//! Event-sourced order state and `Fill` records for the paper simulator.
//! A subset of `03-create-trade-guard-mcp.md`'s full state machine — this
//! vertical slice only ever reaches the states a synchronous, local,
//! always-available paper simulator can actually produce (see
//! `providers::PaperSimulator`); states like `UnknownPendingReconciliation`
//! exist in the enum (so future real-provider work has somewhere to land)
//! but are unreachable from this crate's own code today, which is
//! documented rather than silently true.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::instrument::InstrumentId;
use crate::trade_intent::{OrderType, Side};
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderState {
    IntentReceived,
    Rejected,
    Validated,
    RiskReserved,
    Acknowledged,
    PartiallyFilled,
    Filled,
    CancelPending,
    Canceled,
    Expired,
    ProviderRejected,
    /// Reachable only once a real (non-paper-simulator) provider exists —
    /// see the module doc comment above.
    UnknownPendingReconciliation,
    ManualReview,
}

impl OrderState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OrderState::Rejected | OrderState::Filled | OrderState::Canceled | OrderState::Expired | OrderState::ProviderRejected
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Fill {
    pub fill_id: String,
    pub order_id: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub filled_at: UtcTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Order {
    pub order_id: String,
    pub intent_id: String,
    pub idempotency_key: String,
    pub account_alias: String,
    pub instrument: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<Decimal>,
    pub state: OrderState,
    pub filled_quantity: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_fill_price: Option<Decimal>,
    pub fills: Vec<Fill>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

impl Order {
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Records one fill against this order, updating `filled_quantity`
    /// and the volume-weighted `average_fill_price`, and transitions
    /// `state` to `PartiallyFilled` or `Filled` depending on whether the
    /// full requested quantity has now been filled.
    pub fn apply_fill(&mut self, fill: Fill) {
        let prior_notional = self.average_fill_price.unwrap_or(Decimal::ZERO).checked_mul(&self.filled_quantity).unwrap_or(Decimal::ZERO);
        let fill_notional = fill.price.checked_mul(&fill.quantity).unwrap_or(Decimal::ZERO);
        let new_filled = self.filled_quantity.checked_add(&fill.quantity).unwrap_or(self.filled_quantity);
        if !new_filled.is_zero() {
            let total_notional = prior_notional.checked_add(&fill_notional).unwrap_or(prior_notional);
            self.average_fill_price = Some(total_notional.approx_div(&new_filled));
        }
        self.filled_quantity = new_filled;
        self.updated_at = fill.filled_at;
        self.fills.push(fill);
        self.state = if self.filled_quantity >= self.quantity { OrderState::Filled } else { OrderState::PartiallyFilled };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::InstrumentId;

    fn base_order() -> Order {
        Order {
            order_id: "order-1".into(),
            intent_id: "intent-1".into(),
            idempotency_key: "idem-1".into(),
            account_alias: "acct-1".into(),
            instrument: InstrumentId::equity("inst-1", "SPY"),
            side: Side::Buy,
            order_type: OrderType::Market,
            quantity: Decimal::from_i64(10),
            limit_price: None,
            state: OrderState::RiskReserved,
            filled_quantity: Decimal::ZERO,
            average_fill_price: None,
            fills: Vec::new(),
            created_at: UtcTimestamp::UNIX_EPOCH,
            updated_at: UtcTimestamp::UNIX_EPOCH,
        }
    }

    #[test]
    fn full_fill_transitions_to_filled() {
        let mut order = base_order();
        order.apply_fill(Fill {
            fill_id: "fill-1".into(),
            order_id: "order-1".into(),
            quantity: Decimal::from_i64(10),
            price: Decimal::parse("100").unwrap(),
            filled_at: UtcTimestamp::UNIX_EPOCH,
        });
        assert_eq!(order.state, OrderState::Filled);
        assert_eq!(order.filled_quantity.to_decimal_string(), "10");
        assert_eq!(order.average_fill_price.unwrap().to_decimal_string(), "100");
    }

    #[test]
    fn partial_fill_transitions_to_partially_filled() {
        let mut order = base_order();
        order.apply_fill(Fill {
            fill_id: "fill-1".into(),
            order_id: "order-1".into(),
            quantity: Decimal::from_i64(4),
            price: Decimal::parse("100").unwrap(),
            filled_at: UtcTimestamp::UNIX_EPOCH,
        });
        assert_eq!(order.state, OrderState::PartiallyFilled);
        assert!(!order.is_terminal());
    }

    #[test]
    fn two_partial_fills_compute_volume_weighted_average_price() {
        let mut order = base_order();
        order.apply_fill(Fill {
            fill_id: "fill-1".into(),
            order_id: "order-1".into(),
            quantity: Decimal::from_i64(5),
            price: Decimal::parse("100").unwrap(),
            filled_at: UtcTimestamp::UNIX_EPOCH,
        });
        order.apply_fill(Fill {
            fill_id: "fill-2".into(),
            order_id: "order-1".into(),
            quantity: Decimal::from_i64(5),
            price: Decimal::parse("102").unwrap(),
            filled_at: UtcTimestamp::UNIX_EPOCH,
        });
        assert_eq!(order.state, OrderState::Filled);
        // (5*100 + 5*102) / 10 = 101
        assert_eq!(order.average_fill_price.unwrap().to_decimal_string(), "101");
    }

    #[test]
    fn terminal_states_are_classified_correctly() {
        for state in [OrderState::Rejected, OrderState::Filled, OrderState::Canceled, OrderState::Expired, OrderState::ProviderRejected] {
            assert!(state.is_terminal(), "{state:?} should be terminal");
        }
        for state in [OrderState::IntentReceived, OrderState::Validated, OrderState::RiskReserved, OrderState::Acknowledged, OrderState::PartiallyFilled] {
            assert!(!state.is_terminal(), "{state:?} should not be terminal");
        }
    }
}
