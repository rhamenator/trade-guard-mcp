# Security policy

## Supported code

Security fixes are applied to the current `main` branch. The current product is
paper-only; it has no live broker adapter or live-order path.

## Reporting a vulnerability

Please use GitHub's private vulnerability-reporting flow from the repository's
**Security** tab. If that flow is unavailable, email
`rich@yourfoxprodeveloper.com` with the repository name, affected revision,
impact, reproduction steps, and any proposed mitigation.

Do not open a public issue for an unpatched vulnerability and do not include
real credentials, account data, nonpublic financial information, or personal
data in a report. You should receive an acknowledgment within seven days.

## Security boundary

The current implementation accepts only local stdio MCP traffic and simulates
paper orders. It must fail closed for live intents. Any remote transport,
credential storage, broker adapter, or live execution capability requires a
new threat model and a separate security review before release. See
[docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).
