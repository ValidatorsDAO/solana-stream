# Contributing

Bug reports and pull requests are welcome on
[GitHub](https://github.com/ValidatorsDAO/solana-stream/issues). Report security issues
privately as described in [SECURITY.md](SECURITY.md).

## Checks

CI runs these on every pull request; run them locally before you push:

```bash
cargo test --workspace --locked
cargo deny check
pnpm install --frozen-lockfile && pnpm audit
node scripts/check-erpc-endpoints.mjs
bash scripts/check-oss-hygiene.sh
bash scripts/tests/check-oss-hygiene.test.sh
bash scripts/run-gitleaks.sh git --redact --config .gitleaks.toml --log-opts=--all .
```

## Public repository hygiene

This repository is public. `scripts/check-oss-hygiene.sh` (from elsoul/erpc-sdk) blocks:

- `.env` files (the `.env.example` / `.env.sample` templates are allowed, and scanned);
- `*.erpc.global` hosts that are not in `scripts/oss-public-hosts.txt`, the public hosts
  the ERPC dashboard and the slv.dev quickstarts give out. Add a host there and to
  `scripts/check-erpc-endpoints.mjs` together;
- references to other repositories of the `elsoul` organization, internal documents and
  developer home paths;
- credential assignments, bearer tokens, private keys and Solana keypair arrays.

A genuine false positive is fixed in the script, in the same commit, with the reason in
the commit message.

## Templates

`slv bot init` copies `temp-release/<template>/` from `main`, and those templates depend
on the published packages. A change to them reaches users as soon as it merges.
