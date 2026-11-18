#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"

export CLAIMANT="${CLAIMANT:-0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266}"
export BACKEND_HOST="${BACKEND_HOST:-127.0.0.1}"
export BACKEND_PORT="${BACKEND_PORT:-3000}"
export CLAIM_FRONTEND_HOST="${CLAIM_FRONTEND_HOST:-127.0.0.1}"
export CLAIM_FRONTEND_PORT="${CLAIM_FRONTEND_PORT:-4421}"
export MIGRATE_FRONTEND_HOST="${MIGRATE_FRONTEND_HOST:-127.0.0.1}"
export MIGRATE_FRONTEND_PORT="${MIGRATE_FRONTEND_PORT:-4422}"

exec bash "${ROOT_DIR}/scripts/dev/run-compose-seeded-stack.sh"
