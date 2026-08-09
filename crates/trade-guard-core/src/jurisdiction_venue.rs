//! `VenueProfile`/`JurisdictionProfile` — hand-transcribed from
//! `market-system-contracts/schemas/2.0.0/jurisdiction-venue-profile.schema.json`,
//! the same way every other cross-repo type in this crate is. Closes one
//! of `docs/ROADMAP.md` Phase 4's named gaps for the sibling
//! `smart-dynamic-hedge` repository: "international instrument/venue
//! schemas."
//!
//! `03-create-trade-guard-mcp.md`'s international architecture section:
//! "Never silently approximate a venue rule. If a required profile is
//! missing or stale, block live execution and allow paper simulation
//! only with a visible limitation." This crate has no live-execution
//! path at all (so "block live" is already unconditionally true — see
//! `trade_intent::IntentMode::requests_live_execution`), which makes the
//! second half — "allow paper simulation only with a visible
//! limitation" — the actually load-bearing part to implement here:
//! `check_venue_profile_availability` never blocks a paper order, it
//! only attaches a `PolicyOutcome::warnings` entry when the instrument's
//! venue has no registered profile.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::instrument::InstrumentId;
use crate::utc_timestamp::UtcTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionPhaseName {
    PreMarket,
    Regular,
    AuctionOpen,
    AuctionClose,
    AfterHours,
    Overnight,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionPhase {
    pub phase_name: SessionPhaseName,
    pub opens_local_time: String,
    pub closes_local_time: String,
}

/// A subset of `venue-profile`'s schema fields — the ones this crate
/// actually has a use for today (session/currency/settlement/lot
/// awareness). The schema itself has more fields
/// (`price-bands-and-collars`, `circuit-breakers`, etc.); this type
/// doesn't need to model every one of them to be a real, useful
/// consumer, and `additionalProperties: true` on the schema side means a
/// producer sending extra fields is still schema-valid even though this
/// type would ignore them on deserialize (there is no cross-repo
/// deserialization of this type today — it is only ever hand-built here
/// — so that gap is theoretical, not yet a real drift risk).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct VenueProfile {
    pub venue_mic: String,
    pub market_timezone: String,
    pub effective_from: UtcTimestamp,
    #[serde(default)]
    pub effective_to: Option<UtcTimestamp>,
    #[serde(default)]
    pub session_phases: Vec<SessionPhase>,
    #[serde(default)]
    pub settlement_convention: Option<String>,
    #[serde(default)]
    pub supported_currencies: Vec<String>,
    #[serde(default)]
    pub min_lot_default: Option<u32>,
}

impl VenueProfile {
    pub fn is_effective_at(&self, now: UtcTimestamp) -> bool {
        if self.effective_from.saturating_diff_seconds(&now) > 0 {
            return false;
        }
        match &self.effective_to {
            Some(end) => now.saturating_diff_seconds(end) <= 0,
            None => true,
        }
    }
}

/// An in-memory venue-profile lookup, keyed by `venue-mic`. No storage
/// backend — every profile is either hand-seeded (see
/// `VenueRegistry::with_demo_fixtures`, the same NYSE Arca/Tokyo Stock
/// Exchange fixtures `market-system-contracts`'
/// `testdata/cases/jurisdiction-venue-profile.json` uses) or registered
/// by a caller. A real deployment would load these from a real venue-
/// reference-data source; that source does not exist anywhere in this
/// system yet.
#[derive(Debug, Clone, Default)]
pub struct VenueRegistry {
    profiles: HashMap<String, VenueProfile>,
}

impl VenueRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, profile: VenueProfile) {
        self.profiles.insert(profile.venue_mic.clone(), profile);
    }

    pub fn get(&self, venue_mic: &str) -> Option<&VenueProfile> {
        self.profiles.get(venue_mic)
    }

    /// Seeds two genuinely different real-world venues — not
    /// palette-swapped U.S. data — matching
    /// `market-system-contracts`'s own golden fixtures exactly, so a
    /// query against either side (schema fixture, this registry) is
    /// checking the same real numbers.
    pub fn with_demo_fixtures() -> Self {
        let mut registry = Self::new();
        registry.register(VenueProfile {
            venue_mic: "ARCX".to_string(),
            market_timezone: "America/New_York".to_string(),
            effective_from: UtcTimestamp::parse_rfc3339("2026-01-01T00:00:00Z")
                .expect("valid fixture timestamp"),
            effective_to: None,
            session_phases: vec![
                SessionPhase {
                    phase_name: SessionPhaseName::PreMarket,
                    opens_local_time: "04:00".to_string(),
                    closes_local_time: "09:30".to_string(),
                },
                SessionPhase {
                    phase_name: SessionPhaseName::Regular,
                    opens_local_time: "09:30".to_string(),
                    closes_local_time: "16:00".to_string(),
                },
                SessionPhase {
                    phase_name: SessionPhaseName::AfterHours,
                    opens_local_time: "16:00".to_string(),
                    closes_local_time: "20:00".to_string(),
                },
            ],
            settlement_convention: Some("T+1".to_string()),
            supported_currencies: vec!["USD".to_string()],
            min_lot_default: Some(1),
        });
        registry.register(VenueProfile {
            venue_mic: "XJPX".to_string(),
            market_timezone: "Asia/Tokyo".to_string(),
            effective_from: UtcTimestamp::parse_rfc3339("2026-01-01T00:00:00Z")
                .expect("valid fixture timestamp"),
            effective_to: None,
            session_phases: vec![
                SessionPhase {
                    phase_name: SessionPhaseName::AuctionOpen,
                    opens_local_time: "08:00".to_string(),
                    closes_local_time: "09:00".to_string(),
                },
                SessionPhase {
                    phase_name: SessionPhaseName::Regular,
                    opens_local_time: "09:00".to_string(),
                    closes_local_time: "11:30".to_string(),
                },
                SessionPhase {
                    phase_name: SessionPhaseName::Regular,
                    opens_local_time: "12:30".to_string(),
                    closes_local_time: "15:00".to_string(),
                },
                SessionPhase {
                    phase_name: SessionPhaseName::AuctionClose,
                    opens_local_time: "15:00".to_string(),
                    closes_local_time: "15:25".to_string(),
                },
            ],
            settlement_convention: Some("T+2".to_string()),
            supported_currencies: vec!["JPY".to_string()],
            min_lot_default: Some(100),
        });
        registry
    }
}

/// Returns a warning string (never blocks) when `instrument` names a
/// venue with no registered profile, or when its profile exists but
/// isn't effective at `now` (stale/not-yet-effective). Returns `None`
/// when the instrument has no `venue_mic` at all (nothing to check —
/// most fixtures/tests in this system don't set one) or when a current
/// profile is found.
pub fn check_venue_profile_availability(
    instrument: &InstrumentId,
    registry: &VenueRegistry,
    now: UtcTimestamp,
) -> Option<String> {
    let venue_mic = instrument.venue_mic.as_deref()?;
    match registry.get(venue_mic) {
        Some(profile) if profile.is_effective_at(now) => None,
        Some(_) => Some(format!(
            "venue profile for {venue_mic:?} exists but is not effective as of this decision -- paper simulation proceeding with a visible limitation, per international-architecture policy"
        )),
        None => Some(format!(
            "no venue profile configured for {venue_mic:?} -- paper simulation proceeding with a visible limitation; live execution would require one"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::InstrumentId;

    fn instrument_with_venue(venue_mic: Option<&str>) -> InstrumentId {
        let mut inst = InstrumentId::equity("inst-1", "SPY");
        inst.venue_mic = venue_mic.map(str::to_string);
        inst
    }

    #[test]
    fn registered_and_currently_effective_venue_produces_no_warning() {
        let registry = VenueRegistry::with_demo_fixtures();
        let inst = instrument_with_venue(Some("ARCX"));
        let now = UtcTimestamp::parse_rfc3339("2026-07-20T00:00:00Z").unwrap();
        assert!(check_venue_profile_availability(&inst, &registry, now).is_none());
    }

    #[test]
    fn unregistered_venue_produces_a_warning_not_a_rejection() {
        let registry = VenueRegistry::with_demo_fixtures();
        let inst = instrument_with_venue(Some("XLON"));
        let now = UtcTimestamp::parse_rfc3339("2026-07-20T00:00:00Z").unwrap();
        let warning = check_venue_profile_availability(&inst, &registry, now).unwrap();
        assert!(warning.contains("XLON"));
        assert!(warning.contains("visible limitation"));
    }

    #[test]
    fn instrument_with_no_venue_mic_produces_no_warning() {
        let registry = VenueRegistry::with_demo_fixtures();
        let inst = instrument_with_venue(None);
        let now = UtcTimestamp::parse_rfc3339("2026-07-20T00:00:00Z").unwrap();
        assert!(check_venue_profile_availability(&inst, &registry, now).is_none());
    }

    #[test]
    fn a_profile_not_yet_effective_produces_a_warning() {
        let mut registry = VenueRegistry::new();
        registry.register(VenueProfile {
            venue_mic: "FUTV".to_string(),
            market_timezone: "UTC".to_string(),
            effective_from: UtcTimestamp::parse_rfc3339("2099-01-01T00:00:00Z").unwrap(),
            effective_to: None,
            session_phases: vec![],
            settlement_convention: None,
            supported_currencies: vec![],
            min_lot_default: None,
        });
        let inst = instrument_with_venue(Some("FUTV"));
        let now = UtcTimestamp::parse_rfc3339("2026-07-20T00:00:00Z").unwrap();
        let warning = check_venue_profile_availability(&inst, &registry, now).unwrap();
        assert!(warning.contains("not effective"));
    }

    #[test]
    fn an_expired_profile_produces_a_warning() {
        let mut registry = VenueRegistry::new();
        registry.register(VenueProfile {
            venue_mic: "OLDV".to_string(),
            market_timezone: "UTC".to_string(),
            effective_from: UtcTimestamp::parse_rfc3339("2000-01-01T00:00:00Z").unwrap(),
            effective_to: Some(UtcTimestamp::parse_rfc3339("2010-01-01T00:00:00Z").unwrap()),
            session_phases: vec![],
            settlement_convention: None,
            supported_currencies: vec![],
            min_lot_default: None,
        });
        let inst = instrument_with_venue(Some("OLDV"));
        let now = UtcTimestamp::parse_rfc3339("2026-07-20T00:00:00Z").unwrap();
        assert!(check_venue_profile_availability(&inst, &registry, now).is_some());
    }

    #[test]
    fn demo_fixtures_are_genuinely_different_not_palette_swapped() {
        let registry = VenueRegistry::with_demo_fixtures();
        let us = registry.get("ARCX").unwrap();
        let jp = registry.get("XJPX").unwrap();
        assert_ne!(us.market_timezone, jp.market_timezone);
        assert_ne!(us.supported_currencies, jp.supported_currencies);
        assert_ne!(us.settlement_convention, jp.settlement_convention);
        assert_ne!(us.min_lot_default, jp.min_lot_default);
        // Japan's session has a split lunch-break structure (two
        // "regular" phases); the US session does not.
        let jp_regular_count = jp
            .session_phases
            .iter()
            .filter(|p| matches!(p.phase_name, SessionPhaseName::Regular))
            .count();
        assert_eq!(jp_regular_count, 2);
    }
}
