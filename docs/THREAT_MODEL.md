# Threat model

## Scope

Trade Guard is a paper-only local MCP service. It owns deterministic policy,
paper-order authorization, reconciliation, and a tamper-evident SQLite audit
trail. It has no remote transport, live broker adapter, or live-order path.

## Assets

- integrity of trade intents and evidence bundles;
- deterministic policy outcomes and idempotency decisions;
- integrity and availability of the local audit database;
- separation between untrusted intelligence input and execution policy.

## Trust boundaries

1. The local stdio MCP client is trusted to invoke tools but not to bypass
   validation.
2. Evidence bundles are untrusted until schema, hash, eligibility, and policy
   checks pass.
3. SQLite is local durable state. Its hash chain is tamper-evident, not a
   digital signature and not proof that the host is uncompromised.
4. The paper simulator is the terminal execution boundary in this release.

## Primary abuse cases and controls

| Abuse case | Current control |
| --- | --- |
| Submit a live intent | Rejected before account or provider handling. |
| Replay the same intent | Durable idempotency-key lookup returns the original result. |
| Use restricted or nonpublic evidence | Deterministic eligibility policy denies authorization. |
| Alter or delete audit records | Hash-chain verification detects payload changes and gaps. |
| Bypass policy through malformed input | Typed parsing, explicit validation, and boundary tests fail closed. |
| Reach the service remotely | No network transport exists in the current build. |

## Explicit non-goals

This release does not secure a compromised host, authenticate remote users,
store broker credentials, provide market-abuse surveillance, or execute live
orders. Adding any of those capabilities requires a replacement threat model,
credential-isolation design, operational controls, and independent review.
