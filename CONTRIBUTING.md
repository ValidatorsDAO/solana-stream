# Contributing

Bug reports and pull requests are welcome on
[GitHub](https://github.com/ValidatorsDAO/solana-stream/issues). Report security issues
privately as described in [SECURITY.md](SECURITY.md).

## Checks

CI runs these on every pull request; run them locally before you push:

```bash
cargo test --workspace --locked
cargo test -p solana-stream-sdk --features udp --locked
cargo tree -p solana-stream-sdk --features udp -e normal,build --target all --locked --prefix none \
  | bash scripts/check-udp-deps.sh
bash scripts/tests/check-udp-deps.test.sh
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

## The `udp` feature's dependencies

`scripts/check-udp-deps.sh` fails when `solana-stream-sdk/udp` builds solana-ledger or the
crates that came with it (Agave's validator crates, RocksDB and bindgen, OpenSSL and native
TLS, bzip2) or `ed25519-dalek` below 2.0.0 / `curve25519-dalek` below 4.1.3, the two
advisories `deny.toml` ignores because they reach only the dev-dependency solana-ledger.
Change its lists together with `scripts/tests/check-udp-deps.test.sh`.

## Templates

`slv bot init` copies `temp-release/<template>/` from `main`, and those templates depend
on the published packages. A change to them reaches users as soon as it merges.
