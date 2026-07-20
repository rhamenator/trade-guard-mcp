//! `PolicyDecision` — the vocabulary this service uses to explain an
//! allow/deny outcome. Matches the exact state list
//! `03-create-trade-guard-mcp.md` requires the service "must be able to
//! say".

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyDecision {
    Allow,
    AllowWithAdjustment,
    Reject,
    Expired,
    BlockedBySourcePolicy,
    BlockedByJurisdiction,
    BlockedByEntitlement,
    BlockedByMarketState,
    BlockedByRisk,
    BlockedByOperatorControl,
    UnknownPendingReconciliation,
}

impl PolicyDecision {
    pub fn permits_submission(self) -> bool {
        matches!(self, PolicyDecision::Allow | PolicyDecision::AllowWithAdjustment)
    }
}

/// The full explainable result of running policy against a `TradeIntent`:
/// not just allow/deny, but the reason codes a caller (or an operator
/// reviewing `list-recent-decisions`) needs to understand why.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PolicyOutcome {
    pub decision: PolicyDecision,
    pub reason_codes: Vec<String>,
}

impl PolicyOutcome {
    pub fn allow() -> Self {
        PolicyOutcome { decision: PolicyDecision::Allow, reason_codes: Vec::new() }
    }

    pub fn reject(decision: PolicyDecision, reason_code: impl Into<String>) -> Self {
        PolicyOutcome { decision, reason_codes: vec![reason_code.into()] }
    }

    pub fn is_allowed(&self) -> bool {
        self.decision.permits_submission()
    }

    /// Merges another outcome's reason codes into this one, keeping the
    /// more restrictive of the two decisions (`Allow` loses to anything
    /// else; among two rejections, the first-seen one wins so the most
    /// specific/relevant check that ran first stays the headline
    /// decision).
    pub fn and(self, other: PolicyOutcome) -> PolicyOutcome {
        if !self.is_allowed() {
            let mut reason_codes = self.reason_codes;
            reason_codes.extend(other.reason_codes);
            return PolicyOutcome { decision: self.decision, reason_codes };
        }
        if !other.is_allowed() {
            return other;
        }
        let mut reason_codes = self.reason_codes;
        reason_codes.extend(other.reason_codes);
        PolicyOutcome { decision: PolicyDecision::Allow, reason_codes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_and_allow_with_adjustment_permit_submission() {
        assert!(PolicyDecision::Allow.permits_submission());
        assert!(PolicyDecision::AllowWithAdjustment.permits_submission());
        assert!(!PolicyDecision::Reject.permits_submission());
        assert!(!PolicyDecision::BlockedByRisk.permits_submission());
    }

    #[test]
    fn and_keeps_the_first_rejection_as_headline_decision() {
        let a = PolicyOutcome::reject(PolicyDecision::BlockedByRisk, "over-buying-power");
        let b = PolicyOutcome::reject(PolicyDecision::BlockedBySourcePolicy, "quarantined-evidence");
        let combined = a.and(b);
        assert_eq!(combined.decision, PolicyDecision::BlockedByRisk);
        assert_eq!(combined.reason_codes, vec!["over-buying-power", "quarantined-evidence"]);
    }

    #[test]
    fn and_of_two_allows_is_allow() {
        let combined = PolicyOutcome::allow().and(PolicyOutcome::allow());
        assert!(combined.is_allowed());
    }

    #[test]
    fn and_of_allow_then_reject_is_the_rejection() {
        let combined = PolicyOutcome::allow().and(PolicyOutcome::reject(PolicyDecision::Expired, "intent-expired"));
        assert_eq!(combined.decision, PolicyDecision::Expired);
        assert!(!combined.is_allowed());
    }
}
