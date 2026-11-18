#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCAL_ENV_FILE="${LOCAL_ENV_FILE:-${ROOT_DIR}/.env.local}"

if [[ -f "${LOCAL_ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${LOCAL_ENV_FILE}"
  set +a
fi

COMPOSE_FILE="${COMPOSE_FILE:-${ROOT_DIR}/infra/compose.yml}"
INFRA_ENV_FILE="${INFRA_ENV_FILE:-${ROOT_DIR}/infra/.env.local}"
COMPOSE_ENV_FILE=""
TEMP_COMPOSE_ENV_FILE=""
ORIGINAL_STTY=""
INTERRUPTED=0
PUBLIC_WALLETCONNECT_PROJECT_ID="${PUBLIC_WALLETCONNECT_PROJECT_ID:-}"
WALLETCONNECT_DEV_HOST="${WALLETCONNECT_DEV_HOST:-}"

detect_walletconnect_dev_host() {
  if [[ -n "${WALLETCONNECT_DEV_HOST}" ]]; then
    printf '%s\n' "${WALLETCONNECT_DEV_HOST}"
    return 0
  fi

  if command -v ipconfig >/dev/null 2>&1; then
    ipconfig getifaddr en0 2>/dev/null && return 0
  fi

  if command -v hostname >/dev/null 2>&1; then
    hostname -I 2>/dev/null | awk '{print $1}' && return 0
  fi

  return 1
}

if [[ -n "${PUBLIC_WALLETCONNECT_PROJECT_ID}" ]]; then
  WALLETCONNECT_DEV_HOST="$(detect_walletconnect_dev_host || true)"
fi

POSTGRES_HOST="${POSTGRES_HOST:-127.0.0.1}"
POSTGRES_PORT="${POSTGRES_PORT:-5432}"
BACKEND_PORT="${BACKEND_PORT:-3000}"
CLAIM_FRONTEND_PORT="${CLAIM_FRONTEND_PORT:-4321}"
MIGRATE_FRONTEND_PORT="${MIGRATE_FRONTEND_PORT:-4322}"

if [[ -n "${PUBLIC_WALLETCONNECT_PROJECT_ID}" && -n "${WALLETCONNECT_DEV_HOST}" ]]; then
  BACKEND_HOST="${BACKEND_HOST:-127.0.0.1}"
  CLAIM_FRONTEND_HOST="${CLAIM_FRONTEND_HOST:-0.0.0.0}"
  CLAIM_FRONTEND_URL="${CLAIM_FRONTEND_URL:-http://${WALLETCONNECT_DEV_HOST}:${CLAIM_FRONTEND_PORT}}"
  CLAIM_FRONTEND_READY_URL="${CLAIM_FRONTEND_READY_URL:-http://127.0.0.1:${CLAIM_FRONTEND_PORT}}"
  MIGRATE_FRONTEND_HOST="${MIGRATE_FRONTEND_HOST:-0.0.0.0}"
  MIGRATE_FRONTEND_URL="${MIGRATE_FRONTEND_URL:-http://${WALLETCONNECT_DEV_HOST}:${MIGRATE_FRONTEND_PORT}}"
  MIGRATE_FRONTEND_READY_URL="${MIGRATE_FRONTEND_READY_URL:-http://127.0.0.1:${MIGRATE_FRONTEND_PORT}}"
  CHAIN_RPC_URL="${CHAIN_RPC_URL:-${ANVIL_RPC_URL:-http://127.0.0.1:5010}}"
  BROWSER_CHAIN_RPC_URL="${BROWSER_CHAIN_RPC_URL:-${PUBLIC_CHAIN_RPC_URL:-${CHAIN_RPC_URL}}}"
  WALLET_CHAIN_RPC_URL="${WALLET_CHAIN_RPC_URL:-${PUBLIC_WALLET_CHAIN_RPC_URL:-${FRONTEND_CHAIN_RPC_URL:-http://${WALLETCONNECT_DEV_HOST}:5010}}}"
else
  BACKEND_HOST="${BACKEND_HOST:-127.0.0.1}"
  CLAIM_FRONTEND_HOST="${CLAIM_FRONTEND_HOST:-127.0.0.1}"
  CLAIM_FRONTEND_URL="${CLAIM_FRONTEND_URL:-http://${CLAIM_FRONTEND_HOST}:${CLAIM_FRONTEND_PORT}}"
  CLAIM_FRONTEND_READY_URL="${CLAIM_FRONTEND_READY_URL:-${CLAIM_FRONTEND_URL}}"
  MIGRATE_FRONTEND_HOST="${MIGRATE_FRONTEND_HOST:-127.0.0.1}"
  MIGRATE_FRONTEND_URL="${MIGRATE_FRONTEND_URL:-http://${MIGRATE_FRONTEND_HOST}:${MIGRATE_FRONTEND_PORT}}"
  MIGRATE_FRONTEND_READY_URL="${MIGRATE_FRONTEND_READY_URL:-${MIGRATE_FRONTEND_URL}}"
  CHAIN_RPC_URL="${CHAIN_RPC_URL:-${ANVIL_RPC_URL:-http://127.0.0.1:5010}}"
  BROWSER_CHAIN_RPC_URL="${BROWSER_CHAIN_RPC_URL:-${PUBLIC_CHAIN_RPC_URL:-${CHAIN_RPC_URL}}}"
  WALLET_CHAIN_RPC_URL="${WALLET_CHAIN_RPC_URL:-${PUBLIC_WALLET_CHAIN_RPC_URL:-${FRONTEND_CHAIN_RPC_URL:-${BROWSER_CHAIN_RPC_URL}}}}"
fi

BACKEND_BASE_URL="http://${BACKEND_HOST}:${BACKEND_PORT}"
BACKEND_DATABASE_URL="${BACKEND_DATABASE_URL:-postgres://postgres:postgres@${POSTGRES_HOST}:${POSTGRES_PORT}/zksync_lite_sunset}"
BACKEND_CHAIN_RPC_URL="${BACKEND_CHAIN_RPC_URL:-http://anvil:5010}"
DEV_SNAPSHOT_DIR="${DEV_SNAPSHOT_DIR:-/tmp/zksync-lite-sunset-compose-seeded-snapshot}"
CONTRACTS_ENV_HOST_FILE="${CONTRACTS_ENV_HOST_FILE:-/tmp/zksync-lite-sunset-compose-contracts.env}"
# Sepolia/mainnet fork gas prices can make browser-submitted migration txs
# expensive under Anvil's estimated gas cap. This is a local/dev balance target,
# so keep it generous and set it directly instead of relying on normal transfers.
CLAIMANT_GAS_WEI="${CLAIMANT_GAS_WEI:-1000000000000000000000}"
COMPOSE_BUILD_SERVER="${COMPOSE_BUILD_SERVER:-true}"
DEV_CHAIN_ID="${DEV_CHAIN_ID:-31338}"
export DEV_CHAIN_ID
# Anvil only auto-mines one block per tx, so production's 4-confirmation
# default would stall the frontend's `waitForTransactionReceipt` call here.
# Mirror the backend override in infra/compose.yml.
PUBLIC_MIN_TX_CONFIRMATIONS="${PUBLIC_MIN_TX_CONFIRMATIONS:-1}"
# Use anvil account[1] as the backend sponsor so its nonce is independent of
# the user wallet (account[0], used as FUNDED_WALLET in the e2e tests). When
# both share an account, the user's migrate tx burns nonces the backend has
# already cached for the sponsored claim, and submission fails on retry.
SPONSOR_PRIVATE_KEY="${SPONSOR_PRIVATE_KEY:-0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d}"
SPONSOR_ADDRESS="${SPONSOR_ADDRESS:-0x70997970C51812dc3A010C7d01b50e0d17dc79C8}"

DEFAULT_CLAIMANT="0x6A541B199F0505413970ff014FE5e50AdC93f457"
CLAIMANT="${CLAIMANT:-${DEFAULT_CLAIMANT}}"
# Manual/browser seeded flows should start from a blank migration slate so the
# UI only flips tokens after the user actually migrates them.
FIXTURE_MODE="${FIXTURE_MODE:-all-unmigrated}"
# Select the snapshot pipeline; `dev` runs the single-claimant seeder, `csv`
# imports pre-baked fixtures from REAL_DATA_DIR.
SNAPSHOT_SOURCE="${SNAPSHOT_SOURCE:-dev}"
REAL_DATA_DIR="${REAL_DATA_DIR:-}"

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
  append_cors_origin_variants_for_url "${CLAIM_FRONTEND_READY_URL}"
  append_cors_origin_variants_for_url "${MIGRATE_FRONTEND_URL}"
  append_cors_origin_variants_for_url "${MIGRATE_FRONTEND_READY_URL}"
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
    echo "missing copied contracts env file: ${CONTRACTS_ENV_HOST_FILE}" >&2
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
  if [[ -t 0 || -t 1 || -t 2 ]]; then
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
    PUBLIC_CHAIN_RPC_URL="${BROWSER_CHAIN_RPC_URL}" \
    PUBLIC_WALLET_CHAIN_RPC_URL="${WALLET_CHAIN_RPC_URL}" \
    PUBLIC_CHAIN_ID="${DEV_CHAIN_ID}" \
    PUBLIC_MIN_TX_CONFIRMATIONS="${PUBLIC_MIN_TX_CONFIRMATIONS}" \
    PUBLIC_MIGRATION_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
    PUBLIC_CLAIM_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
    PUBLIC_WALLETCONNECT_PROJECT_ID="${PUBLIC_WALLETCONNECT_PROJECT_ID}" \
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

parse_uint_output() {
  local value="$1"
  value="${value%% *}"
  if [[ -z "${value}" ]]; then
    echo "expected unsigned integer output, got empty value" >&2
    exit 1
  fi
  if [[ ! "${value}" =~ ^[0-9]+$ ]]; then
    echo "expected unsigned integer output, got: ${1}" >&2
    exit 1
  fi
  printf '%s\n' "${value}"
}

run_contracts_bootstrap() {
  echo "[compose-seeded] running single-chain bootstrap source=${SNAPSHOT_SOURCE}" >&2
  # The compose wrapper sets the claimant balance directly after bootstrap.
  # Keep bootstrap funding scoped to contracts so fork gas quirks cannot drain it.
  SNAPSHOT_SOURCE="${SNAPSHOT_SOURCE}" \
  DEV_SNAPSHOT_DIR="${DEV_SNAPSHOT_DIR}" \
  REAL_DATA_DIR="${REAL_DATA_DIR}" \
  ANVIL_RPC_URL="${CHAIN_RPC_URL}" \
  DEV_CHAIN_ID="${DEV_CHAIN_ID}" \
  BACKEND_DATABASE_URL="${BACKEND_DATABASE_URL}" \
  DEPLOY_ENV_FILE="${CONTRACTS_ENV_HOST_FILE}" \
  RESET_DATABASE="0" \
  RESET_RUNTIME_STATE="1" \
  CLAIMANT="${CLAIMANT}" \
  CLAIMANT_GAS_WEI="0" \
  FIXTURE_MODE="${FIXTURE_MODE}" \
  SPONSOR_PRIVATE_KEY="${SPONSOR_PRIVATE_KEY}" \
  SPONSOR_ADDRESS="${SPONSOR_ADDRESS}" \
  bash "${ROOT_DIR}/scripts/dev/bootstrap-dev-execution-env.sh"
  load_contracts_env_file
}

fund_claimant() {
  if [[ -z "${CLAIMANT}" || "${CLAIMANT_GAS_WEI}" == "0" ]]; then
    return 0
  fi

  local claimant_balance target_hex
  claimant_balance="$(parse_uint_output "$(cast balance --rpc-url "${CHAIN_RPC_URL}" "${CLAIMANT}")")"
  if python3 -c 'import sys; sys.exit(0 if int(sys.argv[1]) >= int(sys.argv[2]) else 1)' \
    "${claimant_balance}" \
    "${CLAIMANT_GAS_WEI}"; then
    return 0
  fi

  echo "[compose-seeded] setting claimant balance claimant=${CLAIMANT} current_balance_wei=${claimant_balance} target_balance_wei=${CLAIMANT_GAS_WEI}" >&2
  target_hex="$(cast to-hex "${CLAIMANT_GAS_WEI}")"
  cast rpc \
    --rpc-url "${CHAIN_RPC_URL}" \
    anvil_setBalance \
    "${CLAIMANT}" \
    "${target_hex}" >/dev/null
}

ensure_compose_core() {
  export CONTRACTS_ENV_HOST_FILE

  local current_chain_id
  current_chain_id="$(cast chain-id -r "${CHAIN_RPC_URL}" 2>/dev/null || true)"

  if [[ "${current_chain_id}" == "${DEV_CHAIN_ID}" ]] \
    && nc -z "${POSTGRES_HOST}" "${POSTGRES_PORT}" >/dev/null 2>&1; then
    return 0
  fi

  compose up -d --build --force-recreate --remove-orphans postgres anvil
  wait_for_tcp "${POSTGRES_HOST}" "${POSTGRES_PORT}" "postgres"
  wait_for_tcp "127.0.0.1" "5010" "anvil"

  for _ in $(seq 1 60); do
    current_chain_id="$(cast chain-id -r "${CHAIN_RPC_URL}" 2>/dev/null || true)"
    if [[ "${current_chain_id}" == "${DEV_CHAIN_ID}" ]]; then
      return 0
    fi
    sleep 1
  done

  echo "anvil did not become ready with expected chain id ${DEV_CHAIN_ID}" >&2
  exit 1
}

ensure_database() {
  wait_for_postgres_ready

  echo "[compose-seeded] stopping backend before database reset" >&2
  compose stop zksync-lite-sunset-server >/dev/null 2>&1 || true

  DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx database reset -y \
    --source "${ROOT_DIR}/apps/backend/crates/zksync-lite-sunset-core/migrations"
}

ensure_compose_env_file
ensure_compose_core
ensure_database

run_contracts_bootstrap
echo "[compose-seeded] funding claimant" >&2
fund_claimant

echo "[compose-seeded] starting backend" >&2
export ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL="${BACKEND_CHAIN_RPC_URL}"
if [[ "${COMPOSE_BUILD_SERVER}" == "true" ]]; then
  compose up -d --build --no-deps --force-recreate --remove-orphans zksync-lite-sunset-server
else
  compose up -d --no-deps --force-recreate --remove-orphans zksync-lite-sunset-server
fi
echo "[compose-seeded] waiting for backend" >&2
COMPOSE_FILE="${COMPOSE_FILE}" \
INFRA_ENV_FILE="${INFRA_ENV_FILE}" \
COMPOSE_ENV_FILE="${COMPOSE_ENV_FILE}" \
PUBLIC_HEALTH_URL="${BACKEND_BASE_URL}/healthz" \
PUBLIC_READY_URL="${BACKEND_BASE_URL}/readyz" \
bash "${ROOT_DIR}/scripts/dev/check-compose-backend-health.sh"

echo "[compose-seeded] building claim frontend" >&2
if [[ -n "${PUBLIC_WALLETCONNECT_PROJECT_ID}" ]]; then
  if [[ -n "${WALLETCONNECT_DEV_HOST}" ]]; then
    echo "[compose-seeded] WalletConnect local dev enabled frontend_origin=${CLAIM_FRONTEND_URL} browser_rpc_url=${BROWSER_CHAIN_RPC_URL} wallet_rpc_url=${WALLET_CHAIN_RPC_URL}" >&2
  else
    echo "[compose-seeded] PUBLIC_WALLETCONNECT_PROJECT_ID is set, but no LAN host was detected; mobile wallets may not reach loopback RPC URLs. Set WALLETCONNECT_DEV_HOST if needed." >&2
  fi
fi
PUBLIC_API_BASE_URL="${BACKEND_BASE_URL}" \
PUBLIC_CLAIM_APP_URL="${CLAIM_FRONTEND_URL}" \
PUBLIC_MIGRATE_APP_URL="${MIGRATE_FRONTEND_URL}" \
PUBLIC_CHAIN_RPC_URL="${BROWSER_CHAIN_RPC_URL}" \
PUBLIC_WALLET_CHAIN_RPC_URL="${WALLET_CHAIN_RPC_URL}" \
PUBLIC_CHAIN_ID="${DEV_CHAIN_ID}" \
PUBLIC_MIN_TX_CONFIRMATIONS="${PUBLIC_MIN_TX_CONFIRMATIONS}" \
PUBLIC_MIGRATION_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
PUBLIC_CLAIM_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
PUBLIC_WALLETCONNECT_PROJECT_ID="${PUBLIC_WALLETCONNECT_PROJECT_ID}" \
just frontend build-claim

echo "[compose-seeded] building migrate frontend" >&2
PUBLIC_API_BASE_URL="${BACKEND_BASE_URL}" \
PUBLIC_CLAIM_APP_URL="${CLAIM_FRONTEND_URL}" \
PUBLIC_MIGRATE_APP_URL="${MIGRATE_FRONTEND_URL}" \
PUBLIC_CHAIN_RPC_URL="${BROWSER_CHAIN_RPC_URL}" \
PUBLIC_WALLET_CHAIN_RPC_URL="${WALLET_CHAIN_RPC_URL}" \
PUBLIC_CHAIN_ID="${DEV_CHAIN_ID}" \
PUBLIC_MIN_TX_CONFIRMATIONS="${PUBLIC_MIN_TX_CONFIRMATIONS}" \
PUBLIC_MIGRATION_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
PUBLIC_CLAIM_CONTRACT_ADDRESS="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
PUBLIC_WALLETCONNECT_PROJECT_ID="" \
just frontend build-migrate

echo "[compose-seeded] starting claim frontend preview" >&2
start_frontend_preview claim
CLAIM_FRONTEND_PID=$!

echo "[compose-seeded] starting migrate frontend preview" >&2
start_frontend_preview migrate
MIGRATE_FRONTEND_PID=$!

echo "[compose-seeded] waiting for frontends" >&2
wait_for_http "${CLAIM_FRONTEND_READY_URL}/" "claim frontend"
wait_for_http "${MIGRATE_FRONTEND_READY_URL}/" "migrate frontend"

echo "[compose-seeded] claim frontend: ${CLAIM_FRONTEND_URL}/" >&2
echo "[compose-seeded] migrate frontend: ${MIGRATE_FRONTEND_URL}/" >&2
echo "[compose-seeded] browser RPC URL: ${BROWSER_CHAIN_RPC_URL}" >&2
echo "[compose-seeded] wallet RPC URL: ${WALLET_CHAIN_RPC_URL}" >&2
echo "[compose-seeded] backend: ${BACKEND_BASE_URL}/" >&2
echo "[compose-seeded] ready"
wait
