import { defineChain, type Chain } from "viem";
import { anvil } from "wagmi/chains";

const MAINNET_CHAIN_ID = 1;
const DEFAULT_MAINNET_RPC_URL = "https://ethereum.publicnode.com";
const LOCAL_ANVIL_CHAIN_ID = 31337;
const LOCAL_SEEDED_FORK_CHAIN_ID = 31338;
const STAGING_CHAIN_ID = 11155111;
const TEMPORARY_STAGING_CHAIN_ID = 314159;
const DEFAULT_LOCAL_HOSTNAME = "127.0.0.1";

// Block depth required before a tx is considered finalized for
// claim/migration submissions. The backend enforces the same threshold via
// `execution.min_confirmations` — keep them in sync. Override at build time
// with PUBLIC_MIN_TX_CONFIRMATIONS for environments whose chain doesn't mine
// extra blocks on its own (e.g. local anvil under the seeded compose stack).
export const MIN_TX_CONFIRMATIONS = (() => {
  const raw = import.meta.env.PUBLIC_MIN_TX_CONFIRMATIONS?.trim();
  if (!raw) return 4;
  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 4;
})();

export type WalletChainEnv = {
  publicChainId?: string | null;
  publicChainRpcUrl?: string | null;
  publicWalletChainRpcUrl?: string | null;
};

export function chainName(chainId: number | null): string {
  if (chainId === null) return "Unknown network";
  if (chainId === MAINNET_CHAIN_ID) return "Ethereum Mainnet";
  if (chainId === LOCAL_ANVIL_CHAIN_ID) return "Local Anvil";
  if (chainId === LOCAL_SEEDED_FORK_CHAIN_ID) return "Local Seeded Fork";
  if (chainId === TEMPORARY_STAGING_CHAIN_ID) return "zkSync Lite Staging";
  if (chainId === STAGING_CHAIN_ID) return "Sepolia";
  return `chain ${chainId}`;
}

export function toHexChainId(chainId: number): `0x${string}` {
  return `0x${chainId.toString(16)}`;
}

export function parseHexChainId(value: unknown): number {
  if (typeof value === "number" && Number.isInteger(value) && value >= 0) {
    return value;
  }

  if (typeof value !== "string" || !value.startsWith("0x")) {
    throw new Error(`Wallet returned an invalid chain id: ${String(value)}`);
  }

  const parsed = Number.parseInt(value, 16);
  if (!Number.isFinite(parsed)) {
    throw new Error(`Wallet returned an invalid chain id: ${value}`);
  }

  return parsed;
}

function normalizeOptionalEnv(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed || trimmed.length === 0) return null;
  return trimmed;
}

function walletChainEnv(): WalletChainEnv {
  return {
    publicChainId: import.meta.env.PUBLIC_CHAIN_ID,
    publicChainRpcUrl: import.meta.env.PUBLIC_CHAIN_RPC_URL,
    publicWalletChainRpcUrl: import.meta.env.PUBLIC_WALLET_CHAIN_RPC_URL,
  };
}

function isPrivateIpv4Hostname(hostname: string): boolean {
  if (hostname.startsWith("10.")) return true;
  if (hostname.startsWith("192.168.")) return true;
  return /^172\.(1[6-9]|2\d|3[0-1])\./.test(hostname);
}

function isLocalHostname(hostname: string): boolean {
  return (
    hostname === "localhost" ||
    hostname === "127.0.0.1" ||
    hostname === "::1" ||
    hostname.endsWith(".local") ||
    isPrivateIpv4Hostname(hostname)
  );
}

function parseOptionalChainId(value: string | null | undefined): number | null {
  const normalized = normalizeOptionalEnv(value);
  if (normalized === null) return null;
  const parsed = Number.parseInt(normalized, 10);
  if (!Number.isFinite(parsed)) return null;
  return parsed;
}

function localHostname(): string {
  const hostname =
    typeof window === "undefined"
      ? DEFAULT_LOCAL_HOSTNAME
      : window.location.hostname;
  return hostname || DEFAULT_LOCAL_HOSTNAME;
}

function localFallbackRpcUrl(
  requiredChainId: number,
  hostname: string,
): string | null {
  if (requiredChainId === LOCAL_ANVIL_CHAIN_ID) {
    return `http://${hostname}:8545`;
  }
  if (requiredChainId === LOCAL_SEEDED_FORK_CHAIN_ID) {
    return `http://${hostname}:5010`;
  }
  return null;
}

export function walletRpcUrlForChain(
  requiredChainId: number,
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): string | null {
  const sharedRpcUrl = normalizeOptionalEnv(
    env.publicWalletChainRpcUrl ?? env.publicChainRpcUrl,
  );

  if (requiredChainId === LOCAL_ANVIL_CHAIN_ID) {
    return sharedRpcUrl ?? localFallbackRpcUrl(requiredChainId, hostname);
  }
  if (requiredChainId === LOCAL_SEEDED_FORK_CHAIN_ID) {
    return sharedRpcUrl ?? localFallbackRpcUrl(requiredChainId, hostname);
  }
  if (requiredChainId === STAGING_CHAIN_ID) {
    return sharedRpcUrl;
  }

  if (parseOptionalChainId(env.publicChainId) === requiredChainId) {
    return sharedRpcUrl;
  }

  return null;
}

export function readRpcUrlForChain(
  requiredChainId: number,
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): string | null {
  const sharedRpcUrl = normalizeOptionalEnv(env.publicChainRpcUrl);

  if (requiredChainId === LOCAL_ANVIL_CHAIN_ID) {
    return sharedRpcUrl ?? localFallbackRpcUrl(requiredChainId, hostname);
  }
  if (requiredChainId === LOCAL_SEEDED_FORK_CHAIN_ID) {
    return sharedRpcUrl ?? localFallbackRpcUrl(requiredChainId, hostname);
  }
  if (requiredChainId === STAGING_CHAIN_ID) {
    return sharedRpcUrl;
  }

  if (parseOptionalChainId(env.publicChainId) === requiredChainId) {
    return sharedRpcUrl;
  }

  return mainnetFallback(requiredChainId)?.rpcUrls.default.http[0] ?? null;
}

export function configuredWalletChainId(
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): number | null {
  const explicitChainId = parseOptionalChainId(env.publicChainId);
  if (explicitChainId !== null) return explicitChainId;
  if (isLocalHostname(hostname)) return LOCAL_SEEDED_FORK_CHAIN_ID;
  if (normalizeOptionalEnv(env.publicChainRpcUrl) !== null) {
    return STAGING_CHAIN_ID;
  }
  return null;
}

function etherCurrency() {
  return {
    name: "Ether",
    symbol: "ETH",
    decimals: 18,
  };
}

function chainWithRpcUrls(base: Chain, rpcUrl: string, name: string): Chain {
  return {
    ...base,
    name,
    rpcUrls: {
      default: { http: [rpcUrl] },
      public: { http: [rpcUrl] },
    },
  };
}

const MULTICALL3_ADDRESS =
  "0xcA11bde05977b3631167028862bE2a173976CA11" as const;

const MULTICALL3_CONTRACT = {
  multicall3: {
    address: MULTICALL3_ADDRESS,
    blockCreated: 0,
  },
} as const;

const MAINNET_FALLBACK_CHAIN = defineChain({
  id: MAINNET_CHAIN_ID,
  name: chainName(MAINNET_CHAIN_ID),
  nativeCurrency: etherCurrency(),
  rpcUrls: {
    default: { http: [DEFAULT_MAINNET_RPC_URL] },
    public: { http: [DEFAULT_MAINNET_RPC_URL] },
  },
  blockExplorers: {
    default: {
      name: "Etherscan",
      url: "https://etherscan.io",
    },
  },
  contracts: MULTICALL3_CONTRACT,
});

function walletLocalAnvilChain(
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): Chain {
  const rpcUrl =
    walletRpcUrlForChain(LOCAL_ANVIL_CHAIN_ID, env, hostname) ??
    `http://${hostname}:8545`;
  const base = chainWithRpcUrls(anvil, rpcUrl, chainName(LOCAL_ANVIL_CHAIN_ID));
  return { ...base, contracts: { ...base.contracts, ...MULTICALL3_CONTRACT } };
}

function walletLocalSeededForkChain(
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): Chain {
  const rpcUrl =
    walletRpcUrlForChain(LOCAL_SEEDED_FORK_CHAIN_ID, env, hostname) ??
    `http://${hostname}:5010`;
  return defineChain({
    id: LOCAL_SEEDED_FORK_CHAIN_ID,
    name: chainName(LOCAL_SEEDED_FORK_CHAIN_ID),
    nativeCurrency: etherCurrency(),
    rpcUrls: {
      default: { http: [rpcUrl] },
      public: { http: [rpcUrl] },
    },
    blockExplorers: {
      default: {
        name: "Local Seeded Fork RPC",
        url: rpcUrl,
      },
    },
    contracts: MULTICALL3_CONTRACT,
    testnet: true,
  });
}

function walletStagingChain(
  env: WalletChainEnv = walletChainEnv(),
): Chain | null {
  const rpcUrl = walletRpcUrlForChain(STAGING_CHAIN_ID, env);
  if (rpcUrl === null) return null;

  return defineChain({
    id: STAGING_CHAIN_ID,
    name: chainName(STAGING_CHAIN_ID),
    nativeCurrency: etherCurrency(),
    rpcUrls: {
      default: { http: [rpcUrl] },
      public: { http: [rpcUrl] },
    },
    blockExplorers: {
      default: {
        name: "Sepolia Etherscan",
        url: "https://sepolia.etherscan.io",
      },
    },
    contracts: MULTICALL3_CONTRACT,
  });
}

function walletConfiguredChain(
  requiredChainId: number,
  env: WalletChainEnv = walletChainEnv(),
): Chain | null {
  if (parseOptionalChainId(env.publicChainId) !== requiredChainId) return null;
  const rpcUrl = walletRpcUrlForChain(requiredChainId, env);
  if (rpcUrl === null) return null;

  return defineChain({
    id: requiredChainId,
    name: chainName(requiredChainId),
    nativeCurrency: etherCurrency(),
    rpcUrls: {
      default: { http: [rpcUrl] },
      public: { http: [rpcUrl] },
    },
    blockExplorers: {
      default: {
        name: `${chainName(requiredChainId)} RPC`,
        url: rpcUrl,
      },
    },
    contracts: MULTICALL3_CONTRACT,
    testnet: requiredChainId !== MAINNET_CHAIN_ID,
  });
}

export function walletChainForChainId(
  requiredChainId: number,
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): Chain | null {
  if (requiredChainId === LOCAL_ANVIL_CHAIN_ID) {
    return walletLocalAnvilChain(env, hostname);
  }
  if (requiredChainId === LOCAL_SEEDED_FORK_CHAIN_ID) {
    return walletLocalSeededForkChain(env, hostname);
  }
  if (requiredChainId === STAGING_CHAIN_ID) {
    return walletStagingChain(env);
  }
  return (
    walletConfiguredChain(requiredChainId, env) ??
    mainnetFallback(requiredChainId)
  );
}

function mainnetFallback(requiredChainId: number): Chain | null {
  return requiredChainId === MAINNET_CHAIN_ID ? MAINNET_FALLBACK_CHAIN : null;
}

export function localChainConfig(requiredChainId: number): {
  chainName: string;
  rpcUrls: string[];
  nativeCurrency: { name: string; symbol: string; decimals: number };
} | null {
  const chain = walletChainForChainId(requiredChainId);
  if (chain === null) return null;

  const explicitChainId = parseOptionalChainId(walletChainEnv().publicChainId);

  if (
    requiredChainId !== LOCAL_ANVIL_CHAIN_ID &&
    requiredChainId !== LOCAL_SEEDED_FORK_CHAIN_ID &&
    requiredChainId !== STAGING_CHAIN_ID &&
    requiredChainId !== explicitChainId
  ) {
    return null;
  }

  return {
    chainName: chain.name,
    rpcUrls: chain.rpcUrls.default.http,
    nativeCurrency: chain.nativeCurrency,
  };
}

export function walletChains(
  env: WalletChainEnv = walletChainEnv(),
  hostname: string = localHostname(),
): [Chain, ...Chain[]] {
  const configuredChainId = parseOptionalChainId(env.publicChainId);
  const configuredChain =
    configuredChainId === null
      ? null
      : walletConfiguredChain(configuredChainId, env);
  const chains: Chain[] = [
    configuredChain?.id === MAINNET_CHAIN_ID
      ? configuredChain
      : MAINNET_FALLBACK_CHAIN,
    walletLocalAnvilChain(env, hostname),
    walletLocalSeededForkChain(env, hostname),
  ];
  const stagingChain = walletStagingChain(env);
  if (stagingChain !== null) chains.push(stagingChain);
  if (
    configuredChain !== null &&
    !chains.some((chain) => chain.id === configuredChain.id)
  ) {
    chains.push(configuredChain);
  }
  return chains as [Chain, ...Chain[]];
}

export function walletChain(requiredChainId: number): Chain | null {
  return walletChainForChainId(requiredChainId);
}
