import type { ActionKind, Address, EligibleToken } from "../../lib/api";

export function sameAddress(a: Address, b: Address): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

export function isClaimable(token: EligibleToken): boolean {
  return token.migrated && !token.claimed;
}

export function isMigratable(token: EligibleToken): boolean {
  return !token.migrated;
}

export function isClaimAllEligible(token: EligibleToken): boolean {
  return isClaimable(token);
}

export function actionLabel(action: ActionKind): string {
  return action === "claim" ? "Withdraw" : "Migration";
}

function extractErrorMessage(err: unknown, depth = 0): string | null {
  if (depth > 3) return null;
  if (typeof err === "string" && err.trim().length > 0) return err;
  if (err instanceof Error && err.message.trim().length > 0) {
    return err.message;
  }
  if (typeof err !== "object" || err === null) return null;

  const record = err as Record<string, unknown>;

  for (const key of ["shortMessage", "message", "details", "reason"]) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value;
    }
  }

  for (const key of ["error", "cause", "data"]) {
    const nested = extractErrorMessage(record[key], depth + 1);
    if (nested) return nested;
  }

  const errors = record.errors;
  if (Array.isArray(errors)) {
    for (const candidate of errors) {
      const nested = extractErrorMessage(candidate, depth + 1);
      if (nested) return nested;
    }
  }

  return null;
}

export function asErrorMessage(err: unknown): string {
  return extractErrorMessage(err) ?? String(err);
}

export async function screenAddress(
  address: Address,
  signal?: AbortSignal,
): Promise<void> {
  const screeningApiUrl = import.meta.env.PUBLIC_SCREENING_API_URL;
  if (!screeningApiUrl) return;

  const url = new URL(screeningApiUrl);
  url.searchParams.append("address", address);

  let response: { result?: boolean };
  try {
    const fetchResponse = await fetch(url.toString(), { signal });
    if (!fetchResponse.ok) {
      throw new Error("Network response was not ok.");
    }
    response = await fetchResponse.json();
  } catch {
    response = { result: true };
  }

  if (!response.result) {
    throw new Error("We were unable to process this transaction...");
  }
}

export async function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);

    const onAbort = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
      reject(new DOMException("Aborted", "AbortError"));
    };

    if (signal) {
      if (signal.aborted) {
        onAbort();
        return;
      }
      signal.addEventListener("abort", onAbort);
    }
  });
}
