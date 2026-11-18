export type Address = `0x${string}`;

// Wire shape returned by `/api/eligibility`. The backend never reads chain
// state, so `migrated` / `claimed` are derived client-side from on-chain
// probes (see `enrichEligibilityTokens`).
export type ApiEligibleToken = {
  claimIndex: number;
  tokenAddress: Address;
  symbol: string;
  displayBalance: string;
  rawBalance: string;
  decimals: number;
};

export type EligibilityResponse = {
  address: Address;
  tokens: ApiEligibleToken[];
};

export type ClaimFrontendMode = "unavailable" | "sponsored" | "paid";

export type FrontendConfigResponse = {
  claimMode: ClaimFrontendMode;
};

// UI shape with on-chain status overlaid by `enrichEligibilityTokens`.
export type EligibleToken = ApiEligibleToken & {
  migrated: boolean;
  claimed: boolean;
};

export type ActionKind = "claim" | "migrate";

export type TypedDataDomain = {
  name: string;
  version: string;
  chainId: number;
  verifyingContract: Address;
};

export type ClaimTypedDataMessage = {
  claimant: Address;
  claimsHash: `0x${string}`;
  expiry: number;
  nonce: number;
};

export type ClaimTypedData = {
  primaryType: "ClaimBatch";
  domain: TypedDataDomain;
  message: ClaimTypedDataMessage;
};

export type ClaimBatchItem = {
  index: number;
  tokenAddress: Address;
  amount: string;
  merkleProof: `0x${string}`[];
};

export type ClaimPrepareRequest = {
  address: Address;
  tokenAddress?: Address;
  tokenAddresses?: Address[];
  allEligible?: boolean;
};

export type DirectClaimTransaction = {
  chainId: number;
  contractAddress: Address;
  callData: `0x${string}`;
};

export type ClaimPrepareResponse = {
  address: Address;
  chainId: number;
  verifyingContract: Address;
  claimsHash: `0x${string}`;
  nonce: number;
  expiry: number;
  items: ClaimBatchItem[];
  typedData: ClaimTypedData;
  claimId: string;
  directTx: DirectClaimTransaction;
};

export type ClaimSubmitDirectRequest = {
  claimId: string;
  txHash: `0x${string}`;
};

export type MigrationStatus =
  | "submitted"
  | "mining"
  | "mined_success"
  | "mined_reverted";

export type MigrationFailureStage = "l1_execution";

export type MigrationDeliveryPath = "direct_l1_claim_contract";

export type MigrationReceipt = {
  blockNumber: number;
  blockHash: `0x${string}`;
};

export type MigrationDeliveryDetails = {
  tokenIndex: number;
  tokenAddress: Address;
  total: string;
  claimRoot: `0x${string}`;
  path: MigrationDeliveryPath;
  claimDistributorAddress: Address;
};

export type MigrationResponse = {
  migrationId: string;
  status: MigrationStatus;
  txHash?: `0x${string}`;
  receipt?: MigrationReceipt;
  errorCode?: string;
  error?: string;
  failureStage?: MigrationFailureStage;
  delivery?: MigrationDeliveryDetails;
};

export type MigrationItem = {
  index: number;
  tokenAddress: Address;
  total: string;
  merkleProof: `0x${string}`[];
};

export type MigrationPrepareRequest = {
  address: Address;
  tokenAddress: Address;
};

export type MigrationPrepareResponse = {
  migrationId: string;
  address: Address;
  chainId: number;
  contractAddress: Address;
  item: MigrationItem;
  claimRoot: `0x${string}`;
  tokenMigrationRoot: `0x${string}`;
  callData: `0x${string}`;
};

export type MigrationSubmitRequest = {
  migrationId: string;
  txHash: `0x${string}`;
};

export type ClaimSubmitRequest = {
  address: Address;
  items: ClaimBatchItem[];
  claimsHash: `0x${string}`;
  nonce: number;
  expiry: number;
  signature: `0x${string}`;
};

export type ClaimStatus =
  | "pending"
  | "submitted"
  | "mining"
  | "mined_success"
  | "mined_reverted";

export type ClaimReceipt = {
  blockNumber: number;
  blockHash: `0x${string}`;
};

export type ClaimResponse = {
  claimId: string;
  status: ClaimStatus;
  txHash?: `0x${string}`;
  receipt?: ClaimReceipt;
  errorCode?: string;
  error?: string;
};

export type ApiErrorCode = string;

export class ApiError extends Error {
  status: number;
  code?: ApiErrorCode;

  constructor(opts: { status: number; message: string; code?: ApiErrorCode }) {
    super(opts.message);
    this.name = "ApiError";
    this.status = opts.status;
    this.code = opts.code;
  }
}

export function apiBaseUrl(): string {
  const configuredBaseUrl = import.meta.env.PUBLIC_API_BASE_URL;
  if (configuredBaseUrl) {
    return configuredBaseUrl;
  }

  if (typeof window !== "undefined") {
    const { hostname, port, protocol } = window.location;
    if (
      (port === "4321" || port === "4322") &&
      (hostname === "127.0.0.1" || hostname === "localhost")
    ) {
      return `${protocol}//${hostname}:3300`;
    }
  }

  return "";
}

function parseStructuredError(text: string): {
  code?: string;
  message?: string;
} {
  try {
    const v = JSON.parse(text) as unknown;
    if (typeof v !== "object" || v === null) return {};
    const code =
      "code" in v && typeof (v as { code?: unknown }).code === "string"
        ? (v as { code: string }).code
        : undefined;
    const message =
      "message" in v && typeof (v as { message?: unknown }).message === "string"
        ? (v as { message: string }).message
        : "error" in v && typeof (v as { error?: unknown }).error === "string"
          ? (v as { error: string }).error
          : undefined;
    return { code, message };
  } catch {
    return {};
  }
}

async function postJson<T>(
  path: string,
  body: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const res = await fetch(`${apiBaseUrl()}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal,
  });

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    const parsed = parseStructuredError(text);
    const code = parsed.code;
    const message =
      parsed.message ??
      (text ? text : `Request failed with status ${res.status}`);
    throw new ApiError({ status: res.status, code, message });
  }

  return (await res.json()) as T;
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`${apiBaseUrl()}${path}`, {
    method: "GET",
    signal,
  });

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    const parsed = parseStructuredError(text);
    const code = parsed.code;
    const message =
      parsed.message ??
      (text ? text : `Request failed with status ${res.status}`);
    throw new ApiError({ status: res.status, code, message });
  }

  return (await res.json()) as T;
}

export async function checkEligibility(
  address: Address,
  signal?: AbortSignal,
): Promise<EligibilityResponse> {
  return postJson<EligibilityResponse>("/api/eligibility", { address }, signal);
}

export async function getFrontendConfig(
  signal?: AbortSignal,
): Promise<FrontendConfigResponse> {
  return getJson<FrontendConfigResponse>("/api/config", signal);
}

export async function submitClaim(
  req: ClaimSubmitRequest,
  signal?: AbortSignal,
): Promise<ClaimResponse> {
  return postJson<ClaimResponse>("/api/claims", req, signal);
}

export async function submitDirectClaim(
  req: ClaimSubmitDirectRequest,
  signal?: AbortSignal,
): Promise<ClaimResponse> {
  return postJson<ClaimResponse>("/api/claims/direct", req, signal);
}

export async function prepareClaim(
  req: ClaimPrepareRequest,
  signal?: AbortSignal,
): Promise<ClaimPrepareResponse> {
  return postJson<ClaimPrepareResponse>("/api/claims/prepare", req, signal);
}

export async function getClaimStatus(
  claimId: string,
  signal?: AbortSignal,
): Promise<ClaimResponse> {
  const encoded = encodeURIComponent(claimId);
  return getJson<ClaimResponse>(`/api/claims/${encoded}`, signal);
}

export async function prepareMigration(
  req: MigrationPrepareRequest,
  signal?: AbortSignal,
): Promise<MigrationPrepareResponse> {
  return postJson<MigrationPrepareResponse>(
    "/api/migrations/prepare",
    req,
    signal,
  );
}

export async function submitMigration(
  req: MigrationSubmitRequest,
  signal?: AbortSignal,
): Promise<MigrationResponse> {
  return postJson<MigrationResponse>("/api/migrations", req, signal);
}

export async function getMigrationStatus(
  migrationId: string,
  signal?: AbortSignal,
): Promise<MigrationResponse> {
  const encoded = encodeURIComponent(migrationId);
  return getJson<MigrationResponse>(`/api/migrations/${encoded}`, signal);
}
