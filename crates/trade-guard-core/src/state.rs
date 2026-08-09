//! `GuardState` — bundles the in-memory paper account, the deterministic
//! paper simulator, the venue-profile registry, and the durable audit
//! store into the one piece of state `tools.rs`/`mcp.rs` operate on.
//! There is no concurrency to coordinate here: the stdio transport
//! processes one JSON-RPC line at a time (see `mcp::run_stdio`), so plain
//! `&mut GuardState` is sufficient — no `Mutex`/`RwLock` needed for this
//! vertical slice.

use std::path::Path;

use crate::account::AccountSnapshot;
use crate::audit::{AuditError, AuditStore};
use crate::jurisdiction_venue::VenueRegistry;
use crate::providers::PaperSimulator;

pub struct GuardState {
    pub account: AccountSnapshot,
    pub simulator: PaperSimulator,
    pub audit: AuditStore,
    pub venues: VenueRegistry,
}

impl GuardState {
    pub fn new(
        account: AccountSnapshot,
        audit_db_path: impl AsRef<Path>,
    ) -> Result<Self, AuditError> {
        Ok(GuardState {
            account,
            simulator: PaperSimulator::new(),
            audit: AuditStore::new(audit_db_path)?,
            venues: VenueRegistry::with_demo_fixtures(),
        })
    }
}
