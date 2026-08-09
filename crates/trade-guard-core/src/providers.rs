//! The internal paper simulator — mandatory, and the only execution
//! backend this vertical slice implements. No real broker/venue/FIX
//! adapter exists yet; see `docs/CAPABILITY_STATUS.md` and
//! `06-implementation-order-and-acceptance.md` Phase 6 for the deferred
//! `alpaca`/`ibkr`/`saxo`/`oanda`/crypto/FIX/direct-venue work this module
//! intentionally does not attempt.
//!
//! Deliberately simple relative to `03-create-trade-guard-mcp.md`'s full
//! paper-simulator requirements (no resting limit-order book, no
//! latency/slippage model, no multi-currency ledger, no 24/7 session
//! calendar): a market order always fills in full immediately at the
//! simulated quote; a limit order fills in full immediately only if
//! marketable at that instant, otherwise stays open/unfilled — there is
//! no simulated passage of time in which a resting limit could later
//! cross. This is the smallest deterministic simulator that can still
//! support the atomic authorize-and-submit protocol correctly and is
//! documented as a known limitation, not a silent gap.

use std::collections::HashMap;

use crate::decimal::Decimal;
use crate::order::Order;
use crate::sha256::sha256;
use crate::trade_intent::OrderType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    pub bid: Decimal,
    pub ask: Decimal,
}

impl Quote {
    fn around_mid(mid: Decimal, half_spread: Decimal) -> Self {
        Quote {
            bid: mid.checked_sub(&half_spread).unwrap_or(mid),
            ask: mid.checked_add(&half_spread).unwrap_or(mid),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulatedFillOutcome {
    /// Fully filled immediately at `price`.
    Filled { price: Decimal },
    /// Acknowledged but not marketable at the current quote — stays open.
    Open,
}

/// Deterministic, dependency-free, in-memory paper simulator. Every
/// instrument not explicitly seeded via `set_quote` gets a stable
/// synthetic quote derived from hashing its `instrument_id`, so the same
/// symbol always produces the same fixture price across a process restart
/// — no persisted state needed for the quote source itself (only orders/
/// fills/positions are persisted, by `audit::AuditStore`).
pub struct PaperSimulator {
    quotes: HashMap<String, Decimal>,
    half_spread: Decimal,
}

impl Default for PaperSimulator {
    fn default() -> Self {
        Self::new()
    }
}

impl PaperSimulator {
    pub fn new() -> Self {
        PaperSimulator {
            quotes: HashMap::new(),
            half_spread: Decimal::parse("0.005").unwrap(),
        }
    }

    /// Deterministic test/fixture seam: pin an explicit mid price for an
    /// instrument, overriding the synthetic hash-derived default.
    pub fn set_quote(&mut self, instrument_id: &str, mid: Decimal) {
        self.quotes.insert(instrument_id.to_string(), mid);
    }

    pub fn quote_for(&self, instrument_id: &str) -> Quote {
        let mid = self
            .quotes
            .get(instrument_id)
            .copied()
            .unwrap_or_else(|| synthetic_mid(instrument_id));
        Quote::around_mid(mid, self.half_spread)
    }

    /// Simulates the fill outcome for `order` against the current quote.
    /// Never mutates `order`; the caller (`execution::authorize_and_submit_paper_order`)
    /// is responsible for applying the returned fill.
    pub fn simulate_fill(&self, order: &Order) -> SimulatedFillOutcome {
        let quote = self.quote_for(&order.instrument.instrument_id);
        match order.order_type {
            OrderType::Market | OrderType::MarketOnClose => {
                let price = if order.side.is_buy_side() {
                    quote.ask
                } else {
                    quote.bid
                };
                SimulatedFillOutcome::Filled { price }
            }
            OrderType::Limit | OrderType::LimitOnClose => {
                let Some(limit_price) = order.limit_price else {
                    return SimulatedFillOutcome::Open;
                };
                if order.side.is_buy_side() {
                    if limit_price >= quote.ask {
                        SimulatedFillOutcome::Filled { price: quote.ask }
                    } else {
                        SimulatedFillOutcome::Open
                    }
                } else if limit_price <= quote.bid {
                    SimulatedFillOutcome::Filled { price: quote.bid }
                } else {
                    SimulatedFillOutcome::Open
                }
            }
            // Stop, stop-limit, peg, trailing-stop, multi-leg, other:
            // none of these can be evaluated against a single instant
            // quote without a simulated price path over time, which this
            // simulator does not model. Acknowledged but never filled by
            // this vertical slice — a real limitation, not a bug.
            _ => SimulatedFillOutcome::Open,
        }
    }
}

/// A stable, deterministic synthetic mid price in the `$50.00`-`$250.00`
/// range, derived from `sha256(instrument_id)` — the same "always
/// available, changing test data" spirit as `smart-dynamic-hedge`'s
/// `SyntheticProvider`, so unfamiliar test/demo symbols still get a
/// sensible, reproducible quote instead of an error.
fn synthetic_mid(instrument_id: &str) -> Decimal {
    let digest = sha256(instrument_id.as_bytes());
    let seed = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let cents = 5_000 + (seed % 20_000);
    Decimal::parse(&format!("{}.{:02}", cents / 100, cents % 100)).unwrap_or(Decimal::from_i64(100))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::InstrumentId;
    use crate::order::OrderState;
    use crate::trade_intent::Side;
    use crate::utc_timestamp::UtcTimestamp;

    fn order(order_type: OrderType, side: Side, limit_price: Option<Decimal>) -> Order {
        Order {
            order_id: "order-1".into(),
            intent_id: "intent-1".into(),
            idempotency_key: "idem-1".into(),
            account_alias: "acct-1".into(),
            instrument: InstrumentId::equity("inst-1", "SPY"),
            side,
            order_type,
            quantity: Decimal::from_i64(10),
            limit_price,
            state: OrderState::RiskReserved,
            filled_quantity: Decimal::ZERO,
            average_fill_price: None,
            fills: Vec::new(),
            created_at: UtcTimestamp::UNIX_EPOCH,
            updated_at: UtcTimestamp::UNIX_EPOCH,
        }
    }

    #[test]
    fn synthetic_quote_is_deterministic_across_calls() {
        let sim = PaperSimulator::new();
        let q1 = sim.quote_for("inst-unset");
        let q2 = sim.quote_for("inst-unset");
        assert_eq!(q1, q2);
        assert!(q1.ask > q1.bid);
    }

    #[test]
    fn different_instruments_can_get_different_synthetic_quotes() {
        let sim = PaperSimulator::new();
        let a = sim.quote_for("inst-a");
        let b = sim.quote_for("inst-b");
        // Not a strict guarantee for arbitrary hash collisions, but true
        // for these two fixed literal strings, and demonstrates the quote
        // is a function of the id rather than a single global constant.
        assert_ne!(a.bid, b.bid);
    }

    #[test]
    fn set_quote_overrides_the_synthetic_default() {
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let q = sim.quote_for("inst-1");
        assert_eq!(q.bid.to_decimal_string(), "99.995");
        assert_eq!(q.ask.to_decimal_string(), "100.005");
    }

    #[test]
    fn market_buy_fills_at_ask() {
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let o = order(OrderType::Market, Side::Buy, None);
        match sim.simulate_fill(&o) {
            SimulatedFillOutcome::Filled { price } => {
                assert_eq!(price.to_decimal_string(), "100.005")
            }
            SimulatedFillOutcome::Open => panic!("expected a fill"),
        }
    }

    #[test]
    fn market_sell_fills_at_bid() {
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let o = order(OrderType::Market, Side::Sell, None);
        match sim.simulate_fill(&o) {
            SimulatedFillOutcome::Filled { price } => {
                assert_eq!(price.to_decimal_string(), "99.995")
            }
            SimulatedFillOutcome::Open => panic!("expected a fill"),
        }
    }

    #[test]
    fn marketable_limit_buy_fills_at_ask() {
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let o = order(OrderType::Limit, Side::Buy, Some(Decimal::from_i64(101)));
        match sim.simulate_fill(&o) {
            SimulatedFillOutcome::Filled { price } => {
                assert_eq!(price.to_decimal_string(), "100.005")
            }
            SimulatedFillOutcome::Open => panic!("expected a fill"),
        }
    }

    #[test]
    fn non_marketable_limit_buy_stays_open() {
        let mut sim = PaperSimulator::new();
        sim.set_quote("inst-1", Decimal::from_i64(100));
        let o = order(OrderType::Limit, Side::Buy, Some(Decimal::from_i64(50)));
        assert_eq!(sim.simulate_fill(&o), SimulatedFillOutcome::Open);
    }

    #[test]
    fn limit_order_without_a_limit_price_stays_open_rather_than_panicking() {
        let sim = PaperSimulator::new();
        let o = order(OrderType::Limit, Side::Buy, None);
        assert_eq!(sim.simulate_fill(&o), SimulatedFillOutcome::Open);
    }

    #[test]
    fn stop_order_is_acknowledged_but_never_filled_by_this_simulator() {
        let sim = PaperSimulator::new();
        let o = order(OrderType::Stop, Side::Buy, None);
        assert_eq!(sim.simulate_fill(&o), SimulatedFillOutcome::Open);
    }
}
