//! Reconciliation for ambiguous timeouts, partial fills, and drop-copy
//! confirmation. Never blindly retry a non-idempotent order submission —
//! ambiguous outcomes become `order::OrderState::UnknownPendingReconciliation`.
//!
//! **Status: not-started, deliberately deferred.** `providers::PaperSimulator`
//! is synchronous, local, and always available — it has no network call
//! that can time out ambiguously, so `UnknownPendingReconciliation` is
//! unreachable from this vertical slice's own code (the state exists in
//! `order::OrderState` so a future real provider adapter has somewhere to
//! land). Real reconciliation logic has nothing to reconcile against until
//! a real broker/venue adapter exists — see
//! `06-implementation-order-and-acceptance.md` Phase 6.
