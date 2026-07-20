//! `trade-guard-core` — authoritative account state, deterministic risk
//! policy, and execution boundary.
//!
//! **Status: paper-only vertical slice implemented.** Per
//! `06-implementation-order-and-acceptance.md` Phase 3 and this crate's
//! own `docs/CAPABILITY_STATUS.md`: typed `TradeIntent`/`EvidenceBundle`
//! contracts, `check-evidence-eligibility`, buying-power policy, and the
//! atomic `authorize-and-submit-paper-order` protocol against an internal
//! deterministic paper simulator, backed by a hash-chained SQLite audit
//! store, exposed over a hand-rolled MCP stdio JSON-RPC transport. No
//! live-execution path, no real broker/venue/FIX adapter, no market-abuse
//! surveillance, no remote transport, no operator-admin surface — every
//! one of those is a documented, deliberate scope cut (see each deferred
//! module's own doc comment), not a silent gap.

pub mod account;
pub mod admin;
pub mod audit;
pub mod auth;
pub mod decimal;
pub mod evidence;
pub mod execution;
pub mod instrument;
pub mod legal_status;
pub mod mcp;
pub mod order;
pub mod policy;
pub mod policy_decision;
pub mod providers;
pub mod reconciliation;
pub mod risk;
pub mod sha256;
pub mod state;
pub mod telemetry;
pub mod tools;
pub mod trade_intent;
pub mod utc_timestamp;
