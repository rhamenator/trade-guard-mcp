//! `Position`, `AccountSnapshot` — authoritative account state as this
//! crate's own paper simulator maintains it. There is no external broker
//! yet, so "authoritative" here means "the paper simulator's own ledger",
//! not a real reconciled broker snapshot — see
//! `providers::PaperSimulator`.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::instrument::InstrumentId;
use crate::trade_intent::Side;
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Position {
    pub instrument: InstrumentId,
    /// Signed: positive is long, negative is short.
    pub quantity: Decimal,
    pub average_price: Decimal,
}

impl Position {
    pub fn flat(instrument: InstrumentId) -> Self {
        Position { instrument, quantity: Decimal::ZERO, average_price: Decimal::ZERO }
    }

    pub fn is_flat(&self) -> bool {
        self.quantity.is_zero()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AccountSnapshot {
    pub account_alias: String,
    pub currency: String,
    pub cash: Decimal,
    /// Cash set aside by an in-flight risk reservation
    /// (`execution::reserve_risk_budget`); never negative, never larger
    /// than `cash`.
    pub reserved_cash: Decimal,
    pub positions: Vec<Position>,
    pub as_of: UtcTimestamp,
}

impl AccountSnapshot {
    pub fn new(account_alias: impl Into<String>, currency: impl Into<String>, cash: Decimal, as_of: UtcTimestamp) -> Self {
        AccountSnapshot {
            account_alias: account_alias.into(),
            currency: currency.into(),
            cash,
            reserved_cash: Decimal::ZERO,
            positions: Vec::new(),
            as_of,
        }
    }

    /// Cash actually available to reserve against a new order: total cash
    /// minus whatever is already reserved by other in-flight orders,
    /// clamped to zero rather than going negative (a well-formed account
    /// should never over-reserve, but this keeps the type's invariant —
    /// "buying power" is never a negative number — true even if it does).
    pub fn buying_power(&self) -> Decimal {
        match self.cash.checked_sub(&self.reserved_cash) {
            Some(raw) if !raw.is_negative() => raw,
            _ => Decimal::ZERO,
        }
    }

    pub fn position_for(&self, instrument_id: &str) -> Option<&Position> {
        self.positions.iter().find(|p| p.instrument.instrument_id == instrument_id)
    }

    pub fn position_for_mut(&mut self, instrument_id: &str) -> Option<&mut Position> {
        self.positions.iter_mut().find(|p| p.instrument.instrument_id == instrument_id)
    }

    /// Applies one simulated fill: adjusts cash by the notional
    /// (subtracted for a buy-side fill, added for a sell-side fill) and
    /// updates or creates the position for `instrument`.
    ///
    /// Realized P&L on a position-reducing or direction-flipping fill is
    /// **not** tracked in this vertical slice — a reducing fill keeps the
    /// prior average price, and a fill that flips a position's sign
    /// starts a fresh average price at the fill price for the new
    /// (opposite-sign) remainder. Documented simplification, not a silent
    /// gap: full realized-P&L accounting is future work.
    pub fn apply_fill(&mut self, instrument: &InstrumentId, side: Side, quantity: Decimal, price: Decimal) {
        let notional = price.checked_mul(&quantity).unwrap_or(Decimal::ZERO);
        if side.is_buy_side() {
            self.cash = self.cash.checked_sub(&notional).unwrap_or(self.cash);
        } else {
            self.cash = self.cash.checked_add(&notional).unwrap_or(self.cash);
        }

        let signed_quantity = if side.is_buy_side() { quantity } else { negate(quantity) };
        match self.position_for_mut(&instrument.instrument_id) {
            Some(pos) => apply_signed_fill_to_position(pos, signed_quantity, price),
            None => self.positions.push(Position { instrument: instrument.clone(), quantity: signed_quantity, average_price: price }),
        }
    }
}

fn negate(d: Decimal) -> Decimal {
    Decimal::ZERO.checked_sub(&d).unwrap_or(d)
}

fn apply_signed_fill_to_position(pos: &mut Position, signed_fill_quantity: Decimal, fill_price: Decimal) {
    let same_direction = pos.quantity.is_zero() || (pos.quantity.is_negative() == signed_fill_quantity.is_negative());
    let new_quantity = pos.quantity.checked_add(&signed_fill_quantity).unwrap_or(pos.quantity);

    if same_direction {
        let prior_notional = pos.average_price.checked_mul(&pos.quantity.abs()).unwrap_or(Decimal::ZERO);
        let fill_notional = fill_price.checked_mul(&signed_fill_quantity.abs()).unwrap_or(Decimal::ZERO);
        let new_abs_quantity = new_quantity.abs();
        if !new_abs_quantity.is_zero() {
            pos.average_price = prior_notional.checked_add(&fill_notional).unwrap_or(prior_notional).approx_div(&new_abs_quantity);
        }
    } else if new_quantity.is_zero() {
        pos.average_price = Decimal::ZERO;
    } else if pos.quantity.is_negative() != new_quantity.is_negative() {
        // Sign flipped: the excess became a new position at the fill price.
        pos.average_price = fill_price;
    }
    // else: a partial reduce that doesn't cross zero — average price
    // unchanged (documented above).

    pos.quantity = new_quantity;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buying_power_subtracts_reserved_cash() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(1000), UtcTimestamp::UNIX_EPOCH);
        acct.reserved_cash = Decimal::from_i64(300);
        assert_eq!(acct.buying_power().to_decimal_string(), "700");
    }

    #[test]
    fn buying_power_never_negative_even_if_over_reserved() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(100), UtcTimestamp::UNIX_EPOCH);
        acct.reserved_cash = Decimal::from_i64(500);
        // checked_sub underflows to None here, which we treat as zero
        // buying power rather than propagating a negative "available" cash.
        assert_eq!(acct.buying_power(), Decimal::ZERO);
    }

    #[test]
    fn position_lookup_by_instrument_id() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(1000), UtcTimestamp::UNIX_EPOCH);
        acct.positions.push(Position::flat(InstrumentId::equity("inst-1", "SPY")));
        assert!(acct.position_for("inst-1").is_some());
        assert!(acct.position_for("inst-2").is_none());
    }

    #[test]
    fn flat_position_has_zero_quantity() {
        let pos = Position::flat(InstrumentId::equity("inst-1", "SPY"));
        assert!(pos.is_flat());
    }

    #[test]
    fn buy_fill_reduces_cash_and_opens_a_long_position() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(10_000), UtcTimestamp::UNIX_EPOCH);
        let inst = InstrumentId::equity("inst-1", "SPY");
        acct.apply_fill(&inst, Side::Buy, Decimal::from_i64(10), Decimal::from_i64(100));
        assert_eq!(acct.cash.to_decimal_string(), "9000");
        let pos = acct.position_for("inst-1").unwrap();
        assert_eq!(pos.quantity.to_decimal_string(), "10");
        assert_eq!(pos.average_price.to_decimal_string(), "100");
    }

    #[test]
    fn sell_fill_increases_cash_and_opens_a_short_position() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(10_000), UtcTimestamp::UNIX_EPOCH);
        let inst = InstrumentId::equity("inst-1", "SPY");
        acct.apply_fill(&inst, Side::SellShort, Decimal::from_i64(5), Decimal::from_i64(100));
        assert_eq!(acct.cash.to_decimal_string(), "10500");
        let pos = acct.position_for("inst-1").unwrap();
        assert_eq!(pos.quantity.to_decimal_string(), "-5");
    }

    #[test]
    fn adding_to_a_long_position_computes_weighted_average_price() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(100_000), UtcTimestamp::UNIX_EPOCH);
        let inst = InstrumentId::equity("inst-1", "SPY");
        acct.apply_fill(&inst, Side::Buy, Decimal::from_i64(10), Decimal::from_i64(100));
        acct.apply_fill(&inst, Side::Buy, Decimal::from_i64(10), Decimal::from_i64(102));
        let pos = acct.position_for("inst-1").unwrap();
        assert_eq!(pos.quantity.to_decimal_string(), "20");
        assert_eq!(pos.average_price.to_decimal_string(), "101");
    }

    #[test]
    fn reducing_a_long_position_keeps_the_prior_average_price() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(100_000), UtcTimestamp::UNIX_EPOCH);
        let inst = InstrumentId::equity("inst-1", "SPY");
        acct.apply_fill(&inst, Side::Buy, Decimal::from_i64(10), Decimal::from_i64(100));
        acct.apply_fill(&inst, Side::Sell, Decimal::from_i64(4), Decimal::from_i64(150));
        let pos = acct.position_for("inst-1").unwrap();
        assert_eq!(pos.quantity.to_decimal_string(), "6");
        assert_eq!(pos.average_price.to_decimal_string(), "100");
    }

    #[test]
    fn closing_a_position_exactly_resets_average_price_to_zero() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(100_000), UtcTimestamp::UNIX_EPOCH);
        let inst = InstrumentId::equity("inst-1", "SPY");
        acct.apply_fill(&inst, Side::Buy, Decimal::from_i64(10), Decimal::from_i64(100));
        acct.apply_fill(&inst, Side::Sell, Decimal::from_i64(10), Decimal::from_i64(120));
        let pos = acct.position_for("inst-1").unwrap();
        assert!(pos.is_flat());
        assert_eq!(pos.average_price, Decimal::ZERO);
    }

    #[test]
    fn flipping_a_position_starts_a_fresh_average_price_for_the_remainder() {
        let mut acct = AccountSnapshot::new("acct-1", "USD", Decimal::from_i64(100_000), UtcTimestamp::UNIX_EPOCH);
        let inst = InstrumentId::equity("inst-1", "SPY");
        acct.apply_fill(&inst, Side::Buy, Decimal::from_i64(10), Decimal::from_i64(100));
        // Selling 15 against a 10-long position: closes the long and opens
        // a 5-short remainder.
        acct.apply_fill(&inst, Side::Sell, Decimal::from_i64(15), Decimal::from_i64(110));
        let pos = acct.position_for("inst-1").unwrap();
        assert_eq!(pos.quantity.to_decimal_string(), "-5");
        assert_eq!(pos.average_price.to_decimal_string(), "110");
    }
}
