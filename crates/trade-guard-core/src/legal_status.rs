//! `MnpiClassification` — duplicated from `market-intelligence-mcp`'s
//! `market_intelligence_core::legal_status` (same reasoning as
//! `utc_timestamp.rs`: small, well-tested, per-repository duplication
//! rather than a shared dependency across a security boundary). This crate
//! only needs the MNPI vocabulary, not the full `LegalStatus` enum
//! (`market_intelligence_core` uses `LegalStatus` for entity/event
//! records this crate never handles).

use serde::{Deserialize, Serialize};

/// Classification of how public/nonpublic a record is, and whether it may
/// become execution evidence. Only `PublicConfirmed` records with a
/// compatible `SourceUseDecision` may become execution evidence — see
/// `05-source-policy-and-legal-boundaries.md` in the originating prompt
/// bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MnpiClassification {
    PublicConfirmed,
    PublicButUseRestricted,
    PublicationStatusUncertain,
    AmbiguousOrigin,
    SuspectedNonpublic,
    KnownNonpublic,
    ProhibitedSource,
}

impl MnpiClassification {
    /// Only this classification may ever feed a live-execution evidence
    /// bundle. Every other class must be quarantined per the source-policy
    /// engine, never gated by model judgment.
    pub fn is_execution_eligible_class(self) -> bool {
        matches!(self, MnpiClassification::PublicConfirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_confirmed_is_execution_eligible() {
        assert!(MnpiClassification::PublicConfirmed.is_execution_eligible_class());
        for other in [
            MnpiClassification::PublicButUseRestricted,
            MnpiClassification::PublicationStatusUncertain,
            MnpiClassification::AmbiguousOrigin,
            MnpiClassification::SuspectedNonpublic,
            MnpiClassification::KnownNonpublic,
            MnpiClassification::ProhibitedSource,
        ] {
            assert!(!other.is_execution_eligible_class());
        }
    }

    #[test]
    fn serializes_kebab_case() {
        let s = serde_json::to_string(&MnpiClassification::PublicButUseRestricted).unwrap();
        assert_eq!(s, "\"public-but-use-restricted\"");
    }
}
