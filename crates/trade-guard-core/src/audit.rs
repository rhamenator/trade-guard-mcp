//! Append-only, hash-chained audit log. SQLite by default, PostgreSQL
//! optional (per the baseline spec embedded in
//! `03-create-trade-guard-mcp.md` — Redis is cache/lease/rate-limit only,
//! never the sole audit store). Status: not-started.
