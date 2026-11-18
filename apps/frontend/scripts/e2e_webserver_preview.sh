#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"

export CLAIM_FRONTEND_HOST="${CLAIM_FRONTEND_HOST:-127.0.0.1}"
export CLAIM_FRONTEND_PORT="${CLAIM_FRONTEND_PORT:-4521}"
export MIGRATE_FRONTEND_HOST="${MIGRATE_FRONTEND_HOST:-127.0.0.1}"
export MIGRATE_FRONTEND_PORT="${MIGRATE_FRONTEND_PORT:-4522}"
export PUBLIC_CLAIM_APP_URL="${PUBLIC_CLAIM_APP_URL:-${PLAYWRIGHT_CLAIM_APP_URL:-http://${CLAIM_FRONTEND_HOST}:${CLAIM_FRONTEND_PORT}}}"
export PUBLIC_MIGRATE_APP_URL="${PUBLIC_MIGRATE_APP_URL:-${PLAYWRIGHT_MIGRATE_APP_URL:-http://${MIGRATE_FRONTEND_HOST}:${MIGRATE_FRONTEND_PORT}}}"

# The mocked E2E suite does not spin up the seeded-fork compose stack, so
# `enrichMigrationTokens` and `enrichEligibilityTokens` still need
# syntactically valid contract addresses baked into the build. Tests intercept
# the chain RPC and return canned multicall3 responses, so any well-formed
# address works here.
export PUBLIC_MIGRATION_CONTRACT_ADDRESS="${PUBLIC_MIGRATION_CONTRACT_ADDRESS:-0x1111111111111111111111111111111111111111}"
export PUBLIC_CLAIM_CONTRACT_ADDRESS="${PUBLIC_CLAIM_CONTRACT_ADDRESS:-0x2222222222222222222222222222222222222222}"

cleanup() {
  kill 0 >/dev/null 2>&1 || true
}
trap cleanup INT TERM EXIT

cd "${ROOT_DIR}/apps/frontend"
bun run build >/dev/null
bun run preview:claim &
bun run preview:migrate &
wait
