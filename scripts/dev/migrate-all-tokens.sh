#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCAL_ENV_FILE="${LOCAL_ENV_FILE:-${ROOT_DIR}/.env.local}"
CONTRACTS_ENV_HOST_FILE="${CONTRACTS_ENV_HOST_FILE:-/tmp/zksync-lite-sunset-compose-contracts.env}"

if [[ -f "${LOCAL_ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${LOCAL_ENV_FILE}"
  set +a
fi

if [[ -f "${CONTRACTS_ENV_HOST_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${CONTRACTS_ENV_HOST_FILE}"
  set +a
fi

BACKEND_BASE_URL="${BACKEND_BASE_URL:-http://127.0.0.1:3000}"
CHAIN_RPC_URL="${CHAIN_RPC_URL:-${ANVIL_RPC_URL:-${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL:-http://127.0.0.1:5010}}}"
MIGRATION_PRIVATE_KEY="${MIGRATION_PRIVATE_KEY:-0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80}"
POLL_ATTEMPTS="${POLL_ATTEMPTS:-30}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-1}"

require_command() {
  local command="$1"
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "missing required command: ${command}" >&2
    exit 1
  fi
}

json_post() {
  local url="$1"
  local body="$2"
  local output_file="$3"

  curl \
    --silent \
    --show-error \
    --output "${output_file}" \
    --write-out "%{http_code}" \
    --header "content-type: application/json" \
    --data "${body}" \
    "${url}"
}

extract_tx_hash() {
  local cast_output="$1"
  local tx_hash

  tx_hash="$(jq -r '.transactionHash // .transaction_hash // .hash // empty' <<< "${cast_output}" 2>/dev/null || true)"
  if [[ -n "${tx_hash}" && "${tx_hash}" != "null" ]]; then
    printf '%s\n' "${tx_hash}"
    return 0
  fi

  grep -Eo '0x[0-9a-fA-F]{64}' <<< "${cast_output}" | head -n 1
}

poll_migration_status() {
  local migration_id="$1"
  local status
  local response

  for _ in $(seq 1 "${POLL_ATTEMPTS}"); do
    response="$(curl --fail --silent "${BACKEND_BASE_URL}/api/migrations/${migration_id}")"
    status="$(jq -r '.status' <<< "${response}")"
    case "${status}" in
      mined_success)
        echo "${response}"
        return 0
        ;;
      mined_reverted)
        echo "${response}" >&2
        return 1
        ;;
    esac
    sleep "${POLL_INTERVAL_SECONDS}"
  done

  echo "timed out waiting for migration ${migration_id}" >&2
  return 1
}

require_command cast
require_command curl
require_command jq

MIGRATION_ADDRESS="${MIGRATION_ADDRESS:-$(cast wallet address --private-key "${MIGRATION_PRIVATE_KEY}")}"

echo "[dev-migrate-all] backend: ${BACKEND_BASE_URL}" >&2
echo "[dev-migrate-all] rpc: ${CHAIN_RPC_URL}" >&2
echo "[dev-migrate-all] migrator: ${MIGRATION_ADDRESS}" >&2

tokens_file="$(mktemp "${TMPDIR:-/tmp}/zksync-lite-sunset-migration-tokens.XXXXXX")"
response_file="$(mktemp "${TMPDIR:-/tmp}/zksync-lite-sunset-migration-response.XXXXXX")"
trap 'rm -f "${tokens_file}" "${response_file}"' EXIT

curl --fail --silent "${BACKEND_BASE_URL}/api/migrations/tokens" \
  | jq -r '.tokens[] | [.tokenAddress, .symbol] | @tsv' > "${tokens_file}"

if [[ ! -s "${tokens_file}" ]]; then
  echo "[dev-migrate-all] no migration tokens returned" >&2
  exit 0
fi

migrated_count=0
skipped_count=0

while IFS=$'\t' read -r token_address symbol; do
  if [[ -z "${token_address}" ]]; then
    continue
  fi

  echo "[dev-migrate-all] preparing ${symbol} ${token_address}" >&2
  prepare_body="$(jq -nc \
    --arg address "${MIGRATION_ADDRESS}" \
    --arg tokenAddress "${token_address}" \
    '{address: $address, tokenAddress: $tokenAddress}')"
  status_code="$(json_post "${BACKEND_BASE_URL}/api/migrations/prepare" "${prepare_body}" "${response_file}")"

  if [[ "${status_code}" -lt 200 || "${status_code}" -ge 300 ]]; then
    if grep -qi "already" "${response_file}"; then
      echo "[dev-migrate-all] skipping already-migrated ${symbol}" >&2
      skipped_count=$((skipped_count + 1))
      continue
    fi
    echo "[dev-migrate-all] failed to prepare ${symbol}: $(cat "${response_file}")" >&2
    exit 1
  fi

  migration_id="$(jq -r '.migrationId' "${response_file}")"
  contract_address="$(jq -r '.contractAddress' "${response_file}")"
  call_data="$(jq -r '.callData' "${response_file}")"

  echo "[dev-migrate-all] sending ${symbol} migration tx" >&2
  cast_output="$(cast send \
    --json \
    --rpc-url "${CHAIN_RPC_URL}" \
    --private-key "${MIGRATION_PRIVATE_KEY}" \
    "${contract_address}" \
    "${call_data}")"
  tx_hash="$(extract_tx_hash "${cast_output}")"
  if [[ -z "${tx_hash}" ]]; then
    echo "[dev-migrate-all] failed to parse tx hash from cast output: ${cast_output}" >&2
    exit 1
  fi

  submit_body="$(jq -nc \
    --arg migrationId "${migration_id}" \
    --arg txHash "${tx_hash}" \
    '{migrationId: $migrationId, txHash: $txHash}')"
  status_code="$(json_post "${BACKEND_BASE_URL}/api/migrations" "${submit_body}" "${response_file}")"
  if [[ "${status_code}" -lt 200 || "${status_code}" -ge 300 ]]; then
    echo "[dev-migrate-all] failed to record ${symbol} migration ${tx_hash}: $(cat "${response_file}")" >&2
    exit 1
  fi

  final_status="$(poll_migration_status "${migration_id}")"
  echo "[dev-migrate-all] migrated ${symbol} tx=${tx_hash} status=$(jq -r '.status' <<< "${final_status}")" >&2
  migrated_count=$((migrated_count + 1))
done < "${tokens_file}"

echo "[dev-migrate-all] done migrated=${migrated_count} skipped=${skipped_count}" >&2
