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
        matches!(
            self,
            PolicyDecision::Allow | PolicyDecision::AllowWithAdjustment
        )
    }
}

/// The full explainable result of running policy against a `TradeIntent`:
/// not just allow/deny, but the reason codes a caller (or an operator
/// reviewing `list-recent-decisions`) needs to understand why.
///
/// `warnings` is deliberately separate from `reason_codes`: a warning
/// never changes `decision` and never blocks submission — it is the
/// "visible limitation" `03-create-trade-guard-mcp.md`'s international
/// architecture section calls for ("If a required profile is missing or
/// stale, block live execution and allow paper simulation only with a
/// visible limitation"). A missing venue profile
/// (`jurisdiction_venue::check_venue_profile_availability`) is the first,
/// and so far only, producer of a warning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PolicyOutcome {
    pub decision: PolicyDecision,
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl PolicyOutcome {
    pub fn allow() -> Self {
        PolicyOutcome {
            decision: PolicyDecision::Allow,
            reason_codes: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn reject(decision: PolicyDecision, reason_code: impl Into<String>) -> Self {
        PolicyOutcome {
            decision,
            reason_codes: vec![reason_code.into()],
            warnings: Vec::new(),
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.decision.permits_submission()
    }

    /// Appends one non-blocking warning, keeping the decision and reason
    /// codes untouched. Consuming/returning `Self` so call sites can
    /// chain it: `outcome.with_warning(msg)`.
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Merges another outcome's reason codes and warnings into this one,
    /// keeping the more restrictive of the two decisions (`Allow` loses
    /// to anything else; among two rejections, the first-seen one wins
    /// so the most specific/relevant check that ran first stays the
    /// headline decision). Warnings always accumulate regardless of
    /// which side's decision wins — a warning is informational, not part
    /// of the allow/reject arbitration.
    pub fn and(self, other: PolicyOutcome) -> PolicyOutcome {
        if !self.is_allowed() {
            let mut reason_codes = self.reason_codes;
            reason_codes.extend(other.reason_codes);
            let mut warnings = self.warnings;
            warnings.extend(other.warnings);
            return PolicyOutcome {
                decision: self.decision,
                reason_codes,
                warnings,
            };
        }
        if !other.is_allowed() {
            let mut warnings = self.warnings;
            warnings.extend(other.warnings);
            return PolicyOutcome { warnings, ..other };
        }
        let mut reason_codes = self.reason_codes;
        reason_codes.extend(other.reason_codes);
        let mut warnings = self.warnings;
        warnings.extend(other.warnings);
        PolicyOutcome {
            decision: PolicyDecision::Allow,
            reason_codes,
            warnings,
        }
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
        let b = PolicyOutcome::reject(
            PolicyDecision::BlockedBySourcePolicy,
            "quarantined-evidence",
        );
        let combined = a.and(b);
        assert_eq!(combined.decision, PolicyDecision::BlockedByRisk);
        assert_eq!(
            combined.reason_codes,
            vec!["over-buying-power", "quarantined-evidence"]
        );
    }

    #[test]
    fn and_of_two_allows_is_allow() {
        let combined = PolicyOutcome::allow().and(PolicyOutcome::allow());
        assert!(combined.is_allowed());
    }

    #[test]
    fn and_of_allow_then_reject_is_the_rejection() {
        let combined = PolicyOutcome::allow().and(PolicyOutcome::reject(
            PolicyDecision::Expired,
            "intent-expired",
        ));
        assert_eq!(combined.decision, PolicyDecision::Expired);
        assert!(!combined.is_allowed());
    }
}
