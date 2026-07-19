//! `trade-guard-core` — authoritative account state, deterministic risk
//! policy, and execution boundary.
//!
//! **Status: not-started.** This repository was created and given a module
//! skeleton in the same session as `market-system-contracts` and
//! `market-intelligence-mcp`, but no implementation effort was invested
//! here yet — see the workspace README "Status" section for why, and
//! `docs/CAPABILITY_STATUS.md` for the exact per-module status.
//!
//! Do not build against any type in this crate. Every module below is a
//! placeholder that reserves the location `03-create-trade-guard-mcp.md`
//! describes, so the next implementation session has a structure to fill
//! in rather than a blank repository.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod execution;
pub mod mcp;
pub mod models;
pub mod policy;
pub mod providers;
pub mod reconciliation;
pub mod risk;
pub mod telemetry;
pub mod tools;
pub mod util;
