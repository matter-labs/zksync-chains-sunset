# Backend Workspace

Local Rust workspace for backend components.

## Layout
- `crates/zksync-lite-sunset-cli`: CLI binary + library (uses `zksync-lite-sunset-core`)
- `crates/zksync-lite-sunset-core`: shared core library used by CLI and server
- `crates/zksync-lite-sunset-server`: server binary + library (uses `zksync-lite-sunset-core`)

## Common Commands
Run from this directory:
- `just build`
- `just test`
- `just test-db-up` (starts local postgres for tests via compose)
- `just test-db-down`
- `just fmt`
- `just fmt-check`
- `just clippy`
- `just clippy-fix`
- `just fix`
- `just generate-dev-snapshot <address> <out_dir>`
- `just dev-seed-snapshot <database_url> <address>`

`just test` uses `ZKSYNC_LITE_SUNSET_TEST_DATABASE_URL` if set, otherwise defaults to:
`postgres://postgres:postgres@127.0.0.1:5432/zksync_lite_sunset_test`

Supported end-to-end local runtime validation lives in the compose-backed
seeded smoke flow, for example:

- `just dev-seeded-smoke`
- `cargo run --manifest-path apps/backend/Cargo.toml -p zksync-lite-sunset-cli -- compose-seeded-smoke --teardown-compose`
- `cargo run --manifest-path apps/backend/Cargo.toml -p zksync-lite-sunset-cli -- compose-seeded-smoke --token-symbol USDC --teardown-compose`

`compose-seeded-smoke` now covers both key runtime branches in a single run:
- one representative ERC20 path selected by `--token-symbol`
- the native `ETH` path

## Notes
- Workspace config lives in `apps/backend/Cargo.toml`.
- Common commands live in `apps/backend/justfile`.
- The seeded-address dev flow now goes through the Rust CLI: CSV generation, snapshot import, and claim materialization all use backend commands rather than frontend TypeScript fixtures.
