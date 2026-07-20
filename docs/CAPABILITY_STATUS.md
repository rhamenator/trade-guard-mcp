# Capability status

Per `06-implementation-order-and-acceptance.md`'s status vocabulary
(`not-started, schema-only, mock, fixture-tested, record-replay-tested,
sandbox-tested, demo-tested, paper-tested, certification-tested,
live-tested, disabled-by-policy, unavailable-by-entitlement,
unavailable-by-jurisdiction, deprecated`).

## Summary

**Status: paper-tested vertical slice.** The smallest trustworthy complete
path `03-create-trade-guard-mcp.md`'s own closing line asks for — typed
intent → authoritative state → deterministic policy → paper submission →
durable state → replay — is implemented, tested, and runnable as a real
MCP stdio server, not just a set of tested libraries. **160 tests**
(was 149), `cargo test --workspace` all green, `cargo clippy --workspace
--all-targets` clean under `clippy::all`.

## Crates

| Module | Status | Notes |
|---|---|---|
| `decimal` | fixture-tested | Hand-rolled decimal-safe fixed-point type (8 fractional digits) matching `common.schema.json#/$defs/decimal-string`'s grammar exactly; rejects `NaN`/`Infinity`/leading-zero/negative-zero/exponent forms. |
| `sha256` | fixture-tested | Duplicated from `smart-dynamic-hedge`'s `smart_hedge_models::sha256`; verified against the same NIST test vectors. |
| `utc_timestamp` | fixture-tested | Duplicated from `market-intelligence-mcp`'s `market_intelligence_core::utc_timestamp`, including its fuzz-smoke tests. |
| `instrument`, `trade_intent`, `evidence`, `legal_status`, `account`, `order`, `policy_decision` | fixture-tested | Hand-transcribed from `market-system-contracts/schemas/2.0.0/{instrument-id,trade-intent,evidence-bundle,legal-status}.schema.json`, the same way `market_intelligence_core` transcribes its own schemas. |
| `policy` | fixture-tested | `validate_trade_intent` (schema-shape/sanity), `check_evidence_eligibility` (the MNPI/source-policy gate — required tests #1 and #3 from `03-create-trade-guard-mcp.md` are directly covered), `check_buying_power`. |
| `providers` | fixture-tested | `PaperSimulator` — deterministic, in-memory, no persisted quote state needed (synthetic quotes are a pure function of `sha256(instrument_id)`). Market orders always fill; limit orders fill only if marketable, else stay open. No resting order book, no latency/slippage model — documented limitation, see module doc comment. |
| `audit` | fixture-tested | `AuditStore` — hash-chained SQLite append-only log (the one `rusqlite` dependency exception, matching `smart-dynamic-hedge`'s `smart-hedge-store`). `verify_integrity` detects both payload tampering and record deletion (required test #22) via direct raw-SQL corruption tests. Durable idempotency-key lookup backs the dedup guarantee in `execution`. |
| `execution` | fixture-tested | `authorize_and_submit_paper_order` — the atomic protocol. Required test #14 (duplicate idempotency key returns the original result, never submits twice) is directly covered, along with live-mode rejection, buying-power rejection, and account-mismatch rejection, each verified to leave the account/position untouched. Now also attaches a `PolicyOutcome.warnings` entry (never blocks) when an instrument's venue has no current profile — see `jurisdiction_venue`. |
| `jurisdiction_venue` | fixture-tested + real end-to-end | **New.** `VenueProfile`/`JurisdictionProfile`, hand-transcribed from `market-system-contracts`'s `jurisdiction-venue-profile.schema.json`. `VenueRegistry::with_demo_fixtures()` seeds two genuinely different real venues (NYSE Arca: `America/New_York`, USD, T+1; Tokyo Stock Exchange: `Asia/Tokyo`, JPY, T+2, split lunch-break session) — not palette-swapped U.S. data. `check_venue_profile_availability` never blocks a paper order; per `03-create-trade-guard-mcp.md`'s international-architecture section ("allow paper simulation only with a visible limitation"), a missing or stale profile only adds a warning. |
| `auth` | fixture-tested | Minimal: a single stdio caller always resolves to the `Model` role (matches `smart-dynamic-hedge`'s own stdio-trusted-local-client precedent). No remote transport, so no real authentication exists yet — documented, not silently assumed. |
| `tools`, `mcp`, `state` | fixture-tested + real end-to-end | Hand-rolled stdio JSON-RPC 2.0 transport (no MCP SDK dependency), 13 tools (see below). Verified with a real piped multi-message session against the compiled release binary, including a real paper fill and a real duplicate-idempotency-key round trip. |
| `risk` (market-abuse surveillance), `reconciliation`, `telemetry`, `admin` | **not-started, deliberately deferred** | Each module's own doc comment explains why: market-abuse detection needs multi-account/multi-order correlation this single-account slice has no reason for yet; reconciliation has nothing to reconcile against until a real (non-paper-simulator) provider exists; telemetry is a real dependency this stdio-local, single-caller slice doesn't need yet; admin/live-arming has nothing to gate since no live path exists at all. |

## Tools implemented (14, all paper/read-only — zero live tools exist)

```text
health, capabilities, tool-catalog, self-test
validate-trade-intent, check-evidence-eligibility
get-account-snapshot, get-positions, get-open-orders
get-venue-profile
authorize-and-submit-paper-order
list-recent-decisions, replay-decision, audit-integrity
```

`cancel-paper-order`/`replace-paper-order` are **deliberately not
implemented**: `PaperSimulator` keeps no persistent open-order book across
calls, so a cancel tool would have nothing real to act on — see
`tools.rs`'s module doc comment.

No live-execution tool exists **in source**, not merely disabled at
runtime — `execution::authorize_and_submit_paper_order` rejects any
live-mode `TradeIntent` outright before any account mutation or provider
call.

## Providers

Only `paper-sim` (the built-in deterministic simulator) exists at any
level. `alpaca`, `ibkr`, `saxo`, `oanda`, crypto exchanges, generic FIX,
and direct-venue adapters are all `not-started`, per
`06-implementation-order-and-acceptance.md` Phase 6 — intentionally
deferred, not attempted and abandoned.

## What this vertical slice proves, and what it doesn't

Proves: the atomic authorize-and-submit protocol is idempotent and
durable; the evidence-eligibility gate correctly distinguishes paper-safe
from live-safe evidence and cannot be bypassed by mode; the audit log
detects both payload tampering and record deletion; a real MCP client can
drive the whole thing over stdio end to end.

Does not prove: behavior against a real broker/venue (none exists);
behavior under concurrent/remote callers (stdio is single request at a
time); market-abuse surveillance (not built); anything about live-mode
correctness (no live path exists to be correct or incorrect about).
