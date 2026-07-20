# Changelog

## Unreleased (international venue awareness)

Closes one of the five gaps `smart-dynamic-hedge`'s `docs/ROADMAP.md`
Phase 4 named as still open across the system: international
instrument/venue schemas existed in `market-system-contracts` as an
untested Phase 1 scaffold, and nothing consumed them anywhere.

- **New `jurisdiction_venue` module**: `VenueProfile`/`JurisdictionProfile`,
  hand-transcribed from `market-system-contracts`'s
  `jurisdiction-venue-profile.schema.json`. `VenueRegistry::with_demo_fixtures()`
  seeds two genuinely different real venues — NYSE Arca (`America/New_York`,
  USD, T+1, penny ticks) and Tokyo Stock Exchange (`Asia/Tokyo`, JPY,
  T+2, yen-band ticks, a split lunch-break session with two separate
  `regular` phases) — matching `market-system-contracts`'s own new golden
  fixtures exactly, not palette-swapped U.S. data.
- **`PolicyOutcome` gained a `warnings: Vec<String>` field**, separate
  from `reason_codes`: a warning never changes the decision and never
  blocks submission. `check_venue_profile_availability` is the first
  producer — per `03-create-trade-guard-mcp.md`'s international-
  architecture section ("allow paper simulation only with a visible
  limitation"), a missing or not-yet-effective/expired venue profile
  attaches a warning to an otherwise-successful paper order rather than
  rejecting it. This repository has no live-execution path at all, so
  the "block live execution" half of that guidance was already
  unconditionally true before this change; the warning is what makes the
  second half real.
- **New `get-venue-profile` MCP tool** (14 tools total, was 13).
- Verified end to end against the compiled release binary: a real
  `get-venue-profile` lookup returning the Tokyo Stock Exchange fixture,
  an unconfigured venue lookup failing cleanly, and a real paper order
  against an unregistered venue (`XLON`) filling successfully with the
  warning attached in the returned `policy_outcome`.
- **11 new tests, 160 total** (was 149), `cargo clippy --workspace
  --all-targets` clean.

## Unreleased (paper-only vertical slice: typed contracts, evidence gate, atomic paper execution, hash-chained audit, MCP server)

Implemented `06-implementation-order-and-acceptance.md` Phase 3 — "the
smallest trustworthy complete vertical slice": typed intent -> authoritative
state -> deterministic policy -> paper submission -> durable state -> replay
-> a real MCP server. Every module was a `not-started` doc-comment stub
before this pass; see `docs/CAPABILITY_STATUS.md` for the exact new status.

- **Hand-rolled primitives**: `decimal.rs` (a decimal-safe fixed-point type
  matching `common.schema.json#/$defs/decimal-string`'s exact grammar —
  rejects `NaN`/`Infinity`/leading-zero/negative-zero/exponent forms
  outright, never floating point at an external boundary); `sha256.rs`
  (duplicated from `smart-dynamic-hedge`'s `smart_hedge_models::sha256`,
  same NIST vectors); `utc_timestamp.rs` (duplicated from
  `market-intelligence-mcp`'s `market_intelligence_core::utc_timestamp`,
  including its fuzz-smoke tests).
- **Typed domain contracts**, hand-transcribed from
  `market-system-contracts/schemas/2.0.0/{instrument-id,trade-intent,
  evidence-bundle,legal-status}.schema.json`: `instrument.rs`,
  `trade_intent.rs`, `evidence.rs`, `legal_status.rs`, `account.rs`
  (`Position`/`AccountSnapshot`), `order.rs` (`Order`/`Fill`/`OrderState`),
  `policy_decision.rs` (`PolicyDecision`/`PolicyOutcome`).
- **`policy.rs`**: `validate_trade_intent` (schema-shape/sanity checks —
  quantity, limit/stop price presence, confidence range, expiry) and
  `check_evidence_eligibility` (the MNPI/source-policy gate — directly
  verifies required tests #1 "a research-only evidence bundle is rejected
  for live and allowed for research/paper" and #3 "a future
  public-availability timestamp is rejected" from
  `03-create-trade-guard-mcp.md`), plus `check_buying_power`.
- **`providers.rs`**: `PaperSimulator` — deterministic, in-memory, no
  persisted quote state needed (a synthetic quote is a pure function of
  `sha256(instrument_id)`, so the same symbol always gets the same
  fixture price). Market orders fill fully and immediately; limit orders
  fill only if marketable, else stay open. No resting order book — a
  documented limitation, not a silent gap.
- **`audit.rs`**: `AuditStore` — hash-chained, append-only SQLite log (the
  one `rusqlite` dependency exception, matching `smart-dynamic-hedge`'s
  `smart-hedge-store` justification). `verify_integrity` walks the whole
  chain and detects both payload tampering (a record's own recomputed
  hash no longer matches) and record deletion (the *next* record's
  `prev_hash` link breaks) — verified with two separate raw-SQL corruption
  tests (required test #22).
- **`execution.rs`**: `authorize_and_submit_paper_order` — the atomic
  protocol. The durable idempotency-key check runs first, before any
  validation or account mutation, so a resubmitted intent (allowed,
  rejected, or filled the first time) always returns the original outcome
  (required test #14: "duplicate idempotency key returns/reconciles
  original order rather than submitting twice"). Live-mode intents are
  rejected outright, before touching the account or any provider — not
  merely because no live provider is configured.
- **`auth.rs`**: minimal `CallerRole` — a stdio caller always resolves to
  `Model`, matching `smart-dynamic-hedge`'s own "trusted local client"
  stdio precedent; documented as a placeholder for real authentication
  once a remote transport exists, not a security control today.
- **`tools.rs`/`mcp.rs`/`state.rs`**: a hand-rolled MCP stdio JSON-RPC 2.0
  transport (no MCP SDK dependency, same shape as `smart-dynamic-hedge`'s
  `smart_hedge_mcp`), exposing 13 tools (`health`, `capabilities`,
  `tool-catalog`, `self-test`, `validate-trade-intent`,
  `check-evidence-eligibility`, `get-account-snapshot`, `get-positions`,
  `get-open-orders`, `authorize-and-submit-paper-order`,
  `list-recent-decisions`, `replay-decision`, `audit-integrity`). No
  live-execution tool exists in source at all.
  `cancel-paper-order`/`replace-paper-order` are deliberately not
  implemented, since `PaperSimulator` keeps no persistent open-order book
  for them to act on.
- **`services/trade-guard-server`**: a real binary (`mcp` runs the stdio
  server; `self-test` runs internal invariant checks and reports a process
  exit code). Verified end to end: `self-test` passes against the
  compiled release binary, and a real piped multi-message MCP session
  (`initialize`, `tools/list`, `health`, a real paper fill, a duplicate
  idempotency-key resubmission, `audit-integrity`) produces correct
  responses throughout, including the account/position update from the
  fill and the correct `was_duplicate: true` on resubmission.
- **149 tests total**, `cargo test --workspace` all green, `cargo clippy
  --workspace --all-targets` clean under `clippy::all`.
- Deferred modules (`risk.rs` market-abuse surveillance,
  `reconciliation.rs`, `telemetry.rs`, `admin.rs`) each got an updated doc
  comment explaining specifically why they're still `not-started` for this
  vertical slice, rather than being silently left with a stale
  "not-started" comment written before this pass existed.

## Unreleased (dependency/lint policy alignment)

Adopted the same dependency-minimization and security-lint policy as
`market-intelligence-mcp` before any real code lands here: removed the
placeholder `serde`/`serde_json`/`uuid`/`time`/`thiserror` workspace
dependencies (nothing here used them yet), added
`[workspace.lints] unsafe_code = "forbid"` and `clippy.all = "warn"` applied
to every crate/service via `[lints] workspace = true`, and added
`.cargo/config.toml` disabling incremental compilation (see
`market-intelligence-mcp`'s changelog for why). When real implementation
starts, follow `market_intelligence_core::utc_timestamp`'s precedent:
hand-roll what's reasonably hand-rollable, keep only `serde`/`serde_json`.

## Unreleased — 2026-07-19

Repository created alongside `market-system-contracts` and
`market-intelligence-mcp` to establish the three-repository security
boundary. Scaffolded a two-crate Cargo workspace (`trade-guard-core`,
`trade-guard-server`) with a module skeleton matching
`03-create-trade-guard-mcp.md`'s suggested layout. Every module is an
explicit `not-started` placeholder — no risk engine, execution protocol,
provider adapter, or MCP transport exists yet. See
`docs/CAPABILITY_STATUS.md`.
