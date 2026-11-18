#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ANVIL_RPC_URL="${ANVIL_RPC_URL:-http://127.0.0.1:8545}"
DEPLOY_ENV_FILE="${DEPLOY_ENV_FILE:-/tmp/zksync-lite-sunset-dev.env}"
SNAPSHOT_SOURCE="${SNAPSHOT_SOURCE:-dev}"
DEV_SNAPSHOT_DIR="${DEV_SNAPSHOT_DIR:-/tmp/zksync-lite-sunset-dev-snapshot}"
REAL_DATA_DIR="${REAL_DATA_DIR:-${ROOT_DIR}/data}"
ARTIFACT_DIR="${ARTIFACT_DIR:-/tmp/zksync-lite-sunset-snapshot-artifact}"
DATA_CHUNK_SIZE="${DATA_CHUNK_SIZE:-5000}"
SNAPSHOT_RUST_LOG="${SNAPSHOT_RUST_LOG:-info}"
DEV_CHAIN_ID="${DEV_CHAIN_ID:-31337}"
BACKEND_DATABASE_URL="${BACKEND_DATABASE_URL:-}"
RESET_DATABASE="${RESET_DATABASE:-0}"
SNAPSHOT_LABEL="${SNAPSHOT_LABEL:-}"

DEFAULT_CLAIMANT="0x6A541B199F0505413970ff014FE5e50AdC93f457"
CLAIMANT="${CLAIMANT:-}"
CLAIMANTS="${CLAIMANTS:-}"
FIXTURE_MODE="${FIXTURE_MODE:-default}"

ZERO_ADDRESS="0x0000000000000000000000000000000000000000"
ZERO_ROOT="0x0000000000000000000000000000000000000000000000000000000000000000"
MOCK_ERC20_MINT_AMOUNT="${MOCK_ERC20_MINT_AMOUNT:-1000000000000000000000000000}"
CONTRACT_ETH_FUNDING_WEI="${CONTRACT_ETH_FUNDING_WEI:-1000000000000000000000}"
CLAIM_CONTRACT_ETH_FUNDING_WEI="${CLAIM_CONTRACT_ETH_FUNDING_WEI:-}"
MIGRATION_CONTRACT_ETH_FUNDING_WEI="${MIGRATION_CONTRACT_ETH_FUNDING_WEI:-}"
CLAIMANT_GAS_WEI="${CLAIMANT_GAS_WEI:-1000000000000000000}"
SPONSOR_MIN_POST_FUND_BALANCE_WEI="${SPONSOR_MIN_POST_FUND_BALANCE_WEI:-100000000000000000000}"

CLAIM_ROOT="${CLAIM_MERKLE_ROOT:-${ZERO_ROOT}}"
MIGRATION_CLAIM_ROOT="${MIGRATION_CLAIM_ROOT:-${MIGRATION_OWNED_CLAIM_ROOT:-${MIGRATION_CREATE2_CLAIM_ROOT:-${CLAIM_ROOT}}}}"
MIGRATION_TOKEN_MIGRATION_ROOT="${MIGRATION_TOKEN_MIGRATION_ROOT:-${ZERO_ROOT}}"
SNAPSHOT_DATA_DIR=""
RESET_RUNTIME_STATE="${RESET_RUNTIME_STATE:-1}"

normalize_snapshot_source() {
  case "${SNAPSHOT_SOURCE}" in
    dev|csv|artifact)
      ;;
    *)
      echo "unsupported SNAPSHOT_SOURCE: ${SNAPSHOT_SOURCE} (expected 'dev', 'csv', or 'artifact')" >&2
      exit 1
      ;;
  esac
}

log_step() {
  local event="$1"
  shift

  printf 'ts=%s level=INFO component=staging-bootstrap event=%s' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "${event}" >&2

  while (($#)); do
    printf ' %s' "$1" >&2
    shift
  done

  printf '\n' >&2
}

normalize_paths() {
  if [[ "${REAL_DATA_DIR}" != /* ]]; then
    REAL_DATA_DIR="${ROOT_DIR}/${REAL_DATA_DIR}"
  fi
  if [[ "${DEV_SNAPSHOT_DIR}" != /* ]]; then
    DEV_SNAPSHOT_DIR="${ROOT_DIR}/${DEV_SNAPSHOT_DIR}"
  fi
  if [[ "${ARTIFACT_DIR}" != /* ]]; then
    ARTIFACT_DIR="${ROOT_DIR}/${ARTIFACT_DIR}"
  fi
  if [[ "${DEPLOY_ENV_FILE}" != /* ]]; then
    DEPLOY_ENV_FILE="${ROOT_DIR}/${DEPLOY_ENV_FILE}"
  fi
}

resolve_snapshot_label() {
  if [[ -n "${SNAPSHOT_LABEL}" ]]; then
    printf '%s\n' "${SNAPSHOT_LABEL}"
    return
  fi

  case "${SNAPSHOT_SOURCE}" in
    dev)
      printf 'snapshot-staging-dev\n'
      ;;
    csv)
      printf 'snapshot-staging-real-csv\n'
      ;;
    artifact)
      printf 'snapshot-staging-real-artifact\n'
      ;;
  esac
}

BACKEND_MIGRATIONS_DIR="${ROOT_DIR}/apps/backend/crates/zksync-lite-sunset-core/migrations"

initialize_database() {
  if [[ -z "${BACKEND_DATABASE_URL}" ]]; then
    return
  fi

  if [[ "${RESET_DATABASE}" == "1" ]]; then
    log_step "reset_database_schema"
    DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx database reset -y \
      --source "${BACKEND_MIGRATIONS_DIR}"
    return
  fi

  log_step "run_database_migrations"
  DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx database create
  DATABASE_URL="${BACKEND_DATABASE_URL}" sqlx migrate run \
    --source "${BACKEND_MIGRATIONS_DIR}"
}

seed_dev_snapshot() {
  local seed_result_json

  log_step "seed_dev_snapshot" "snapshot_label=${SNAPSHOT_LABEL}" "claimant=${CLAIMANT:-unset}" "fixture_mode=${FIXTURE_MODE}"

  if [[ -z "${CLAIMANT}" ]]; then
    CLAIMANT="${DEFAULT_CLAIMANT}"
  fi

  seed_result_json="$(
    cd "${ROOT_DIR}/apps/backend"
    RUST_LOG="${SNAPSHOT_RUST_LOG}" cargo run --quiet -p zksync-lite-sunset-cli -- \
      dev-seed-snapshot \
      --database-url "${BACKEND_DATABASE_URL}" \
      --address "${CLAIMANT}" \
      --mode "${FIXTURE_MODE}" \
      --snapshot-label "${SNAPSHOT_LABEL}" \
      --out-dir "${DEV_SNAPSHOT_DIR}" \
      --json
  )"

  CLAIM_ROOT="$(printf '%s' "${seed_result_json}" | jq -r '.materialized.merkle_root')"
  MIGRATION_CLAIM_ROOT="${CLAIM_ROOT}"
  SNAPSHOT_DATA_DIR="${DEV_SNAPSHOT_DIR}"
}

import_real_csv_snapshot() {
  local import_result_json
  local accounts_csv
  local tokens_csv
  local balances_csv
  local cli_args

  log_step "import_real_csv_snapshot" "data_dir=${REAL_DATA_DIR}" "snapshot_label=${SNAPSHOT_LABEL}" "chunk_size=${DATA_CHUNK_SIZE}"

  accounts_csv="${REAL_DATA_DIR}/accounts.csv"
  tokens_csv="${REAL_DATA_DIR}/tokens.csv"
  balances_csv="${REAL_DATA_DIR}/balances.csv"

  if [[ ! -f "${accounts_csv}" ]]; then
    echo "real CSV import requested, but ${REAL_DATA_DIR}/accounts.csv is missing" >&2
    exit 1
  fi
  if [[ ! -f "${tokens_csv}" || ! -f "${balances_csv}" ]]; then
    echo "real CSV import requested, but one or more required CSV files are missing in ${REAL_DATA_DIR}" >&2
    exit 1
  fi

  cli_args=(
    claims-import
    --database-url "${BACKEND_DATABASE_URL}"
    --data-dir "${REAL_DATA_DIR}"
    --chunk-size "${DATA_CHUNK_SIZE}"
    --snapshot-label "${SNAPSHOT_LABEL}"
    --truncate
    --json
  )
  if [[ "${RESET_RUNTIME_STATE}" == "1" ]]; then
    cli_args+=(--reset-runtime-state)
  fi

  import_result_json="$(
    cd "${ROOT_DIR}/apps/backend"
    RUST_LOG="${SNAPSHOT_RUST_LOG}" cargo run --quiet -p zksync-lite-sunset-cli -- \
      "${cli_args[@]}"
  )"

  CLAIM_ROOT="$(printf '%s' "${import_result_json}" | jq -r '.materialized.merkle_root')"
  if [[ -z "${CLAIM_ROOT}" || "${CLAIM_ROOT}" == "null" ]]; then
    echo "claims-import did not return a merkle_root; raw output:" >&2
    printf '%s\n' "${import_result_json}" >&2
    exit 1
  fi
  MIGRATION_CLAIM_ROOT="${CLAIM_ROOT}"
  SNAPSHOT_DATA_DIR="${REAL_DATA_DIR}"
}

import_snapshot_artifact() {
  local manifest_json
  local import_result_json
  local reset_runtime_state_arg

  log_step "import_snapshot_artifact" "artifact_dir=${ARTIFACT_DIR}"

  if [[ ! -f "${ARTIFACT_DIR}/manifest.json" ]]; then
    echo "artifact import requested, but ${ARTIFACT_DIR}/manifest.json is missing" >&2
    exit 1
  fi

  manifest_json="$(cat "${ARTIFACT_DIR}/manifest.json")"
  SNAPSHOT_LABEL="$(printf '%s' "${manifest_json}" | jq -r '.snapshotLabel')"
  CLAIM_ROOT="$(printf '%s' "${manifest_json}" | jq -r '.roots.claimRoot')"
  MIGRATION_CLAIM_ROOT="${CLAIM_ROOT}"

  if [[ "${RESET_RUNTIME_STATE}" == "1" ]]; then
    reset_runtime_state_arg="true"
  else
    reset_runtime_state_arg="false"
  fi

  import_result_json="$(
    cd "${ROOT_DIR}/apps/backend"
    RUST_LOG="${SNAPSHOT_RUST_LOG}" cargo run --quiet -p zksync-lite-sunset-cli -- \
      snapshots-import-artifacts \
      --database-url "${BACKEND_DATABASE_URL}" \
      --artifact-dir "${ARTIFACT_DIR}" \
      $(if [[ "${reset_runtime_state_arg}" == "true" ]]; then printf '%s' '--reset-runtime-state'; fi) \
      --json
  )"

  SNAPSHOT_DATA_DIR="${ARTIFACT_DIR}"

  log_step "artifact_import_complete" \
    "snapshot_label=$(printf '%s' "${import_result_json}" | jq -r '.snapshotLabel')"
}

deploy_contracts() {
  log_step "deploy_contracts" "rpc_url=${ANVIL_RPC_URL}" "chain_id=${DEV_CHAIN_ID}"
  ANVIL_RPC_URL="${ANVIL_RPC_URL}" \
  DEV_CHAIN_ID="${DEV_CHAIN_ID}" \
  OUT_ENV_FILE="${DEPLOY_ENV_FILE}" \
  CLAIM_MERKLE_ROOT="${CLAIM_ROOT}" \
  MIGRATION_CLAIM_ROOT="${MIGRATION_CLAIM_ROOT}" \
  MIGRATION_TOKEN_MIGRATION_ROOT="${MIGRATION_TOKEN_MIGRATION_ROOT}" \
  BACKEND_DATABASE_URL="${BACKEND_DATABASE_URL}" \
  SPONSOR_PRIVATE_KEY="${SPONSOR_PRIVATE_KEY:-}" \
  SPONSOR_ADDRESS="${SPONSOR_ADDRESS:-}" \
  "${ROOT_DIR}/scripts/dev/deploy-fork-contracts.sh"
}

ensure_multicall3_deployed() {
  local address="0xcA11bde05977b3631167028862bE2a173976CA11"
  local source_rpc="${MULTICALL3_SOURCE_RPC_URL:-https://eth.llamarpc.com}"
  local fallback_artifact="${ROOT_DIR}/contracts/zksync-lite-migration-claims/out/Multicall3.sol/Multicall3.json"
  local existing
  existing="$(cast code "${address}" --rpc-url "${ANVIL_RPC_URL}" 2>/dev/null || printf '%s' '0x')"
  if [[ -n "${existing}" && "${existing}" != "0x" ]]; then
    if cast call --rpc-url "${ANVIL_RPC_URL}" \
      "${address}" \
      "aggregate3((address,bool,bytes)[])(tuple(bool,bytes)[])" \
      "[]" >/dev/null 2>&1; then
      log_step "multicall3_already_present" "address=${address}"
      return 0
    fi
    log_step "multicall3_missing_aggregate3" "address=${address}"
  fi

  log_step "deploy_multicall3" "address=${address}" "source_rpc=${source_rpc}"
  local runtime
  runtime="$(cast code "${address}" --rpc-url "${source_rpc}" 2>/dev/null || printf '%s' '0x')"
  if [[ -z "${runtime}" || "${runtime}" == "0x" ]]; then
    runtime="$(jq -r '.deployedBytecode.object' "${fallback_artifact}")"
    if [[ "${runtime}" != 0x* ]]; then
      runtime="0x${runtime}"
    fi
    log_step "deploy_multicall3_fallback" "artifact=${fallback_artifact}"
  fi
  cast rpc anvil_setCode "${address}" "${runtime}" --rpc-url "${ANVIL_RPC_URL}" >/dev/null
}

hydrate_mock_tokens() {
  local token_csv bytecode

  token_csv="${SNAPSHOT_DATA_DIR}/tokens.csv"
  if [[ ! -f "${token_csv}" ]]; then
    echo "snapshot token list not found at ${token_csv}" >&2
    exit 1
  fi
  log_step "hydrate_mock_tokens" "token_csv=${token_csv}"

  bytecode="$(jq -r '.deployedBytecode.object' "${ROOT_DIR}/contracts/zksync-lite-migration-claims/out/TestERC20.sol/TestERC20.json")"
  if [[ "${bytecode}" != 0x* ]]; then
    bytecode="0x${bytecode}"
  fi

  while IFS=',' read -r _id token_address _symbol _decimals; do
    token_address="$(printf '%s' "${token_address}" | tr '[:upper:]' '[:lower:]')"
    if [[ -z "${token_address}" || "${token_address}" == "${ZERO_ADDRESS}" ]]; then
      continue
    fi

    cast rpc --rpc-url "${ANVIL_RPC_URL}" anvil_setCode "${token_address}" "${bytecode}" >/dev/null
    cast send --rpc-url "${ANVIL_RPC_URL}" --private-key "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY}" \
      "${token_address}" "mint(address,uint256)" "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" "${MOCK_ERC20_MINT_AMOUNT}" >/dev/null
    cast send --rpc-url "${ANVIL_RPC_URL}" --private-key "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY}" \
      "${token_address}" "mint(address,uint256)" "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" "${MOCK_ERC20_MINT_AMOUNT}" >/dev/null
  done < <(tail -n +2 "${token_csv}")
}

resolve_contract_eth_funding() {
  local claim_funding migration_funding source

  claim_funding="${CLAIM_CONTRACT_ETH_FUNDING_WEI}"
  migration_funding="${MIGRATION_CONTRACT_ETH_FUNDING_WEI}"
  source="env"

  if [[ -z "${claim_funding}" || -z "${migration_funding}" ]]; then
    if [[ -n "${BACKEND_DATABASE_URL}" ]]; then
      local claim_total
      claim_total="$(
        cd "${ROOT_DIR}/apps/backend"
        cargo run --quiet -p zksync-lite-sunset-cli -- \
          claims-token-total \
          --database-url "${BACKEND_DATABASE_URL}" \
          --token-address "${ZERO_ADDRESS}" 2>/dev/null
      )"
      claim_total="$(printf '%s' "${claim_total}" | tr -d '[:space:]')"

      if [[ -z "${claim_funding}" ]]; then
        claim_funding="${claim_total:-0}"
      fi
      if [[ -z "${migration_funding}" ]]; then
        migration_funding="${claim_total:-0}"
      fi
      source="claim_entries_db"
    fi
  fi

  if [[ -z "${claim_funding}" ]]; then
    claim_funding="${CONTRACT_ETH_FUNDING_WEI}"
    source="fallback"
  fi
  if [[ -z "${migration_funding}" ]]; then
    migration_funding="${CONTRACT_ETH_FUNDING_WEI}"
    source="fallback"
  fi

  printf '%s\n%s\n%s\n' "${claim_funding}" "${migration_funding}" "${source}"
}

claimant_gas_addresses() {
  local raw address seen

  raw="${CLAIMANT} ${CLAIMANTS}"
  raw="${raw//,/ }"
  seen=""

  for address in ${raw}; do
    if [[ -z "${address}" ]]; then
      continue
    fi

    case $'\n'"${seen}"$'\n' in
      *$'\n'"${address}"$'\n'*)
        continue
        ;;
    esac

    seen="${seen}"$'\n'"${address}"
    printf '%s\n' "${address}"
  done
}

claimant_gas_total_wei() {
  local address count

  count=0
  while IFS= read -r address; do
    if [[ -n "${address}" ]]; then
      count=$((count + 1))
    fi
  done < <(claimant_gas_addresses)

  python3 -c 'import sys; print(int(sys.argv[1]) * int(sys.argv[2]))' \
    "${CLAIMANT_GAS_WEI}" \
    "${count}"
}

resolve_required_sponsor_balance() {
  local claim_funding="$1"
  local migration_funding="$2"
  local claimant_gas_total

  claimant_gas_total="$(claimant_gas_total_wei)"

  python3 -c 'import sys; print(int(sys.argv[1]) + int(sys.argv[2]) + int(sys.argv[3]) + int(sys.argv[4]))' \
    "${claim_funding}" \
    "${migration_funding}" \
    "${claimant_gas_total}" \
    "${SPONSOR_MIN_POST_FUND_BALANCE_WEI}"
}

ensure_sponsor_balance() {
  local required_balance="$1"
  local sponsor_address="${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__FROM_ADDRESS:-}"
  local current_balance required_balance_hex

  if [[ -z "${sponsor_address}" ]]; then
    echo "missing sponsor deployer address in deploy env" >&2
    exit 1
  fi

  current_balance="$(cast balance --rpc-url "${ANVIL_RPC_URL}" "${sponsor_address}")"
  current_balance="$(printf '%s' "${current_balance}" | tr -d '[:space:]')"

  if python3 -c 'import sys; sys.exit(0 if int(sys.argv[1]) < int(sys.argv[2]) else 1)' "${current_balance}" "${required_balance}"; then
    required_balance_hex="$(cast to-hex "${required_balance}")"
    log_step "ensure_sponsor_balance" \
      "address=${sponsor_address}" \
      "action=top_up" \
      "current_balance_wei=${current_balance}" \
      "required_balance_wei=${required_balance}" \
      "target_balance_hex=${required_balance_hex}"
    cast rpc --rpc-url "${ANVIL_RPC_URL}" anvil_setBalance "${sponsor_address}" "${required_balance_hex}" >/dev/null
  else
    log_step "ensure_sponsor_balance" \
      "address=${sponsor_address}" \
      "action=noop" \
      "current_balance_wei=${current_balance}" \
      "required_balance_wei=${required_balance}"
  fi
}

fund_claim_and_migration_contracts() {
  local resolved claim_funding migration_funding required_sponsor_balance source
  resolved="$(resolve_contract_eth_funding)"
  claim_funding="$(printf '%s\n' "${resolved}" | sed -n '1p')"
  migration_funding="$(printf '%s\n' "${resolved}" | sed -n '2p')"
  source="$(printf '%s\n' "${resolved}" | sed -n '3p')"
  required_sponsor_balance="$(resolve_required_sponsor_balance "${claim_funding}" "${migration_funding}")"

  log_step "resolve_contract_funding" \
    "source=${source}" \
    "claim_value_wei=${claim_funding}" \
    "migration_value_wei=${migration_funding}" \
    "required_sponsor_balance_wei=${required_sponsor_balance}"

  ensure_sponsor_balance "${required_sponsor_balance}"

  if [[ "${claim_funding}" != "0" ]]; then
    cast send --rpc-url "${ANVIL_RPC_URL}" --private-key "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY}" \
      "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" --value "${claim_funding}" >/dev/null
  fi

  if [[ "${migration_funding}" != "0" ]]; then
    cast send --rpc-url "${ANVIL_RPC_URL}" --private-key "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY}" \
      "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" --value "${migration_funding}" >/dev/null
  fi
}

fund_claimants() {
  local claimant

  if [[ "${CLAIMANT_GAS_WEI}" == "0" ]]; then
    return
  fi

  while IFS= read -r claimant; do
    if [[ -z "${claimant}" ]]; then
      continue
    fi

    log_step "fund_claimant" "claimant=${claimant}" "value_wei=${CLAIMANT_GAS_WEI}"
    cast send --async --rpc-url "${ANVIL_RPC_URL}" --private-key "${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY}" \
      "${claimant}" --value "${CLAIMANT_GAS_WEI}" >/dev/null
  done < <(claimant_gas_addresses)
}

normalize_snapshot_source
normalize_paths
SNAPSHOT_LABEL="$(resolve_snapshot_label)"
log_step "start_bootstrap" "snapshot_source=${SNAPSHOT_SOURCE}" "snapshot_label=${SNAPSHOT_LABEL}"
initialize_database

if [[ -n "${BACKEND_DATABASE_URL}" ]]; then
  case "${SNAPSHOT_SOURCE}" in
    dev)
      seed_dev_snapshot
      ;;
    csv)
      import_real_csv_snapshot
      ;;
    artifact)
      import_snapshot_artifact
      ;;
  esac
fi

deploy_contracts
log_step "load_deploy_env" "env_file=${DEPLOY_ENV_FILE}"
source "${DEPLOY_ENV_FILE}"
ensure_multicall3_deployed
hydrate_mock_tokens
fund_claim_and_migration_contracts
fund_claimants

log_step "bootstrap_complete" \
  "snapshot_source=${SNAPSHOT_SOURCE}" \
  "snapshot_label=${SNAPSHOT_LABEL}" \
  "snapshot_data_dir=${SNAPSHOT_DATA_DIR:-unset}" \
  "rpc_url=${ANVIL_RPC_URL}" \
  "chain_id=${DEV_CHAIN_ID}" \
  "claim_contract=${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS}" \
  "migration_contract=${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS}" \
  "sponsor_deployer=${ZKSYNC_LITE_SUNSET__CORE__EXECUTION__FROM_ADDRESS}" \
  "env_file=${DEPLOY_ENV_FILE}"
