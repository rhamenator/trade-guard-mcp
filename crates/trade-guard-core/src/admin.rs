//! Separately authenticated operator surface: live arming lease,
//! risk-limit mutation, credential/provider management, kill switch.
//! Never model-callable.
//!
//! **Status: not-started, deliberately deferred.** This vertical slice
//! implements no live-execution path at all (see `trade_intent::IntentMode::requests_live_execution`
//! and `execution::authorize_and_submit_paper_order`, which rejects any
//! live-mode intent outright) — there is nothing yet for a live-arming
//! lease to gate. Build this module together with the first real
//! (non-paper-simulator) provider adapter, not before.
