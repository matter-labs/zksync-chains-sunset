import { createStorage, createConfig } from "wagmi";
import { walletConnect, injected } from "wagmi/connectors";
import { http } from "viem";

import { readRpcUrlForChain, walletChains } from "./chains";

const WALLET_STORAGE_KEY = "zksync-lite-sunset.wallet";
const WALLETCONNECT_QR_MODAL = true;
const WALLETCONNECT_METADATA_FALLBACK_URL = "https://lite.zksync.io";
const WALLETCONNECT_METADATA_ICON_PATH = "/assets/header-logo.svg";
const BRAVE_WALLET_CONNECTOR_ID = "braveWallet";

type InjectedProvider = {
  isBraveWallet?: boolean;
  providers?: InjectedProvider[];
};

function injectedProviders(window: Window | undefined): InjectedProvider[] {
  const ethereum = (
    window as (Window & { ethereum?: InjectedProvider }) | undefined
  )?.ethereum;
  if (!ethereum) return [];
  if (Array.isArray(ethereum.providers) && ethereum.providers.length > 0) {
    return ethereum.providers;
  }
  return [ethereum];
}

const braveWalletTarget = {
  id: BRAVE_WALLET_CONNECTOR_ID,
  name: "Brave Wallet",
  provider(window: Window | undefined) {
    return injectedProviders(window).find((provider) => provider.isBraveWallet);
  },
};

function multiInjectedDiscoveryEnabled(): boolean {
  if (typeof navigator === "undefined") return true;
  return !navigator.userAgent.toLowerCase().includes("jsdom");
}

function defaultStorage() {
  const storage =
    typeof window === "undefined" || !window.localStorage
      ? null
      : window.localStorage;

  return {
    getItem(key: string) {
      return storage?.getItem(key) ?? null;
    },
    removeItem(key: string) {
      storage?.removeItem(key);
    },
    setItem(key: string, value: string) {
      try {
        storage?.setItem(key, value);
      } catch {
        // Ignore quota/security failures and keep the session in memory.
      }
    },
  };
}

function configuredWalletConnectProjectId(): string | null {
  const disabled =
    import.meta.env.PUBLIC_WALLETCONNECT_DISABLED?.trim().toLowerCase() ===
    "true";
  if (disabled) return null;

  const projectId = import.meta.env.PUBLIC_WALLETCONNECT_PROJECT_ID?.trim();
  if (!projectId || projectId.length === 0) return null;
  return projectId;
}

function walletConnectMetadata() {
  const appUrl =
    typeof window === "undefined"
      ? WALLETCONNECT_METADATA_FALLBACK_URL
      : window.location.origin;

  return {
    name: "zkSync Lite",
    description: "Withdraw remaining funds from zkSync Lite.",
    url: appUrl,
    icons: [new URL(WALLETCONNECT_METADATA_ICON_PATH, appUrl).href],
  };
}

function createConnectors() {
  const connectors = [
    injected({ target: "metaMask" }),
    injected({ target: braveWalletTarget }),
    injected(),
  ];
  const walletConnectProjectId = configuredWalletConnectProjectId();
  if (walletConnectProjectId !== null) {
    connectors.push(
      walletConnect({
        projectId: walletConnectProjectId,
        showQrModal: WALLETCONNECT_QR_MODAL,
        metadata: walletConnectMetadata(),
      }),
    );
  }
  return connectors;
}

function createWalletConfig() {
  const chains = walletChains();
  return createConfig({
    chains,
    connectors: createConnectors(),
    multiInjectedProviderDiscovery: multiInjectedDiscoveryEnabled(),
    storage: createStorage({
      key: WALLET_STORAGE_KEY,
      storage: defaultStorage(),
    }),
    transports: Object.fromEntries(
      chains.map((chain) => [
        chain.id,
        http(readRpcUrlForChain(chain.id) ?? chain.rpcUrls.default.http[0]),
      ]),
    ),
  });
}

let walletConfig: ReturnType<typeof createWalletConfig> | null = null;

export function getWalletConfig() {
  walletConfig ??= createWalletConfig();
  return walletConfig;
}

export function walletConnectConfigured(): boolean {
  return configuredWalletConnectProjectId() !== null;
}

export function clearWalletConfigStorage() {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.removeItem(`${WALLET_STORAGE_KEY}.recentConnectorId`);
    window.localStorage.removeItem(`${WALLET_STORAGE_KEY}.state`);
  } catch {
    // Ignore storage failures and let wagmi reconcile in memory.
  }
}
