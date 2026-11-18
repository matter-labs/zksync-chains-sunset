# zksync-lite-sunset

zkSync Lite sunset claim project.

This repository contains the services and contracts used for the zkSync Lite sunset migration and claim flow. It includes:

- a Rust backend for sunset claim APIs and processing,
- a frontend app for the claim user experience,
- contract and infra code used by the migration/claim pipeline.

## Getting Started

Clone with submodules (required):

```bash
git clone --recurse-submodules https://github.com/matter-labs/zksync-chains-sunset.git
```

If you already cloned without submodules, run:

```bash
git submodule update --init --recursive
```

## Required Tooling

- [`just`](https://github.com/casey/just) task runner.
- `rustup` + Rust toolchain `1.93.0` (see `rust-toolchain.toml`, includes `rustfmt` and `clippy`).
- `bun` (used by the frontend for install/dev/build/test tasks).
- `foundry` (`forge`/`cast`) for contract tasks under `contracts/zksync-lite-migration-claims`.
- `docker` + Docker Compose and `curl` for infra compose workflows (`just infra ...`).

## Development

First, verify required local toolchains are installed and install frontend dependencies:

```bash
just setup
```

Start local development from the repo root:

```bash
just dev
```

`just dev` starts the development backend and frontend together.

To start the seeded dev/e2e stack for a funded address you have imported into your browser wallet:

```bash
just dev-seeded-address 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
```

That flow now starts the compose-backed seeded local stack:

- L1 Anvil on `31338` (`Local Seeded Fork`, RPC `http://127.0.0.1:5010`)
- seeded backend plus both frontend previews

It also generates fixture CSVs, imports snapshot data, materializes claims/proofs, deploys the local contracts, and wires the backend to the discovered dev token addresses.

With a funded wallet connected in the browser, the compose-backed local flow now works end to end on the same local chain: migrate first, then claim the migrated balances from the L1 claim contract.

The compose-backed local L1 uses chain id `31338`, so the wallet must be on `Local Seeded Fork` with RPC `http://127.0.0.1:5010`, not public Sepolia. The frontend now forces an explicit switch attempt and blocks with a clear error if MetaMask is still pointed at the wrong RPC.

For WalletConnect testing with consumer mobile wallets that do not support the
local custom chain, use the mainnet-chain-id variant:

```bash
just dev-seeded-mainnet-walletconnect 0xYourMobileWalletAddress
```

This keeps backend execution on the local seeded Anvil stack, but makes the
wallet-facing chain ID `1` so the mobile wallet can stay on Ethereum Mainnet.

To migrate every pending token in the running seeded stack without clicking
through the migrate UI, use the Anvil-funded helper:

```bash
just dev-migrate-all-tokens
```

That command prepares each migration through the backend, sends the returned
calldata from an Anvil rich wallet, records each tx hash with the backend, and
waits for `mined_success`.

`CLAIMANT_PRIVATE_KEY` is not required for `just dev-seeded-address`. The browser wallet signs the claim batch. That private-key input is only used by the automated smoke path (`just dev-seeded-smoke` / `just infra compose-seeded-smoke`) where no real wallet UI is present.

For arbitrary seeded addresses that are not backed by a wallet you can actually use in the browser, the stack can still fully prepare migration and claim payloads, and L1 migration can be exercised locally via Anvil impersonation. Final claim submission still requires the claimant's real wallet because the browser-driven flow is what authorizes the request.

If you explicitly want a prep-only seeded session for an unsignable address, set `FIXTURE_MODE=all-unmigrated`.
