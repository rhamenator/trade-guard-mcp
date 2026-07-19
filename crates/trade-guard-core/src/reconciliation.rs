//! Reconciliation for ambiguous timeouts, partial fills, and drop-copy
//! confirmation. Never blindly retry a non-idempotent order submission —
//! ambiguous outcomes become a reconciliation state. Status: not-started.
