#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ANVIL_HOST="${ANVIL_HOST:-127.0.0.1}"
ANVIL_PORT="${ANVIL_PORT:-8545}"
ANVIL_RPC_URL="${ANVIL_RPC_URL:-http://${ANVIL_HOST}:${ANVIL_PORT}}"
ANVIL_LOG_FILE="${ANVIL_LOG_FILE:-/tmp/zksync-lite-sunset-anvil.log}"
ENV_FILE="${OUT_ENV_FILE:-/tmp/zksync-lite-sunset-dev.env}"
DEV_CHAIN_ID="${DEV_CHAIN_ID:-31337}"

if [[ -z "${MAINNET_RPC_URL:-}" ]]; then
  echo "MAINNET_RPC_URL is required to run the forked dev stack." >&2
  exit 1
fi

cleanup() {
  if [[ -n "${ANVIL_PID:-}" ]]; then
    kill "${ANVIL_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${BACKEND_PID:-}" ]]; then
    kill "${BACKEND_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${CLAIM_FRONTEND_PID:-}" ]]; then
    kill "${CLAIM_FRONTEND_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${MIGRATE_FRONTEND_PID:-}" ]]; then
    kill "${MIGRATE_FRONTEND_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

echo "Starting Anvil mainnet fork at ${ANVIL_RPC_URL}..." >&2
anvil --host "${ANVIL_HOST}" --port "${ANVIL_PORT}" --chain-id "${DEV_CHAIN_ID}" --fork-url "${MAINNET_RPC_URL}" >"${ANVIL_LOG_FILE}" 2>&1 &
ANVIL_PID=$!

for _ in $(seq 1 30); do
  if nc -z "${ANVIL_HOST}" "${ANVIL_PORT}" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! nc -z "${ANVIL_HOST}" "${ANVIL_PORT}" >/dev/null 2>&1; then
  echo "Anvil did not start. See ${ANVIL_LOG_FILE}" >&2
  exit 1
fi

echo "Deploying dev contracts to fork..." >&2
echo "WARNING: scripts/dev/run-dev-stack.sh is a legacy single-RPC dev path. Prefer just dev-seeded-address or just dev-seeded-smoke." >&2
ALLOW_LEGACY_SINGLE_RPC_DEV=1 \
ANVIL_RPC_URL="${ANVIL_RPC_URL}" DEV_CHAIN_ID="${DEV_CHAIN_ID}" OUT_ENV_FILE="${ENV_FILE}" "${ROOT_DIR}/scripts/dev/deploy-fork-contracts.sh"
source "${ENV_FILE}"

if [[ -n "${ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL:-}" ]]; then
  echo "Running db-init..." >&2
  (
    cd "${ROOT_DIR}/apps/backend"
    cargo run -p zksync-lite-sunset-cli -- db-init --database-url "${ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL}"
  )
fi

echo "Starting backend server..." >&2
(
  cd "${ROOT_DIR}/apps/backend"
  RUST_LOG=info cargo run -p zksync-lite-sunset-server
) &
BACKEND_PID=$!

echo "Starting claim frontend dev server..." >&2
(
  cd "${ROOT_DIR}/apps/frontend"
  bun run dev:claim
) &
CLAIM_FRONTEND_PID=$!

echo "Starting migrate frontend dev server..." >&2
(
  cd "${ROOT_DIR}/apps/frontend"
  bun run dev:migrate
) &
MIGRATE_FRONTEND_PID=$!

wait -n "${BACKEND_PID}" "${CLAIM_FRONTEND_PID}" "${MIGRATE_FRONTEND_PID}" "${ANVIL_PID}"
