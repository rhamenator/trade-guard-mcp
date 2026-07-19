# Changelog

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
