# Capability status

Every module in `crates/trade-guard-core/src/` is **not-started**: a
doc-commented placeholder file only, no logic, no tests. This repository
was created in the same session as `market-system-contracts` and
`market-intelligence-mcp` to establish the three-repository security
boundary, but implementation effort that session went into
`market-intelligence-mcp`'s Phase 2 slice instead — see that repo's
`docs/CAPABILITY_STATUS.md` for what a completed vertical slice looks like
in this system.

No provider (paper simulator, Alpaca, IBKR, Saxo, OANDA, any crypto
exchange, FIX, any direct venue) is implemented at any level.

No order-entry, authorization, or audit code exists. **Do not build against
this repository or assume any tool, type, or endpoint described in
`03-create-trade-guard-mcp.md` currently exists.**

## Recommended next milestone

Per `06-implementation-order-and-acceptance.md` Phase 3: build the
authoritative account/risk/execution path against the internal paper
simulator only, before any real broker. Concretely:

1. Hand-transcribe `TradeIntent`, `EvidenceBundle` (read-only consumer),
   and account/position/order types from `market-system-contracts` into
   `trade_guard_core::models`, the same way `market_intelligence_core` does
   it in the sibling repository.
2. Implement `trade_guard_core::policy::check_evidence_eligibility`
   against a stubbed `EvidenceBundle` — this is the one concrete,
   well-specified cross-repo contract point with `market-intelligence-mcp`.
3. Implement the atomic `authorize-and-submit-paper-order` protocol against
   an in-memory paper simulator, with idempotency-key deduplication as the
   first test.
4. Only then add MCP transport, a real broker adapter, or any live-mode
   code.
