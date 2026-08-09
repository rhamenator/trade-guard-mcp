//! `EvidenceBundle` — read-only consumer transcription from
//! `market-system-contracts/schemas/2.0.0/evidence-bundle.schema.json`,
//! mirroring `market_intelligence_core::evidence` (that crate produces
//! these; this crate only ever verifies one it receives, never
//! constructs one for its own purposes). See `policy::check_evidence_eligibility`
//! for the actual gate this type exists to support.

use serde::{Deserialize, Serialize};

use crate::legal_status::MnpiClassification;
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BundlePurpose {
    Research,
    Advisory,
    PaperExecution,
    LiveExecution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EvidenceRecordRef {
    pub source_record_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_use_decision_id: Option<String>,
    pub mnpi_classification: MnpiClassification,
    #[serde(default)]
    pub public_availability_time: Option<UtcTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_seconds: Option<i64>,
    pub execution_eligible: bool,
}

/// Immutable, signed/attested bundle of evidence backing a `TradeIntent`.
/// This crate independently verifies this before any execution intent is
/// authorized — the model cannot waive `check_evidence_eligibility`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EvidenceBundle {
    pub evidence_bundle_id: String,
    pub created_at: UtcTimestamp,
    #[serde(default)]
    pub decision_time: Option<UtcTimestamp>,
    pub records: Vec<EvidenceRecordRef>,
    pub bundle_purpose: BundlePurpose,
    pub all_records_execution_eligible: bool,
    pub quarantine_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_attestation: Option<String>,
    pub canonical_hash: String,
}

impl EvidenceBundle {
    /// A bundle may back a live-execution intent only if it was built for
    /// that purpose, contains zero quarantined records, and every record
    /// is independently execution-eligible — mirrors the identical check
    /// `market_intelligence_core::evidence::EvidenceBundle` performs on
    /// the producing side, so a bundle that would fail here should never
    /// have been emitted in the first place.
    pub fn is_live_execution_eligible(&self) -> bool {
        self.bundle_purpose == BundlePurpose::LiveExecution
            && self.all_records_execution_eligible
            && self.quarantine_count == 0
            && !self.records.is_empty()
            && self.records.iter().all(|r| r.execution_eligible)
    }

    /// A looser bar for paper/research/advisory use: any declared purpose
    /// is acceptable, but zero quarantined records and internal
    /// consistency (`all_records_execution_eligible` matching the actual
    /// per-record flags) are still required — paper mode is a rehearsal
    /// for live mode, not a place to silently tolerate an inconsistent
    /// bundle.
    pub fn is_internally_consistent(&self) -> bool {
        self.quarantine_count == 0
            && self.all_records_execution_eligible
                == self.records.iter().all(|r| r.execution_eligible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eligible_record() -> EvidenceRecordRef {
        EvidenceRecordRef {
            source_record_id: "rec-1".into(),
            content_hash: None,
            source_use_decision_id: None,
            mnpi_classification: MnpiClassification::PublicConfirmed,
            public_availability_time: Some(UtcTimestamp::UNIX_EPOCH),
            freshness_seconds: Some(60),
            execution_eligible: true,
        }
    }

    fn bundle(purpose: BundlePurpose, quarantine_count: u32, all_eligible: bool) -> EvidenceBundle {
        EvidenceBundle {
            evidence_bundle_id: "b1".into(),
            created_at: UtcTimestamp::UNIX_EPOCH,
            decision_time: None,
            records: vec![eligible_record()],
            bundle_purpose: purpose,
            all_records_execution_eligible: all_eligible,
            quarantine_count,
            service_attestation: None,
            canonical_hash: "sha256:0".into(),
        }
    }

    #[test]
    fn research_bundle_is_never_live_eligible() {
        assert!(!bundle(BundlePurpose::Research, 0, true).is_live_execution_eligible());
    }

    #[test]
    fn quarantined_record_blocks_live_eligibility() {
        let mut b = bundle(BundlePurpose::LiveExecution, 1, true);
        assert!(!b.is_live_execution_eligible());
        b.quarantine_count = 0;
        assert!(b.is_live_execution_eligible());
    }

    #[test]
    fn empty_records_never_live_eligible_even_if_flagged_all_eligible() {
        let mut b = bundle(BundlePurpose::LiveExecution, 0, true);
        b.records.clear();
        assert!(!b.is_live_execution_eligible());
    }

    #[test]
    fn internal_consistency_catches_a_lying_bundle() {
        // `all_records_execution_eligible: true` but the one record it
        // actually contains says otherwise.
        let mut b = bundle(BundlePurpose::Research, 0, true);
        b.records[0].execution_eligible = false;
        assert!(!b.is_internally_consistent());
    }
}
