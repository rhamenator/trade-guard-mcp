//! Metrics, structured logs, trace IDs.
//!
//! **Status: not-started, deliberately deferred.** `audit::AuditStore`
//! already gives this vertical slice a durable, queryable record of every
//! decision (`list-recent-decisions`, `audit-integrity`); a Prometheus/
//! OpenTelemetry pipeline is a real, heavier dependency this stdio-local,
//! single-caller vertical slice does not yet need — see the workspace
//! README "Dependency and testing policy". Revisit once a remote
//! (Streamable HTTP) transport with real concurrent load exists.
