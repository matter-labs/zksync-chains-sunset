# Infra

Local dev and CI smoke testing runs the backend server via Docker Compose.

## Commands
- `just infra compose-up`
- `just infra compose-smoke`
- `just infra compose-seeded-smoke`
- `just infra compose-debug`
- `just infra compose-logs`
- `just infra compose-down`

## Full Flow Smoke
- `just infra compose-seeded-smoke` boots the same compose-backed seeded stack as `just dev-seeded-address`, runs a focused real funded-wallet smoke that covers both one representative ERC20 migration+claim and the native ETH migration+claim path, verifies post-claim state, and prints a JSON summary. The smoke now lives in the Rust CLI as `zksync-lite-sunset-cli compose-seeded-smoke`.
- By default it uses the funded Anvil wallet `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266`.
- Pass `keep_stack=true` to leave the seeded stack process running after the smoke succeeds.
- Pass `teardown=false` if you do not want the recipe to tear down a compose stack that it started from cold state.
- Browser-level validation for the same seeded flow lives in `just test-e2e-compose` / `just frontend test-e2e-compose`.

## Debug Surface
- `just infra compose-debug` prints the active compose contracts env plus the live claim contract balances for ETH, USDC, DAI, and USDT.
- By default it reads `/tmp/zksync-lite-sunset-compose-contracts.env`.

## Optional Forking
- Set `ETH_SEPOLIA_RPC_URL` if you want the local Anvil runtime to fork Sepolia.
- You can provide it either via your shell environment or via the default local dotenv path `infra/.env.local`. A checked-in template lives at `infra/.env.example`.
- GitHub Actions can provide `ETH_SEPOLIA_RPC_URL` for the compose-backed frontend and infra workflows, but it is no longer required for the seeded local stack.
- Without `ETH_SEPOLIA_RPC_URL`, the compose-backed seeded stack still runs against a blank local Anvil chain.

## Healthcheck
- Default URL: `http://127.0.0.1:3000/healthz`
- Override via `LOCAL_DEV_HEALTH_URL`
