# zksync-lite-sunset-cli

CLI entrypoint for zksync-lite-sunset.

## Dev Snapshot Commands

Generate a minimal CSV fixture for a seeded address:

```sh
cargo run -p zksync-lite-sunset-cli -- \
  generate-dev-snapshot \
  --address 0xBa98deEFBca8E585208B252f39f997733aB65072 \
  --out-dir /tmp/zksync-lite-snapshot \
  --mode default \
  --json
```

Generate CSVs, import them, materialize claims/proofs, and seed runtime state:

```sh
cargo run -p zksync-lite-sunset-cli -- \
  dev-seed-snapshot \
  --database-url postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset_test \
  --address 0xBa98deEFBca8E585208B252f39f997733aB65072 \
  --mode default \
  --snapshot-label snapshot-dev-seeded-address \
  --json
```

Run the compose-backed funded-wallet migrate-plus-claim smoke:

```sh
cargo run -p zksync-lite-sunset-cli -- \
  compose-seeded-smoke \
  --address 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --teardown-compose
```

This smoke covers both:
- one representative ERC20 path selected by `--token-symbol` (default `DAI`)
- the native `ETH` path

## Notes
- `src/lib.rs` wires CLI parsing and dispatch.
- command implementations live in focused modules such as `src/compose_seeded_smoke.rs`.
- `src/main.rs` is a thin wrapper that calls into the library.
- Fixture generation, snapshot import, and claim materialization business logic live in `zksync-lite-sunset-core`; this crate exposes that logic through CLI commands.
