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

## Status: not-started

**This repository contains a directory/module skeleton only.** It was
created to establish the repository/security boundary early, not because
Phase 3 implementation work happened here yet. Every file under
`crates/trade-guard-core/src/` is a doc comment describing what belongs
there — see [`docs/CAPABILITY_STATUS.md`](docs/CAPABILITY_STATUS.md) for
the exact status and the recommended next milestone (an internal
paper-simulator vertical slice, per
`06-implementation-order-and-acceptance.md` Phase 3).

Do not integrate against this repository yet. `cargo build --workspace`
succeeds and `cargo run --bin trade_guard_server` prints an honest
"not-started" message — that is the entire current behavior.

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
authoritative and the baseline as filling gaps V2 doesn't restate (e.g. the
baseline's SQLite-by-default audit-storage default). Resolve this properly
before deep implementation rather than guessing twice.

## License

GNU General Public License v3.0 (or, at your option, any later version). See
[LICENSE](LICENSE) and [NOTICE](NOTICE).
