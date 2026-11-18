#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE_FILE="${COMPOSE_FILE:-${ROOT_DIR}/infra/compose.yml}"
INFRA_ENV_FILE="${INFRA_ENV_FILE:-${ROOT_DIR}/infra/.env.local}"
COMPOSE_ENV_FILE="${COMPOSE_ENV_FILE:-}"
PUBLIC_HEALTH_URL="${PUBLIC_HEALTH_URL:-http://127.0.0.1:3000/healthz}"
PUBLIC_READY_URL="${PUBLIC_READY_URL:-http://127.0.0.1:3000/readyz}"
BACKEND_SERVICE="${BACKEND_SERVICE:-zksync-lite-sunset-server}"
MAX_ATTEMPTS="${MAX_ATTEMPTS:-30}"

if [[ -z "${COMPOSE_ENV_FILE}" && -f "${INFRA_ENV_FILE}" ]]; then
  COMPOSE_ENV_FILE="${INFRA_ENV_FILE}"
fi

compose() {
  if [[ -n "${COMPOSE_ENV_FILE}" ]]; then
    docker compose --env-file "${COMPOSE_ENV_FILE}" -f "${COMPOSE_FILE}" "$@"
  else
    docker compose -f "${COMPOSE_FILE}" "$@"
  fi
}

backend_container_id() {
  compose ps -q "${BACKEND_SERVICE}"
}

backend_health_status() {
  local container_id="$1"
  docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "${container_id}" 2>/dev/null || true
}

for _ in $(seq 1 "${MAX_ATTEMPTS}"); do
  public_live_ok=false
  public_ready_ok=false
  internal_ok=false

  if curl --fail --show-error --silent "${PUBLIC_HEALTH_URL}" >/dev/null; then
    public_live_ok=true
  fi

  if curl --fail --show-error --silent "${PUBLIC_READY_URL}" >/dev/null; then
    public_ready_ok=true
    internal_ok=true
  fi

  container_id="$(backend_container_id)"
  if [[ "${internal_ok}" != "true" && -n "${container_id}" ]]; then
    health_status="$(backend_health_status "${container_id}")"
    if [[ "${health_status}" == "healthy" ]]; then
      internal_ok=true
    fi
  fi

  if [[ "${public_live_ok}" == "true" && "${internal_ok}" == "true" ]]; then
    exit 0
  fi

  sleep 1
done

echo "backend healthcheck failed after retries" >&2
compose ps || true
compose logs --tail=200 || true
exit 1
