//! Market-abuse/surveillance layer: self-trade, wash-trading, spoofing/
//! layering, quote-stuffing, marking-the-close detection.
//!
//! **Status: not-started, deliberately deferred.** This vertical slice
//! (see `docs/CAPABILITY_STATUS.md`) implements the atomic
//! authorize-and-submit-paper-order protocol, evidence eligibility, and a
//! buying-power check (`policy::check_buying_power`) — the smallest
//! trustworthy complete path `03-create-trade-guard-mcp.md`'s own closing
//! line asks for. Behavioral surveillance across multiple orders (self-
//! trade/wash-trading/spoofing pattern detection) meaningfully requires
//! order history correlation and account-relationship data this slice
//! does not yet have reason to build — a single caller submitting to a
//! single in-memory paper account has no other account to wash-trade
//! against yet. Revisit once multiple accounts/strategies are real.
