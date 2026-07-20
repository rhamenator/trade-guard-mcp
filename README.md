# trade-guard-mcp

Authoritative account state, deterministic pre-trade risk policy, idempotent
execution, reconciliation, audit, and broker/venue adapters (paper and,
eventually, live) for the Smart Dynamic Hedge system.

- [`smart-dynamic-hedge`](https://github.com/rhamenator/smart-dynamic-hedge) — strategy, research, GUI, and autonomy plane. Sends typed `TradeIntent`s here; never holds unrestricted broker credentials.
- [`market-intelligence-mcp`](https://github.com/rhamenator/market-intelligence-mcp) — public/licensed intelligence. This repo verifies its signed `EvidenceBundle`s but never scrapes the web or generates a market thesis itself.
- **`trade-guard-mcp`** (this repo) — the only place in the system permitted to hold narrowly-scoped execution credentials, and only once live mode is explicitly armed by an operator.

See [`market-system-contracts`](https://github.com/rhamenator/market-system-contracts)
for the schemas all three repositories share, in particular
`trade-intent.schema.json` and `evidence-bundle.schema.json`.

## Status: paper-only vertical slice

A real, working, tested implementation exists — not a skeleton. See
[`docs/CAPABILITY_STATUS.md`](docs/CAPABILITY_STATUS.md) for the exact
per-module status. In short:

- Typed `TradeIntent`/`EvidenceBundle`/`InstrumentId` contracts, hand-
  transcribed from `market-system-contracts`.
- `check-evidence-eligibility` — the MNPI/source-policy gate a `TradeIntent`
  must pass before an evidence-backed order can be authorized.
- The atomic `authorize-and-submit-paper-order` protocol: durable,
  idempotency-key-deduplicated, against an internal deterministic paper
  simulator.
- A hash-chained, tamper-evident SQLite audit log (`audit-integrity`,
  `list-recent-decisions`, `replay-decision`).
- International venue awareness (`get-venue-profile`): a
  `VenueProfile`/`JurisdictionProfile` registry seeded with two genuinely
  different real venues (NYSE Arca and Tokyo Stock Exchange — different
  timezone, currency, settlement convention, session structure). A
  missing or stale venue profile never blocks a paper order; it attaches
  a visible `PolicyOutcome.warnings` entry instead, per
  `03-create-trade-guard-mcp.md`'s international-architecture guidance.
- A real MCP stdio JSON-RPC server (`cargo run --bin trade_guard_server --
  mcp`) exposing 14 tools.

**There is no live-execution path.** Not "present but disabled" —
genuinely absent from the source. `authorize_and_submit_paper_order`
rejects any live-mode `TradeIntent` before touching the account or any
provider. No real broker/venue/FIX adapter, no market-abuse surveillance,
no remote transport, and no operator-admin surface exist yet — each is a
documented, deliberate scope cut (`docs/CAPABILITY_STATUS.md`), staged per
`06-implementation-order-and-acceptance.md`.

## Quick start

```bash
cargo build --release --workspace
TRADE_GUARD_DB=.trade-guard/audit.sqlite3 ./target/release/trade_guard_server self-test
```

Drive the MCP server directly over stdio:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | ./target/release/trade_guard_server mcp
```

`TRADE_GUARD_DB` defaults to `.trade-guard/audit.sqlite3` under the
current working directory if unset. Every run starts with a fresh
in-memory `$100,000` USD paper account (`account_alias: "paper-default"`)
— nothing about account state is persisted across process restarts yet,
only the audit log is.

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```

160 tests, all passing; `clippy::all` clean. See
`docs/CAPABILITY_STATUS.md` for what's covered — including two required
tests from `03-create-trade-guard-mcp.md` this pass verified directly: a
research-only evidence bundle is rejected for live-mode (#1), and a
duplicate idempotency key returns the original order rather than
submitting twice (#14).

## Why this is a separate repository, not a module of `smart-dynamic-hedge`

This is the one place in the three-repository system where broker/exchange
execution credentials will eventually live. It must never share a process,
network boundary, or codebase with the intelligence-collection service
(which handles untrusted public text) or with the strategy/model-reasoning
plane. See `05-source-policy-and-legal-boundaries.md` and
`06-implementation-order-and-acceptance.md` in the originating prompt
bundle for the reasoning.

## Where this comes from

This repository is scoped by `03-create-trade-guard-mcp.md` from the
`smart-dynamic-hedge-v2-prompt-bundle` (2026-07-19). That file itself
layers a "V2" specification over an older baseline that it says it
"supersedes" without fully reconciling the two — read V2 sections as
authoritative and the baseline as filling gaps V2 doesn't restate. This
vertical slice follows the baseline's own closing guidance: "Build the
smallest trustworthy complete vertical slice first: typed intent ->
authoritative state -> deterministic policy -> paper submission -> durable
state -> replay. Add live provider code only after that slice is correct
and tested."

## License

GNU General Public License v3.0 (or, at your option, any later version). See
[LICENSE](LICENSE) and [NOTICE](NOTICE).
