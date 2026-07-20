//! Append-only, hash-chained audit log. SQLite by default (per the
//! baseline spec embedded in `03-create-trade-guard-mcp.md` — Redis would
//! be cache/lease/rate-limit only, never the sole audit store, and this
//! vertical slice has no Redis at all). The one deliberate dependency
//! exception in this crate: the SQLite file format (WAL, B-tree pages,
//! journal recovery) is exactly the kind of complex, correctness-critical
//! format that is a *worse* trade-off to hand-roll than to depend on —
//! same reasoning `smart-dynamic-hedge`'s `smart-hedge-store` crate
//! documents for its own `rusqlite` dependency.
//!
//! Every `authorize_and_submit_paper_order` call appends exactly one
//! `AuditRecord`, chained to the previous record's content hash. This
//! gives two independent tamper-evidence properties on `verify_integrity`:
//! a modified record's own `content_hash` stops matching its stored
//! payload, and a deleted/reordered record breaks the *next* record's
//! `prev_hash` link — catching row deletion, not just row modification.
//!
//! The `idempotency_key` column carries a `UNIQUE` constraint as
//! defense-in-depth; the actual duplicate-submission guarantee is
//! `execution::authorize_and_submit_paper_order` checking
//! `find_by_idempotency_key` *before* ever calling `append`, not this
//! constraint catching a race after the fact (MCP stdio here is single
//! request-at-a-time, so there is no real concurrent-race window today).

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::order::Order;
use crate::policy_decision::PolicyOutcome;
use crate::sha256::canonical_hash;
use crate::utc_timestamp::UtcTimestamp;

/// A fixed, documented anchor for the first record's `prev_hash` — not a
/// secret, just a well-known non-record value so the chain has a
/// consistent starting point to verify against.
pub const GENESIS_HASH: &str = "sha256:e69ba0ea583c19f5b7d6c3d8ddc0e2a2f1e1f8b7e77e5f7f7c1c25b4c1f3a2e5";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AuditRecord {
    pub event_id: String,
    pub created_at: UtcTimestamp,
    pub intent_id: String,
    pub idempotency_key: String,
    pub policy_outcome: PolicyOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Order>,
}

#[derive(Debug)]
pub enum AuditError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    InvalidJson(String),
    DuplicateIdempotencyKey(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::Sqlite(e) => write!(f, "sqlite error: {e}"),
            AuditError::Io(e) => write!(f, "io error: {e}"),
            AuditError::InvalidJson(e) => write!(f, "invalid stored JSON: {e}"),
            AuditError::DuplicateIdempotencyKey(k) => write!(f, "idempotency key already recorded: {k}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<rusqlite::Error> for AuditError {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(err, Some(msg)) = &e
            && err.code == rusqlite::ErrorCode::ConstraintViolation
            && msg.contains("idempotency_key")
        {
            return AuditError::DuplicateIdempotencyKey(msg.clone());
        }
        AuditError::Sqlite(e)
    }
}

impl From<std::io::Error> for AuditError {
    fn from(e: std::io::Error) -> Self {
        AuditError::Io(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityFailure {
    /// The record at this sequence number's stored `content_hash` no
    /// longer matches its own payload+prev_hash — its payload was edited.
    PayloadTampered { seq: i64, event_id: String },
    /// The record at this sequence number's `prev_hash` does not match
    /// the actual previous record's `content_hash` — a record was
    /// deleted, inserted, or reordered.
    ChainBroken { seq: i64, event_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    pub records_checked: u64,
    pub failure: Option<IntegrityFailure>,
}

impl IntegrityReport {
    pub fn is_valid(&self) -> bool {
        self.failure.is_none()
    }
}

pub struct AuditStore {
    path: PathBuf,
}

impl AuditStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let store = AuditStore { path };
        store.initialize()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> Result<Connection, AuditError> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }

    fn initialize(&self) -> Result<(), AuditError> {
        let conn = self.connect()?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT UNIQUE NOT NULL,
                idempotency_key TEXT UNIQUE NOT NULL,
                created_at TEXT NOT NULL,
                record_json TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                content_hash TEXT NOT NULL
            )",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at DESC)", [])?;
        Ok(())
    }

    fn last_content_hash(conn: &Connection) -> Result<String, AuditError> {
        let hash: Option<String> = conn
            .query_row("SELECT content_hash FROM events ORDER BY seq DESC LIMIT 1", [], |r| r.get(0))
            .optional()?;
        Ok(hash.unwrap_or_else(|| GENESIS_HASH.to_string()))
    }

    /// Appends one hash-chained record. Fails with
    /// `AuditError::DuplicateIdempotencyKey` if this key was already
    /// recorded — callers should check `find_by_idempotency_key` first to
    /// treat that case as "return the original result", not as an error
    /// path to surface to the caller.
    pub fn append(&self, record: &AuditRecord) -> Result<String, AuditError> {
        let record_json = serde_json::to_string(record).expect("AuditRecord serialization is infallible");
        let conn = self.connect()?;
        let prev_hash = Self::last_content_hash(&conn)?;
        let content_hash = canonical_hash(format!("{record_json}|{prev_hash}").as_bytes());

        conn.execute(
            "INSERT INTO events(event_id, idempotency_key, created_at, record_json, prev_hash, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.event_id,
                record.idempotency_key,
                record.created_at.to_rfc3339(),
                record_json,
                prev_hash,
                content_hash
            ],
        )?;
        Ok(content_hash)
    }

    fn parse_row(record_json: String) -> Result<AuditRecord, AuditError> {
        serde_json::from_str(&record_json).map_err(|e| AuditError::InvalidJson(e.to_string()))
    }

    pub fn find_by_idempotency_key(&self, key: &str) -> Result<Option<AuditRecord>, AuditError> {
        let conn = self.connect()?;
        let row: Option<String> = conn
            .query_row("SELECT record_json FROM events WHERE idempotency_key = ?1", params![key], |r| r.get(0))
            .optional()?;
        row.map(Self::parse_row).transpose()
    }

    pub fn get(&self, event_id: &str) -> Result<Option<AuditRecord>, AuditError> {
        let conn = self.connect()?;
        let row: Option<String> = conn
            .query_row("SELECT record_json FROM events WHERE event_id = ?1", params![event_id], |r| r.get(0))
            .optional()?;
        row.map(Self::parse_row).transpose()
    }

    /// `limit` is clamped to `[1, 200]`, the same convention
    /// `smart-dynamic-hedge`'s `DecisionStore::recent` uses.
    pub fn list_recent(&self, limit: i64) -> Result<Vec<AuditRecord>, AuditError> {
        let clamped = limit.clamp(1, 200);
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT record_json FROM events ORDER BY seq DESC LIMIT ?1")?;
        let rows: Vec<String> = stmt.query_map(params![clamped], |r| r.get(0))?.collect::<Result<_, _>>()?;
        rows.into_iter().map(Self::parse_row).collect()
    }

    /// Walks every record in insertion order, recomputing each
    /// `content_hash` from its stored payload and `prev_hash`, and
    /// confirming each record's `prev_hash` matches the previous record's
    /// (recomputed, not merely stored) `content_hash`. Returns the first
    /// failure found, if any — a hash chain's whole point is that one
    /// break invalidates everything after it, so there is no value in
    /// continuing to report further "failures" past the first real one.
    pub fn verify_integrity(&self) -> Result<IntegrityReport, AuditError> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT seq, event_id, record_json, prev_hash, content_hash FROM events ORDER BY seq ASC")?;
        let rows: Vec<(i64, String, String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<Result<_, _>>()?;

        let mut expected_prev = GENESIS_HASH.to_string();
        let mut checked = 0u64;
        for (seq, event_id, record_json, prev_hash, stored_content_hash) in rows {
            if prev_hash != expected_prev {
                return Ok(IntegrityReport { records_checked: checked, failure: Some(IntegrityFailure::ChainBroken { seq, event_id }) });
            }
            let recomputed = canonical_hash(format!("{record_json}|{prev_hash}").as_bytes());
            if recomputed != stored_content_hash {
                return Ok(IntegrityReport { records_checked: checked, failure: Some(IntegrityFailure::PayloadTampered { seq, event_id }) });
            }
            expected_prev = stored_content_hash;
            checked += 1;
        }
        Ok(IntegrityReport { records_checked: checked, failure: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_decision::PolicyDecision;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_store() -> (AuditStore, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("trade-guard-audit-test-{}-{n}.sqlite3", std::process::id()));
        let store = AuditStore::new(&path).unwrap();
        (store, path)
    }

    fn record(idempotency_key: &str) -> AuditRecord {
        AuditRecord {
            event_id: format!("evt-{idempotency_key}"),
            created_at: UtcTimestamp::UNIX_EPOCH,
            intent_id: "intent-1".into(),
            idempotency_key: idempotency_key.into(),
            policy_outcome: PolicyOutcome::allow(),
            order: None,
        }
    }

    #[test]
    fn append_then_find_by_idempotency_key_round_trips() {
        let (store, path) = temp_store();
        store.append(&record("idem-1")).unwrap();
        let found = store.find_by_idempotency_key("idem-1").unwrap().unwrap();
        assert_eq!(found.event_id, "evt-idem-1");
        assert!(store.find_by_idempotency_key("idem-missing").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn duplicate_idempotency_key_is_rejected_at_the_storage_layer() {
        let (store, path) = temp_store();
        store.append(&record("idem-1")).unwrap();
        let result = store.append(&record("idem-1"));
        assert!(matches!(result, Err(AuditError::DuplicateIdempotencyKey(_))));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn list_recent_returns_newest_first() {
        let (store, path) = temp_store();
        store.append(&record("idem-1")).unwrap();
        store.append(&record("idem-2")).unwrap();
        store.append(&record("idem-3")).unwrap();
        let recent = store.list_recent(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].idempotency_key, "idem-3");
        assert_eq!(recent[2].idempotency_key, "idem-1");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_store_has_valid_integrity() {
        let (store, path) = temp_store();
        let report = store.verify_integrity().unwrap();
        assert!(report.is_valid());
        assert_eq!(report.records_checked, 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_freshly_appended_chain_verifies_as_valid() {
        let (store, path) = temp_store();
        for i in 0..5 {
            store.append(&record(&format!("idem-{i}"))).unwrap();
        }
        let report = store.verify_integrity().unwrap();
        assert!(report.is_valid());
        assert_eq!(report.records_checked, 5);
        let _ = std::fs::remove_file(path);
    }

    /// Required test #22 (`03-create-trade-guard-mcp.md`): audit
    /// hash-chain verification detects tampering. Directly corrupts a
    /// stored row's payload via raw SQL — the same tamper-simulation
    /// technique `smart-dynamic-hedge`'s store integration test uses.
    #[test]
    fn tampering_with_a_stored_payload_is_detected() {
        let (store, path) = temp_store();
        store.append(&record("idem-1")).unwrap();
        store.append(&record("idem-2")).unwrap();

        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE events SET record_json = '{\"tampered\": true}' WHERE idempotency_key = 'idem-1'",
            [],
        )
        .unwrap();

        let report = store.verify_integrity().unwrap();
        assert!(!report.is_valid());
        assert!(matches!(report.failure, Some(IntegrityFailure::PayloadTampered { .. })));
        let _ = std::fs::remove_file(path);
    }

    /// Deleting a record (rather than editing one) breaks the *next*
    /// record's chain link instead — a different failure mode this test
    /// exercises separately from payload tampering.
    #[test]
    fn deleting_a_record_breaks_the_chain_for_the_next_one() {
        let (store, path) = temp_store();
        store.append(&record("idem-1")).unwrap();
        store.append(&record("idem-2")).unwrap();
        store.append(&record("idem-3")).unwrap();

        let conn = Connection::open(&path).unwrap();
        conn.execute("DELETE FROM events WHERE idempotency_key = 'idem-2'", []).unwrap();

        let report = store.verify_integrity().unwrap();
        assert!(!report.is_valid());
        assert!(matches!(report.failure, Some(IntegrityFailure::ChainBroken { .. })));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn get_by_event_id_returns_none_for_unknown_id() {
        let (store, path) = temp_store();
        assert!(store.get("nonexistent").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_with_a_rejection_and_no_order_round_trips() {
        let (store, path) = temp_store();
        let mut r = record("idem-1");
        r.policy_outcome = PolicyOutcome::reject(PolicyDecision::BlockedByRisk, "over-buying-power");
        store.append(&r).unwrap();
        let found = store.find_by_idempotency_key("idem-1").unwrap().unwrap();
        assert_eq!(found.policy_outcome.decision, PolicyDecision::BlockedByRisk);
        assert!(found.order.is_none());
        let _ = std::fs::remove_file(path);
    }
}
