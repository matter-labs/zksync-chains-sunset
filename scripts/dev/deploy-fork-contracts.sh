#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTRACTS_DIR="${ROOT_DIR}/contracts/zksync-lite-migration-claims"

RPC_URL="${ANVIL_RPC_URL:-http://127.0.0.1:8545}"
DEV_CHAIN_ID="${DEV_CHAIN_ID:-31337}"
DEPLOYER_PRIVATE_KEY="${DEPLOYER_PRIVATE_KEY:-0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80}"
DEPLOYER_ADDRESS="${DEPLOYER_ADDRESS:-0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266}"
# Sponsor key/address default to the deployer for back-compat. Override in
# environments where the user wallet shares the deployer account so backend
# sponsor txs don't collide on nonce with user-submitted txs.
SPONSOR_PRIVATE_KEY="${SPONSOR_PRIVATE_KEY:-${DEPLOYER_PRIVATE_KEY}}"
SPONSOR_ADDRESS="${SPONSOR_ADDRESS:-${DEPLOYER_ADDRESS}}"
OUT_ENV_FILE="${OUT_ENV_FILE:-/tmp/zksync-lite-sunset-dev.env}"
CLAIM_MERKLE_ROOT="${CLAIM_MERKLE_ROOT:-0x0000000000000000000000000000000000000000000000000000000000000000}"
CLAIM_WINDOW_START="${CLAIM_WINDOW_START:-0}"
CLAIM_PROXY_ADMIN_ADDRESS="${CLAIM_PROXY_ADMIN_ADDRESS:-0x000000000000000000000000000000000000BEEF}"
CLAIM_OWNER_ADDRESS="${CLAIM_OWNER_ADDRESS:-${DEPLOYER_ADDRESS}}"
MIGRATION_CLAIM_ROOT="${MIGRATION_CLAIM_ROOT:-${MIGRATION_OWNED_CLAIM_ROOT:-${MIGRATION_CREATE2_CLAIM_ROOT:-${CLAIM_MERKLE_ROOT}}}}"
MIGRATION_TOKEN_MIGRATION_ROOT="${MIGRATION_TOKEN_MIGRATION_ROOT:-0x0000000000000000000000000000000000000000000000000000000000000000}"

deploy_contract() {
  local fqcn="$1"
  local constructor_sig="${2:-}"
  shift
  if [[ -n "${constructor_sig}" ]]; then
    shift
  fi
  local source_path="${fqcn%%:*}"
  local contract_name="${fqcn##*:}"
  local artifact_path="${CONTRACTS_DIR}/out/$(basename "${source_path}")/${contract_name}.json"
  local bytecode
  bytecode="$(jq -r '.bytecode.object' "${artifact_path}")"
  if [[ "${bytecode}" != 0x* ]]; then
    bytecode="0x${bytecode}"
  fi
  local constructor_args="0x"
  if [[ -n "${constructor_sig}" ]]; then
    constructor_args="$(cast abi-encode "${constructor_sig}" "$@")"
  fi
  local creation_data="${bytecode}${constructor_args#0x}"
  send_transaction "" "${creation_data}" "0x0" | tail -n 1
}

send_transaction() {
  local to="$1"
  local data="$2"
  local value="$3"
  local tx_json
  if [[ -n "${to}" ]]; then
    tx_json="$(jq -cn \
      --arg from "${DEPLOYER_ADDRESS}" \
      --arg to "${to}" \
      --arg data "${data}" \
      --arg value "${value}" \
      '{jsonrpc:"2.0",id:1,method:"eth_sendTransaction",params:[{from:$from,to:$to,data:$data,value:$value}]}')"
  else
    tx_json="$(jq -cn \
      --arg from "${DEPLOYER_ADDRESS}" \
      --arg data "${data}" \
      --arg value "${value}" \
      '{jsonrpc:"2.0",id:1,method:"eth_sendTransaction",params:[{from:$from,data:$data,value:$value}]}')"
  fi
  local tx_hash
  tx_hash="$(curl -fsS -H 'Content-Type: application/json' --data "${tx_json}" "${RPC_URL}" | jq -r '.result')"
  if [[ -z "${tx_hash}" || "${tx_hash}" == "null" ]]; then
    echo "failed to send transaction" >&2
    exit 1
  fi
  wait_for_receipt "${tx_hash}"
}

wait_for_receipt() {
  local tx_hash="$1"
  local response
  for _ in $(seq 1 60); do
    response="$(jq -cn \
      --arg tx_hash "${tx_hash}" \
      '{jsonrpc:"2.0",id:1,method:"eth_getTransactionReceipt",params:[$tx_hash]}' \
      | curl -fsS -H 'Content-Type: application/json' --data @- "${RPC_URL}")"
    local contract_address
    contract_address="$(printf '%s' "${response}" | jq -r '.result.contractAddress // empty')"
    local status
    status="$(printf '%s' "${response}" | jq -r '.result.status // empty')"
    if [[ -n "${contract_address}" ]]; then
      echo "${contract_address}"
      return 0
    fi
    if [[ -n "${status}" ]]; then
      if [[ "${status}" != "0x1" ]]; then
        echo "transaction reverted: ${tx_hash}" >&2
        exit 1
      fi
      echo "${tx_hash}"
      return 0
    fi
    sleep 1
  done
  echo "timed out waiting for receipt: ${tx_hash}" >&2
  exit 1
}

send_contract_call() {
  local to="$1"
  local sig="$2"
  shift
  shift
  local data
  data="$(cast calldata "${sig}" "$@")"
  send_transaction "${to}" "${data}" "0x0" >/dev/null
}

cd "${CONTRACTS_DIR}" && forge build >/dev/null

echo "Deploying claim distributor implementation (L1)..." >&2
CLAIM_IMPLEMENTATION_ADDRESS="$(
  deploy_contract src/ZkSyncLiteSunsetClaimDistributorL1.sol:ZkSyncLiteSunsetClaimDistributorL1
)"
CLAIM_INIT_DATA="$(
  cast calldata \
    "initialize(bytes32,uint256,address)" \
    "${CLAIM_MERKLE_ROOT}" \
    "${CLAIM_WINDOW_START}" \
    "${CLAIM_OWNER_ADDRESS}"
)"

echo "Deploying claim distributor proxy..." >&2
CLAIM_ADDRESS="$(
  deploy_contract \
    lib/openzeppelin-contracts/contracts/proxy/transparent/TransparentUpgradeableProxy.sol:TransparentUpgradeableProxy \
    "constructor(address,address,bytes)" \
    "${CLAIM_IMPLEMENTATION_ADDRESS}" "${CLAIM_PROXY_ADMIN_ADDRESS}" "${CLAIM_INIT_DATA}"
)"

echo "Deploying migrate mock migration state..." >&2
MIGRATION_ADDRESS="$(deploy_contract src/MockZkSyncLiteMigrationState.sol:MockZkSyncLiteMigrationState)"
MIGRATION_L1_CLAIM_DISTRIBUTOR_ADDRESS="${CLAIM_ADDRESS}"

echo "Configuring migrate mock migration state..." >&2
send_contract_call "${MIGRATION_ADDRESS}" \
  "setL1ClaimDistributor(address)" \
  "${MIGRATION_L1_CLAIM_DISTRIBUTOR_ADDRESS}"
send_contract_call "${MIGRATION_ADDRESS}" \
  "setClaimRoot(bytes32)" \
  "${MIGRATION_CLAIM_ROOT}"
if [[ "${MIGRATION_TOKEN_MIGRATION_ROOT}" != "0x0000000000000000000000000000000000000000000000000000000000000000" ]]; then
  send_contract_call "${MIGRATION_ADDRESS}" \
    "setTokenMigrationRoot(bytes32)" \
    "${MIGRATION_TOKEN_MIGRATION_ROOT}"
fi

cat > "${OUT_ENV_FILE}" <<EOF
ZKSYNC_LITE_SUNSET__CORE__AUTH__MIGRATE_CHAIN_ID=${DEV_CHAIN_ID}
ZKSYNC_LITE_SUNSET__CORE__AUTH__CLAIM_CHAIN_ID=${DEV_CHAIN_ID}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__RPC_URL=${RPC_URL}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__FROM_ADDRESS=${SPONSOR_ADDRESS}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__SPONSOR_PRIVATE_KEY=${SPONSOR_PRIVATE_KEY}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CONTRACT_ADDRESS=${MIGRATION_ADDRESS}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_L1_CLAIM_DISTRIBUTOR_ADDRESS=${MIGRATION_L1_CLAIM_DISTRIBUTOR_ADDRESS}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_CLAIM_ROOT=${MIGRATION_CLAIM_ROOT}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__MIGRATION_TOKEN_MIGRATION_ROOT=${MIGRATION_TOKEN_MIGRATION_ROOT}
ZKSYNC_LITE_SUNSET__CORE__EXECUTION__CLAIM_CONTRACT_ADDRESS=${CLAIM_ADDRESS}
EOF

if [[ -n "${BACKEND_DATABASE_URL:-}" ]]; then
  cat >> "${OUT_ENV_FILE}" <<EOF
ZKSYNC_LITE_SUNSET__CORE__DATABASE__URL=${BACKEND_DATABASE_URL}
ZKSYNC_LITE_SUNSET__CORE__DATABASE__AUTO_MIGRATE=true
EOF
fi

echo "Deployed migrate contract: ${MIGRATION_ADDRESS}" >&2
echo "Deployed migrate L1 claim distributor: ${MIGRATION_L1_CLAIM_DISTRIBUTOR_ADDRESS}" >&2
echo "Deployed claim implementation: ${CLAIM_IMPLEMENTATION_ADDRESS}" >&2
echo "Deployed claim contract proxy: ${CLAIM_ADDRESS}" >&2
echo "Wrote env file: ${OUT_ENV_FILE}" >&2
