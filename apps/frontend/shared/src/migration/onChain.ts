import { multicall } from "wagmi/actions";

import { getWalletConfig } from "../wallet/config";
import { configuredWalletChainId } from "../wallet/chains";

export type Address = `0x${string}`;

export const ETH_SENTINEL: Address =
  "0x0000000000000000000000000000000000000000";

export const STETH: Address = "0xae7ab96520de3a18e5e111b5eaab095312d7fe84";

export const MIGRATION_STATE_ABI = [
  {
    type: "function",
    name: "isTokenMigrated",
    stateMutability: "view",
    inputs: [{ name: "token", type: "address" }],
    outputs: [{ name: "", type: "bool" }],
  },
] as const;

export const CLAIM_DISTRIBUTOR_ABI = [
  {
    type: "function",
    name: "isClaimed",
    stateMutability: "view",
    inputs: [{ name: "index", type: "uint256" }],
    outputs: [{ name: "", type: "bool" }],
  },
] as const;

function resolveAddressEnv(name: string, value: string | undefined): Address {
  const raw = value?.trim();
  if (!raw) {
    throw new Error(
      `${name} is not configured; frontend cannot read on-chain state.`,
    );
  }
  if (!/^0x[0-9a-fA-F]{40}$/.test(raw)) {
    throw new Error(`${name} is not a valid 20-byte hex address: ${raw}`);
  }
  return raw.toLowerCase() as Address;
}

export function resolveMigrationContract(): Address {
  return resolveAddressEnv(
    "PUBLIC_MIGRATION_CONTRACT_ADDRESS",
    import.meta.env.PUBLIC_MIGRATION_CONTRACT_ADDRESS,
  );
}

export function resolveClaimContract(): Address {
  return resolveAddressEnv(
    "PUBLIC_CLAIM_CONTRACT_ADDRESS",
    import.meta.env.PUBLIC_CLAIM_CONTRACT_ADDRESS,
  );
}

export function resolveMigrationChainId(): number {
  const chainId = configuredWalletChainId();
  if (chainId === null) {
    throw new Error(
      "No wallet chain configured. Set PUBLIC_CHAIN_ID / PUBLIC_CHAIN_RPC_URL to read on-chain state.",
    );
  }
  return chainId;
}

export async function fetchMigratedFlags(
  tokenAddresses: readonly Address[],
): Promise<Map<string, boolean>> {
  const result = new Map<string, boolean>();
  if (tokenAddresses.length === 0) return result;

  const contract = resolveMigrationContract();
  const chainId = resolveMigrationChainId();
  const config = getWalletConfig();

  const calls = tokenAddresses.map((token) => ({
    address: contract,
    abi: MIGRATION_STATE_ABI,
    functionName: "isTokenMigrated" as const,
    args: [token] as const,
  }));

  const responses = await multicall(config, {
    chainId,
    allowFailure: true,
    contracts: calls,
  });

  tokenAddresses.forEach((token, idx) => {
    const entry = responses[idx];
    if (entry?.status === "success") {
      if (token === STETH) {
        // stETH migration was just deposited, but not migrated
        result.set(token.toLowerCase(), true);
      } else {
        result.set(token.toLowerCase(), Boolean(entry.result));
      }
    }
  });

  return result;
}

export async function fetchClaimedFlags(
  indices: readonly bigint[],
): Promise<Map<string, boolean>> {
  const result = new Map<string, boolean>();
  if (indices.length === 0) return result;

  const contract = resolveClaimContract();
  const chainId = resolveMigrationChainId();
  const config = getWalletConfig();

  const calls = indices.map((index) => ({
    address: contract,
    abi: CLAIM_DISTRIBUTOR_ABI,
    functionName: "isClaimed" as const,
    args: [index] as const,
  }));

  const responses = await multicall(config, {
    chainId,
    allowFailure: true,
    contracts: calls,
  });

  indices.forEach((index, idx) => {
    const entry = responses[idx];
    if (entry?.status === "success") {
      result.set(index.toString(), Boolean(entry.result));
    }
  });

  return result;
}
