# Root task entrypoints. Keep this minimal; delegate to subprojects.

# List tasks
list:
  @just --list

# Setup local toolchain + project dependencies
setup:
  @bash scripts/setup.sh

# Dev (compose-backed single-chain local stack seeded from the multi-claimant ci-snapshot fixture).
dev:
  @SNAPSHOT_SOURCE=csv REAL_DATA_DIR=apps/backend/fixtures/semi_real_data bash scripts/dev/run-compose-seeded-stack.sh

# Legacy host-local single-RPC dev stack kept only for comparison/debugging.
dev-host-legacy:
  @bash scripts/dev/run-dev-stack.sh

# Dev mock (run backend + frontend with mock execution mode)
dev-mock:
  @bash scripts/dev/run-dev-mock-stack.sh

# Resume the compose-backed stack against a preexisting L1 without wiping the
# database or redeploying contracts. Requires a prior seeded bootstrap so the
# contracts env file (/tmp/zksync-lite-sunset-compose-contracts.env by default)
# exists and its addresses are already deployed on the target L1.
#
# If l1_url is localhost/127.0.0.1, also set BACKEND_CHAIN_RPC_URL to a URL the
# backend container can reach (e.g. http://host.docker.internal:<port>).
dev-resume l1_url:
  @CHAIN_RPC_URL="{{l1_url}}" bash scripts/dev/run-compose-resume-stack.sh || test $? -eq 130

# Compose-backed frontend browser stack on the supported seeded runtime.
dev-e2e:
  @bash apps/frontend/scripts/e2e_webserver_compose.sh

# Dev e2e for a specific address. The seed path generates CSV fixtures and
# materializes the active snapshot via the Rust CLI. For manual browser testing,
# use a wallet you can actually sign with. This manual path defaults to an
# all-unmigrated fixture so claim/migrate state only changes after you perform
# the migration in the UI. For automated funded-wallet coverage, use
# `just dev-seeded-smoke`.
dev-seeded-address address:
  @CLAIMANT="{{address}}" bash scripts/dev/run-compose-seeded-stack.sh || test $? -eq 130

# Mainnet-chain-id variant of the seeded stack for WalletConnect mobile-wallet
# sponsored-claim testing. The backend/browser still use the local seeded Anvil
# RPC, but Anvil reports chain id 1 and wallet-facing metadata points at a
# standard public mainnet RPC so consumer wallets do not need a custom network.
dev-seeded-mainnet-walletconnect address:
  @CLAIMANT="{{address}}" DEV_CHAIN_ID=1 WALLET_CHAIN_RPC_URL=https://ethereum.publicnode.com bash scripts/dev/run-compose-seeded-stack.sh || test $? -eq 130

# Migrate every currently pending token on the running seeded stack using an
# Anvil funded account. Run this after `just dev-seeded-address ...` when you
# want to set up the withdraw flow without clicking through the migrate UI.
dev-migrate-all-tokens:
  @bash scripts/dev/migrate-all-tokens.sh

# Automated funded-wallet smoke for the compose-backed seeded flow.
# This is the path that intentionally uses a claimant private key.
dev-seeded-smoke address="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266" claimant_private_key="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80" keep_stack="false" teardown="true":
  @just -f infra/justfile compose-seeded-smoke "{{address}}" "{{claimant_private_key}}" "{{keep_stack}}" "{{teardown}}"

# Compose-backed frontend browser e2e on the seeded local stack.
test-e2e:
  @just -f apps/frontend/justfile test-e2e

# Explicit alias for the supported compose-backed browser e2e path.
test-e2e-compose:
  @just -f apps/frontend/justfile test-e2e-compose

# Lightweight frontend-only browser tests with mocked wallet/API traffic.
test-e2e-ui:
  @just -f apps/frontend/justfile test-e2e-ui

# Fix (backend Rust autofix path: fmt + clippy --fix)
fix:
  @just -f apps/backend/justfile fix

# Check (backend Rust validation path)
check:
  @just -f apps/backend/justfile check

# Test (backend default test suite via nextest).
# Agent note: run this with escalated permissions because the backend
# integration tests spawn local infra like Anvil, which may be blocked by
# the sandbox.
test:
  @just -f apps/backend/justfile test

# Backend (delegate to apps/backend/justfile)
backend *args:
  @if [ -z "{{args}}" ]; then \
    just -f apps/backend/justfile --list; \
  else \
    just -f apps/backend/justfile {{args}}; \
  fi

# Frontend (placeholder)
frontend *args:
  @if [ -z "{{args}}" ]; then \
    just -f apps/frontend/justfile --list; \
  else \
    just -f apps/frontend/justfile {{args}}; \
  fi

# Contracts (delegate to contracts/justfile)
contracts *args:
  @if [ -z "{{args}}" ]; then \
    just -f contracts/justfile --list; \
  else \
    just -f contracts/justfile {{args}}; \
  fi

# Infra (delegate to infra/justfile)
infra *args:
  @if [ -z "{{args}}" ]; then \
    just -f infra/justfile --list; \
  else \
    just -f infra/justfile {{args}}; \
  fi
