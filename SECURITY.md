# Security policy

## Reporting a vulnerability

Report suspected vulnerabilities privately through
[GitHub private vulnerability reporting](https://github.com/ValidatorsDAO/solana-stream/security/advisories/new).
If that form is not available, contact ERPC support from the
[ERPC Web Dashboard](https://dashboard.erpc.global/en) and ask for a private channel.
Do not open a public issue for a suspected vulnerability, an exposed credential or an
exploitable detail.

Include enough to reproduce the issue safely: package and version (crates.io
`solana-stream-sdk`, npm `@validators-dao/solana-stream-sdk`,
`@validators-dao/solana-shreds-client`, `@validators-dao/solana-entry-decoder`, or a
sample client), runtime and operating system, the impact, and a minimal reproduction.

Redact API keys, `X_TOKEN` values, endpoint URLs that carry a key (`?api-key=…`),
wallet files and key material from the report. Never send a real secret as proof.

## Scope

This policy covers the SDK packages, the sample clients in `client/` and `temp-release/`
(the templates `slv bot init` installs), the repository tooling, and the dependencies as
this repository integrates them. Issues in ERPC endpoints or other services go to their
operators; describe the SDK side of the interaction.

## Supported versions

Security fixes go to the latest published version of each package.

## What the repository checks

- `scripts/run-gitleaks.sh` scans the whole history for credentials and
  `scripts/check-oss-hygiene.sh` checks tracked files for non-public hosts, internal
  references and key material (CI, every pull request).
- `cargo deny check` (`deny.toml`) and `pnpm audit` check dependencies for advisories,
  licenses and sources (CI, every pull request). `deny.toml` lists each accepted
  advisory with its reason.
- The SDK, the sample clients in `client/` and the TypeScript templates in
  `temp-release/` keep credentials out of logs: endpoints and RPC errors are logged
  without the credentials, path or query of their URLs (`solana_stream_sdk::redact`,
  `src/utils/redact.ts`). The Rust templates in `temp-release/` get this when they move
  to SDK 4.0.
