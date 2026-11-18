#!/usr/bin/env bash
# Resume the compose-backed stack (backend + frontends) against a preexisting L1.
#
# Unlike run-compose-seeded-stack.sh, this script does NOT:
#   - reset the postgres database (schema + snapshot are preserved)
#   - start the in-compose anvil service
#   - redeploy the migration/claim contracts
#
# It requires a prior successful run of run-compose-seeded-stack.sh (or the
# equivalent bootstrap) so the contracts env file at
# ${CONTRACTS_ENV_HOST_FILE} exists and the addresses it points to are already
# deployed on the target L1.
#
# Arguments / env vars:
#   CHAIN_RPC_URL           L1 RPC URL reachable from the host (required).
#   BACKEND_CHAIN_RPC_URL   L1 RPC URL reachable from inside Docker; defaults to
#                           CHAIN_RPC_URL. If CHAIN_RPC_URL is localhost/127.0.0.1,
#                           set this to http://host.docker.internal:<port>.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE_FILE="${COMPOSE_FILE:-${ROOT_DIR}/infra/compose.yml}"
INFRA_ENV_FILE="${INFRA_ENV_FILE:-${ROOT_DIR}/infra/.env.local}"
COMPOSE_ENV_FILE=""
TEMP_COMPOSE_ENV_FILE=""
ORIGINAL_STTY=""
INTERRUPTED=0

POSTGRES_HOST="${POSTGRES_HOST:-127.0.0.1}"
POSTGRES_PORT="${POSTGRES_PORT:-5432}"
BACKEND_HOST="${BACKEND_HOST:-127.0.0.1}"
BACKEND_PORT="${BACKEND_PORT:-3000}"
BACKEND_BASE_URL="http://${BACKEND_HOST}:${BACKEND_PORT}"
BACKEND_DATABASE_URL="${BACKEND_DATABASE_URL:-postgres://postgres:postgres@${POSTGRES_HOST}:${POSTGRES_PORT}/zksync_lite_sunset}"
CLAIM_FRONTEND_HOST="${CLAIM_FRONTEND_HOST:-127.0.0.1}"
CLAIM_FRONTEND_PORT="${CLAIM_FRONTEND_PORT:-4321}"
CLAIM_FRONTEND_URL="${CLAIM_FRONTEND_URL:-http://${CLAIM_FRONTEND_HOST}:${CLAIM_FRONTEND_PORT}}"
MIGRATE_FRONTEND_HOST="${MIGRATE_FRONTEND_HOST:-127.0.0.1}"
MIGRATE_FRONTEND_PORT="${MIGRATE_FRONTEND_PORT:-4322}"
MIGRATE_FRONTEND_URL="${MIGRATE_FRONTEND_URL:-http://${MIGRATE_FRONTEND_HOST}:${MIGRATE_FRONTEND_PORT}}"

if [[ -z "${CHAIN_RPC_URL:-}" ]]; then
  echo "CHAIN_RPC_URL is required (pass the L1 RPC URL reachable from the host)" >&2
  exit 1
fi
BACKEND_CHAIN_RPC_URL="${BACKEND_CHAIN_RPC_URL:-${CHAIN_RPC_URL}}"

CONTRACTS_ENV_HOST_FILE="${CONTRACTS_ENV_HOST_FILE:-/tmp/zksync-lite-sunset-compose-contracts.env}"
COMPOSE_BUILD_SERVER="${COMPOSE_BUILD_SERVER:-true}"
DEV_CHAIN_ID="${DEV_CHAIN_ID:-31338}"

declare -a BACKEND_CORS_ALLOWED_ORIGINS=()

append_cors_origin() {
  local origin="$1"
  local existing

  for existing in "${BACKEND_CORS_ALLOWED_ORIGINS[@]:-}"; do
    if [[ "${existing}" == "${origin}" ]]; then
      return 0
    fi
  done

  BACKEND_CORS_ALLOWED_ORIGINS+=("${origin}")
}

append_cors_origin_variants_for_url() {
  local url="$1"
  local scheme remainder host_and_port host port

  scheme="${url%%://*}"
  remainder="${url#*://}"
  host_and_port="${remainder%%/*}"
  host="${host_and_port%:*}"
  port="${host_and_port##*:}"

  append_cors_origin "${scheme}://${host}:${port}"

  case "${host}" in
    127.0.0.1)
      append_cors_origin "${scheme}://localhost:${port}"
      ;;
    localhost)
      append_cors_origin "${scheme}://127.0.0.1:${port}"
      ;;
  esac
}

build_backend_cors_allowed_origins() {
  BACKEND_CORS_ALLOWED_ORIGINS=()
  append_cors_origin_variants_for_url "${CLAIM_FRONTEND_URL}"
  append_cors_origin_variants_for_url "${MIGRATE_FRONTEND_URL}"
  (
    IFS=,
    printf '%s' "${BACKEND_CORS_ALLOWED_ORIGINS[*]}"
  )
}

ZKSYNC_LITE_SUNSET__CORS__ALLOWED_ORIGINS="${ZKSYNC_LITE_SUNSET__CORS__ALLOWED_ORIGINS:-$(build_backend_cors_allowed_origins)}"
export ZKSYNC_LITE_SUNSET__CORS__ALLOWED_ORIGINS

if [[ -t 0 ]]; then
  ORIGINAL_STTY="$(stty -g 2>/dev/null || true)"
fi

ensure_compose_env_file() {
  if [[ -f "${INFRA_ENV_FILE}" ]]; then
    COMPOSE_ENV_FILE="${INFRA_ENV_FILE}"
    return 0
  fi

  if [[ -z "${ETH_SEPOLIA_RPC_URL:-}" ]]; then
    COMPOSE_ENV_FILE=""
    return 0
  fi

  TEMP_COMPOSE_ENV_FILE="$(mktemp "${TMPDIR:-/tmp}/zksync-lite-sunset-compose-env.XXXXXX")"
  printf 'ETH_SEPOLIA_RPC_URL=%s\n' "${ETH_SEPOLIA_RPC_URL}" > "${TEMP_COMPOSE_ENV_FILE}"
  COMPOSE_ENV_FILE="${TEMP_COMPOSE_ENV_FILE}"
}

compose() {
  if [[ -n "${COMPOSE_ENV_FILE}" ]]; then
    docker compose --env-file "${COMPOSE_ENV_FILE}" -f "${COMPOSE_FILE}" "$@"
  else
    docker compose -f "${COMPOSE_FILE}" "$@"
  fi
}

load_contracts_env_file() {
  if [[ ! -f "${CONTRACTS_ENV_HOST_FILE}" ]]; then
    echo "missing contracts env file: ${CONTRACTS_ENV_HOST_FILE}" >&2
    echo "run \`just dev\` (or another seeded bootstrap) at least once before resuming." >&2
    exit 1
  fi

  set -a
  # shellcheck disable=SC1090
  source "${CONTRACTS_ENV_HOST_FILE}"
  set +a
}

cleanup() {
  if [[ -n "${CLAIM_FRONTEND_PID:-}" ]]; then
    kill "${CLAIM_FRONTEND_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${MIGRATE_FRONTEND_PID:-}" ]]; then
    kill "${MIGRATE_FRONTEND_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${CLAIM_FRONTEND_PID:-}" || -n "${MIGRATE_FRONTEND_PID:-}" ]]; then
    wait "${CLAIM_FRONTEND_PID:-}" "${MIGRATE_FRONTEND_PID:-}" 2>/dev/null || true
  fi
  if [[ -n "${TEMP_COMPOSE_ENV_FILE:-}" ]]; then
    rm -f "${TEMP_COMPOSE_ENV_FILE}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${ORIGINAL_STTY}" && -t 0 ]]; then
    stty "${ORIGINAL_STTY}" >/dev/null 2>&1 || true
  fi
  if [[ -t 0 ]]; then
    stty -pendin >/dev/null 2>&1 || true
  fi
  if [[ -w /dev/tty ]]; then
    tput smkx >/dev/tty 2>/dev/null || printf '\033[?1h\033=' >/dev/tty || true
  fi
}

handle_interrupt() {
  INTERRUPTED=1
  exit 130
}

trap cleanup EXIT
trap handle_interrupt INT TERM

start_frontend_preview() {
  local target="$1"

  (
    cd "${ROOT_DIR}/apps/frontend"
    PUBLIC_API_BASE_URL="${BACKEND_BASE_URL}" \
    PUBLIC_CLAIM_APP_URL="${CLAIM_FRONTEND_URL}" \
    PUBLIC_MIGRATE_APP_URL="${MIGRATE_FRONTEND_URL}" \
    PUBLIC_CHAIN_RPC_URL="${CHAIN_RPC_URL}" \
    PUBLIC_CHAIN_ID="${DEV_CHAIN_ID}" \
    PUBLIC_MIGRATION_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
    PUBLIC_CLAIM_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
    bun run "preview:${target}" </dev/null
  ) &
}

wait_for_tcp() {
  local host="$1"
  local port="$2"
  local label="$3"

  for _ in $(seq 1 180); do
    if nc -z "${host}" "${port}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "${label} did not become ready on ${host}:${port}" >&2
  exit 1
}

wait_for_http() {
  local url="$1"
  local label="$2"

  for _ in $(seq 1 180); do
    if curl \
      --fail \
      --show-error \
      --silent \
      --connect-timeout 1 \
      --max-time 3 \
      "${url}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "${label} did not become ready at ${url}" >&2
  exit 1
}

wait_for_postgres_ready() {
  for _ in $(seq 1 180); do
    if compose exec -T postgres pg_isready -U postgres -d postgres >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "postgres did not become ready for commands" >&2
  exit 1
}

ensure_postgres() {
  if nc -z "${POSTGRES_HOST}" "${POSTGRES_PORT}" >/dev/null 2>&1; then
    return 0
  fi

  compose up -d --remove-orphans postgres
  wait_for_tcp "${POSTGRES_HOST}" "${POSTGRES_PORT}" "postgres"
}

ensure_database_migrations() {
  wait_for_postgres_ready

  DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx database create
  DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx migrate run \
    --source "${ROOT_DIR}/apps/backend/crates/zksync-lite-sunset-core/migrations"
}

ensure_compose_env_file
ensure_postgres
ensure_database_migrations
load_contracts_env_file

echo "[compose-resume] backend RPC (from container): ${BACKEND_CHAIN_RPC_URL}" >&2
echo "[compose-resume] frontend RPC (from browser): ${CHAIN_RPC_URL}" >&2

export ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL="${BACKEND_CHAIN_RPC_URL}"
echo "[compose-resume] starting backend" >&2
if [[ "${COMPOSE_BUILD_SERVER}" == "true" ]]; then
  compose up -d --build --no-deps --force-recreate --remove-orphans zksync-lite-sunset-server
else
  compose up -d --no-deps --force-recreate --remove-orphans zksync-lite-sunset-server
fi
echo "[compose-resume] waiting for backend" >&2
COMPOSE_FILE="${COMPOSE_FILE}" \
INFRA_ENV_FILE="${INFRA_ENV_FILE}" \
COMPOSE_ENV_FILE="${COMPOSE_ENV_FILE}" \
PUBLIC_HEALTH_URL="${BACKEND_BASE_URL}/healthz" \
PUBLIC_READY_URL="${BACKEND_BASE_URL}/readyz" \
bash "${ROOT_DIR}/scripts/dev/check-compose-backend-health.sh"

echo "[compose-resume] building claim frontend" >&2
PUBLIC_API_BASE_URL="${BACKEND_BASE_URL}" \
PUBLIC_CLAIM_APP_URL="${CLAIM_FRONTEND_URL}" \
PUBLIC_MIGRATE_APP_URL="${MIGRATE_FRONTEND_URL}" \
PUBLIC_CHAIN_RPC_URL="${CHAIN_RPC_URL}" \
PUBLIC_CHAIN_ID="${DEV_CHAIN_ID}" \
PUBLIC_MIGRATION_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
PUBLIC_CLAIM_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
just frontend build-claim

echo "[compose-resume] building migrate frontend" >&2
PUBLIC_API_BASE_URL="${BACKEND_BASE_URL}" \
PUBLIC_CLAIM_APP_URL="${CLAIM_FRONTEND_URL}" \
PUBLIC_MIGRATE_APP_URL="${MIGRATE_FRONTEND_URL}" \
PUBLIC_CHAIN_RPC_URL="${CHAIN_RPC_URL}" \
PUBLIC_CHAIN_ID="${DEV_CHAIN_ID}" \
PUBLIC_MIGRATION_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
PUBLIC_CLAIM_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
just frontend build-migrate

echo "[compose-resume] starting claim frontend preview" >&2
start_frontend_preview claim
CLAIM_FRONTEND_PID=$!

echo "[compose-resume] starting migrate frontend preview" >&2
start_frontend_preview migrate
MIGRATE_FRONTEND_PID=$!

echo "[compose-resume] waiting for frontends" >&2
wait_for_http "${CLAIM_FRONTEND_URL}/" "claim frontend"
wait_for_http "${MIGRATE_FRONTEND_URL}/" "migrate frontend"

echo "[compose-resume] claim frontend: ${CLAIM_FRONTEND_URL}/" >&2
echo "[compose-resume] migrate frontend: ${MIGRATE_FRONTEND_URL}/" >&2
echo "[compose-resume] backend: ${BACKEND_BASE_URL}/" >&2
echo "[compose-resume] ready"
wait
