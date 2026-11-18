import type { ClaimResponse, MigrationResponse } from "../../lib/api";
import { getClaimStatus, getMigrationStatus } from "../../lib/api";
import { isClaimTerminal, isMigrationTerminal } from "./types";
import { sleep } from "./utils";

const POLL_INTERVAL_MS = 2_000;
const RECEIPT_TIMEOUT_MS = 180_000;
const POLL_MAX_ATTEMPTS = RECEIPT_TIMEOUT_MS / POLL_INTERVAL_MS;

export function claimStatusLabel(status: ClaimResponse["status"]): string {
  if (status === "pending") return "Queued for broadcast";
  if (status === "submitted") return "Transaction submitted";
  if (status === "mining") return "In progress";
  if (status === "mined_success") return "Transaction mined";
  return "Transaction reverted";
}

export function migrationStatusLabel(
  status: MigrationResponse["status"],
): string {
  if (status === "submitted") return "Transaction submitted";
  if (status === "mining") return "In progress";
  if (status === "mined_success") return "Transaction mined";
  return "Transaction reverted";
}

export async function pollClaimUntilTerminal(
  initial: ClaimResponse,
  onProgress: (snapshot: ClaimResponse) => void,
  signal?: AbortSignal,
): Promise<ClaimResponse> {
  let current = initial;
  onProgress(current);

  for (let i = 0; i < POLL_MAX_ATTEMPTS; i += 1) {
    if (isClaimTerminal(current.status)) return current;
    await sleep(POLL_INTERVAL_MS, signal);
    current = await getClaimStatus(current.claimId, signal);
    onProgress(current);
  }

  throw new Error(
    "Claim is taking longer than expected. Please check again soon.",
  );
}

export async function pollMigrationUntilTerminal(
  initial: MigrationResponse,
  onProgress: (snapshot: MigrationResponse) => void,
  signal?: AbortSignal,
): Promise<MigrationResponse> {
  let current = initial;
  onProgress(current);

  for (let i = 0; i < POLL_MAX_ATTEMPTS; i += 1) {
    if (isMigrationTerminal(current.status)) return current;
    await sleep(POLL_INTERVAL_MS, signal);
    current = await getMigrationStatus(current.migrationId, signal);
    onProgress(current);
  }

  throw new Error(
    "Migration is taking longer than expected. Please check again soon.",
  );
}
