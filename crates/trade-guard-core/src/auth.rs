//! Authenticated service identity and roles.
//!
//! `03-create-trade-guard-mcp.md` specifies `observer`/`model`/`operator`/
//! `admin` roles enforced over an authenticated Streamable HTTP/OAuth
//! transport. This vertical slice has **only** a local stdio transport
//! (`mcp.rs`), which — same precedent as `smart-dynamic-hedge`'s own MCP
//! server, see that repo's `docs/THREAT_MODEL.md` "The current stdio
//! server assumes a trusted local client" — treats its single caller as
//! trusted by construction (whoever can spawn this process and write to
//! its stdin already has equivalent or greater access). `CallerRole`
//! exists now so `tools.rs`'s per-tool role gate has a real type to check
//! against, even though every stdio caller currently resolves to the same
//! role; a real Streamable HTTP transport would replace `resolve_caller`
//! with actual authentication, not add a new concept.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallerRole {
    Observer,
    Model,
    Operator,
    Admin,
}

impl CallerRole {
    /// This crate implements no admin surface (`admin.rs` is
    /// not-started — no live arming, no credential management, nothing
    /// for `Admin` to actually do yet), so `resolve_caller` never returns
    /// `Admin`. `Model` is the correct default for a local stdio caller:
    /// it can read state, validate/check, and submit paper orders — the
    /// full surface this vertical slice's tool registry exposes.
    pub fn resolve_stdio_caller() -> CallerRole {
        CallerRole::Model
    }

    /// Whether this role is permitted to invoke a tool that requires at
    /// least `required`. `Observer < Model < Operator < Admin`, matching
    /// increasing capability — see the derived `Ord`.
    pub fn permits(self, required: CallerRole) -> bool {
        self >= required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering_is_increasing_capability() {
        assert!(CallerRole::Observer < CallerRole::Model);
        assert!(CallerRole::Model < CallerRole::Operator);
        assert!(CallerRole::Operator < CallerRole::Admin);
    }

    #[test]
    fn permits_allows_equal_or_higher_role_only() {
        assert!(CallerRole::Model.permits(CallerRole::Observer));
        assert!(CallerRole::Model.permits(CallerRole::Model));
        assert!(!CallerRole::Model.permits(CallerRole::Operator));
        assert!(!CallerRole::Observer.permits(CallerRole::Model));
    }

    #[test]
    fn stdio_caller_resolves_to_model_not_admin() {
        assert_eq!(CallerRole::resolve_stdio_caller(), CallerRole::Model);
    }
}
