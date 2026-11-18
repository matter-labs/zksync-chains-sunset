#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
POSTGRES_HOST="${POSTGRES_HOST:-127.0.0.1}"
POSTGRES_PORT="${POSTGRES_PORT:-5432}"
BACKEND_DATABASE_URL="${BACKEND_DATABASE_URL:-postgres://postgres:postgres@${POSTGRES_HOST}:${POSTGRES_PORT}/zksync_lite_sunset_mock}"
START_POSTGRES="${START_POSTGRES:-1}"
DEV_CHAIN_ID="${DEV_CHAIN_ID:-31337}"

cleanup() {
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

if [[ "${START_POSTGRES}" == "1" ]]; then
  echo "Starting postgres via compose..." >&2
  docker compose -f "${ROOT_DIR}/infra/compose.yml" up -d postgres

  for _ in $(seq 1 30); do
    if nc -z "${POSTGRES_HOST}" "${POSTGRES_PORT}" >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done

  if ! nc -z "${POSTGRES_HOST}" "${POSTGRES_PORT}" >/dev/null 2>&1; then
    echo "Postgres did not start on ${POSTGRES_HOST}:${POSTGRES_PORT}" >&2
    exit 1
  fi

  DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx database create
fi

echo "Starting backend server in mock mode..." >&2
(
  cd "${ROOT_DIR}/apps/backend"
  ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL="${BACKEND_DATABASE_URL}" \
  ZKSYNC_LITE_SUNSET__CORE__DATABASE__AUTO_MIGRATE=true \
  ZKSYNC_LITE_SUNSET__CORE__AUTH__MIGRATE_CHAIN_ID="${DEV_CHAIN_ID}" \
  ZKSYNC_LITE_SUNSET__CORE__AUTH__CLAIM_CHAIN_ID="${DEV_CHAIN_ID}" \
  ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL="http://127.0.0.1:8545" \
  ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY="0x1111111111111111111111111111111111111111111111111111111111111111" \
  ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS="0x1111111111111111111111111111111111111111" \
  ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS="0x2222222222222222222222222222222222222222" \
  RUST_LOG=info \
  cargo run -p zksync-lite-sunset-server
) &
BACKEND_PID=$!

echo "Starting claim frontend dev server..." >&2
(
  cd "${ROOT_DIR}/apps/frontend"
  PUBLIC_MIGRATION_CONTRACT_ADDRESS="0x1111111111111111111111111111111111111111" \
  PUBLIC_CLAIM_CONTRACT_ADDRESS="0x2222222222222222222222222222222222222222" \
  bun run dev:claim
) &
CLAIM_FRONTEND_PID=$!

echo "Starting migrate frontend dev server..." >&2
(
  cd "${ROOT_DIR}/apps/frontend"
  PUBLIC_MIGRATION_CONTRACT_ADDRESS="0x1111111111111111111111111111111111111111" \
  PUBLIC_CLAIM_CONTRACT_ADDRESS="0x2222222222222222222222222222222222222222" \
  bun run dev:migrate
) &
MIGRATE_FRONTEND_PID=$!

wait -n "${BACKEND_PID}" "${CLAIM_FRONTEND_PID}" "${MIGRATE_FRONTEND_PID}"
