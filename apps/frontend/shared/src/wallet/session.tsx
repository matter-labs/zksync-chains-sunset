import type { PropsWithChildren } from "react";
import { createContext, useContext, useEffect, useRef, useState } from "react";
import { getAddress, isAddress, type Hex } from "viem";
import type { Connector } from "wagmi";
import {
  useConfig,
  useConnection,
  useConnect,
  useConnectors,
  useDisconnect,
} from "wagmi";
import { getWalletClient, waitForTransactionReceipt } from "wagmi/actions";

import { shortenAddress, type Address } from "../identity/types";
import {
  chainName,
  configuredWalletChainId,
  MIN_TX_CONFIRMATIONS,
  parseHexChainId,
  toHexChainId,
  walletChain,
} from "./chains";
import { clearWalletConfigStorage, walletConnectConfigured } from "./config";

type WalletStatus =
  | "disconnected"
  | "connecting"
  | "reconnecting"
  | "connected";

type ClaimBatchTypedData = {
  domain: {
    name: string;
    version: string;
    chainId: number;
    verifyingContract: Address;
  };
  message: {
    claimant: Address;
    claimsHash: Hex;
    expiry: number;
    nonce: number;
  };
};

type PreparedWalletTransaction = {
  chainId: number;
  contractAddress: Address;
  callData: Hex;
};

type ConnectedWallet = {
  address: Address;
  chainId: number;
};

export type WalletOption = {
  id: string;
  name: string;
};

type WalletSessionContextValue = {
  status: WalletStatus;
  providerAvailable: boolean;
  availableWallets: WalletOption[];
  preferredWalletId: string | null;
  address: Address | null;
  chainId: number | null;
  switchBusy: boolean;
  errorMessage: string | null;
  connect: (
    connectorId?: string,
    requiredChainId?: number | null,
  ) => Promise<ConnectedWallet>;
  forgetConnection: () => Promise<void>;
  clearError: () => void;
  switchChain: (requiredChainId: number) => Promise<void>;
  ensureRequiredChain: (requiredChainId: number) => Promise<void>;
  signClaimBatchTypedData: (
    expectedAddress: Address,
    typedData: ClaimBatchTypedData,
    signal?: AbortSignal,
  ) => Promise<Hex>;
  sendPreparedTransaction: (
    expectedAddress: Address,
    prepared: PreparedWalletTransaction,
    signal?: AbortSignal,
  ) => Promise<Hex>;
};

const CLAIM_BATCH_TYPES = {
  ClaimBatch: [
    { name: "claimant", type: "address" },
    { name: "claimsHash", type: "bytes32" },
    { name: "expiry", type: "uint256" },
    { name: "nonce", type: "uint256" },
  ],
} as const;

const GENERIC_INJECTED_CONNECTOR_ID = "injected";
const WALLETCONNECT_CONNECTOR_ID = "walletConnect";
const PREFERRED_WALLET_STORAGE_KEY = "zksync-lite-sunset:preferred-wallet";
const REMEMBER_WALLET_STORAGE_KEY = "zksync-lite-sunset:wallet-session";

const WalletSessionContext = createContext<WalletSessionContextValue | null>(
  null,
);

type EthereumProvider = {
  request(args: { method: string; params?: unknown[] }): Promise<unknown>;
};

function sameAddress(left: Address, right: Address): boolean {
  return left.toLowerCase() === right.toLowerCase();
}

function readPreferredWalletId(): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(PREFERRED_WALLET_STORAGE_KEY);
  } catch {
    return null;
  }
}

function writePreferredWalletId(walletId: string | null) {
  if (typeof window === "undefined") return;
  try {
    if (walletId !== null) {
      window.localStorage.setItem(PREFERRED_WALLET_STORAGE_KEY, walletId);
      return;
    }
    window.localStorage.removeItem(PREFERRED_WALLET_STORAGE_KEY);
  } catch {
    // Ignore storage failures and keep the in-memory session alive.
  }
}

function readRememberedConnection(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return (
      window.localStorage.getItem(REMEMBER_WALLET_STORAGE_KEY) === "connected"
    );
  } catch {
    return false;
  }
}

function writeRememberedConnection(connected: boolean) {
  if (typeof window === "undefined") return;
  try {
    if (connected) {
      window.localStorage.setItem(REMEMBER_WALLET_STORAGE_KEY, "connected");
      return;
    }
    window.localStorage.removeItem(REMEMBER_WALLET_STORAGE_KEY);
  } catch {
    // Ignore storage failures and keep the in-memory session alive.
  }
}

function parseWalletAddress(value: unknown): Address {
  if (typeof value !== "string" || !isAddress(value, { strict: false })) {
    throw new Error("Wallet account is not a valid EVM address.");
  }

  return getAddress(value) as Address;
}

function providerErrorCode(error: unknown): number | undefined {
  if (!error || typeof error !== "object") return undefined;

  if ("code" in error && typeof error.code === "number") {
    return error.code;
  }

  if ("cause" in error) {
    return providerErrorCode(error.cause);
  }

  return undefined;
}

function providerErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }
  return fallback;
}

function connectionRejectedMessage(error: unknown): string {
  if (providerErrorCode(error) === 4001) {
    return "Wallet connection was rejected. Approve the request and retry.";
  }

  const message = providerErrorMessage(error, "Wallet connection failed.");
  if (message.toLowerCase().includes("reject")) {
    return "Wallet connection was rejected. Approve the request and retry.";
  }

  return message;
}

function noWalletMessage(): string {
  if (walletConnectConfigured()) {
    return "No supported wallet connector is ready. Install a browser wallet or retry with WalletConnect.";
  }

  return "No injected wallet found. Open this page in a wallet-enabled browser.";
}

type AvailableWalletConnector = WalletOption & {
  connector: Connector;
  provider: unknown;
};

type WalletConnectorCandidate = {
  connector: Connector;
  provider: unknown;
  ready: boolean;
};

function connectorDisplayName(connector: Connector): string {
  if (connector.id === GENERIC_INJECTED_CONNECTOR_ID) {
    return "Browser Wallet";
  }
  if (connector.id === WALLETCONNECT_CONNECTOR_ID) {
    return "WalletConnect";
  }
  return connector.name;
}

function connectorPriority(connector: Connector): number {
  if (connector.id === "metaMask") return 0;
  if (
    connector.type === "injected" &&
    connector.id !== GENERIC_INJECTED_CONNECTOR_ID
  ) {
    return 1;
  }
  if (connector.id === GENERIC_INJECTED_CONNECTOR_ID) return 2;
  if (connector.id === WALLETCONNECT_CONNECTOR_ID) return 3;
  return 4;
}

function nestedProviders(provider: unknown): unknown[] {
  if (!provider || typeof provider !== "object") return [];
  const providers = (provider as { providers?: unknown[] }).providers;
  return Array.isArray(providers) ? providers : [];
}

function containsAnyKnownProvider(
  provider: unknown,
  seenProviders: unknown[],
): boolean {
  return nestedProviders(provider).some((candidate) =>
    seenProviders.some((seenProvider) => Object.is(seenProvider, candidate)),
  );
}

function availableWalletConnectors(
  candidates: WalletConnectorCandidate[],
): AvailableWalletConnector[] {
  const seenProviders: unknown[] = [];

  return candidates
    .filter((candidate) => candidate.ready)
    .sort(
      (left, right) =>
        connectorPriority(left.connector) - connectorPriority(right.connector),
    )
    .flatMap((candidate) => {
      if (
        candidate.provider !== undefined &&
        seenProviders.some((provider) =>
          Object.is(provider, candidate.provider),
        )
      ) {
        return [];
      }

      if (
        candidate.connector.id === GENERIC_INJECTED_CONNECTOR_ID &&
        containsAnyKnownProvider(candidate.provider, seenProviders)
      ) {
        return [];
      }

      if (candidate.provider !== undefined) {
        seenProviders.push(candidate.provider);
      }

      return [
        {
          id: candidate.connector.id,
          name: connectorDisplayName(candidate.connector),
          connector: candidate.connector,
          provider: candidate.provider,
        },
      ];
    });
}

async function listAvailableWalletConnectors(
  connectors: readonly Connector[],
): Promise<AvailableWalletConnector[]> {
  const candidates = await Promise.all(
    connectors.map(async (connector) => {
      const provider = await connector.getProvider().catch(() => undefined);
      return {
        connector,
        provider,
        ready:
          connector.id === WALLETCONNECT_CONNECTOR_ID || provider !== undefined,
      } satisfies WalletConnectorCandidate;
    }),
  );

  return availableWalletConnectors(candidates);
}

function resolveDirectWallet(
  availableWallets: WalletOption[],
  preferredWalletId: string | null,
): WalletOption | null {
  if (preferredWalletId !== null) {
    const preferredWallet =
      availableWallets.find((wallet) => wallet.id === preferredWalletId) ??
      null;
    if (preferredWallet !== null) return preferredWallet;
  }

  if (availableWallets.length === 1) {
    return availableWallets[0];
  }

  return null;
}

async function assertSignalNotAborted(signal?: AbortSignal) {
  if (!signal?.aborted) return;
  throw new DOMException("The operation was aborted.", "AbortError");
}

async function connectedProvider(
  connector: Connector | undefined,
): Promise<EthereumProvider> {
  if (connector === undefined) {
    throw new Error("Connect a wallet before continuing.");
  }

  const provider = await connector.getProvider().catch(() => undefined);
  if (!provider || typeof provider.request !== "function") {
    throw new Error("Connected wallet provider is unavailable.");
  }

  return provider as EthereumProvider;
}

async function readActiveChainId(provider: EthereumProvider): Promise<number> {
  const chainId = await provider.request({
    method: "eth_chainId",
    params: [],
  });
  return parseHexChainId(chainId);
}

function providerHasPendingRequest(error: unknown): boolean {
  if (providerErrorCode(error) === -32002) return true;
  return providerErrorMessage(error, "").toLowerCase().includes("pending");
}

async function wait(ms: number): Promise<void> {
  await new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function waitForActiveChainId(
  provider: EthereumProvider,
  requiredChainId: number,
  attempts = 8,
  delayMs = 200,
): Promise<boolean> {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (attempt > 0) {
      await wait(delayMs);
    }

    const activeChainId = await readActiveChainId(provider).catch(() => null);
    if (activeChainId === requiredChainId) {
      return true;
    }
  }

  return false;
}

async function ensureProviderChain(
  provider: EthereumProvider,
  requiredChainId: number,
  addParameters: NonNullable<ReturnType<typeof addEthereumChainParameters>>,
): Promise<void> {
  let addedChain = false;
  const switchParams = [{ chainId: toHexChainId(requiredChainId) }];

  for (let attempt = 0; attempt < 12; attempt += 1) {
    const activeChainId = await readActiveChainId(provider);
    if (activeChainId === requiredChainId) {
      return;
    }

    try {
      await provider.request({
        method: "wallet_switchEthereumChain",
        params: switchParams,
      });

      if (await waitForActiveChainId(provider, requiredChainId)) {
        return;
      }

      continue;
    } catch (error) {
      const code = providerErrorCode(error);
      if (code === 4001) {
        throw new Error(
          `Wallet switch was rejected. Switch to ${chainName(requiredChainId)} and retry.`,
        );
      }

      if (code === 4902) {
        if (addedChain) {
          await wait(150);
          continue;
        }

        try {
          await provider.request({
            method: "wallet_addEthereumChain",
            params: [addParameters],
          });
        } catch (addError) {
          if (providerErrorCode(addError) === 4001) {
            throw new Error(
              `Wallet network add was rejected. Add ${chainName(requiredChainId)} and retry.`,
            );
          }
          throw addError;
        }

        addedChain = true;
        if (await waitForActiveChainId(provider, requiredChainId, 3, 150)) {
          return;
        }

        try {
          await provider.request({
            method: "wallet_switchEthereumChain",
            params: switchParams,
          });
        } catch (switchAfterAddError) {
          const switchAfterAddCode = providerErrorCode(switchAfterAddError);
          if (switchAfterAddCode === 4001) {
            throw new Error(
              `Wallet switch was rejected. Switch to ${chainName(requiredChainId)} and retry.`,
            );
          }
          if (
            switchAfterAddCode === 4902 ||
            providerHasPendingRequest(switchAfterAddError)
          ) {
            if (await waitForActiveChainId(provider, requiredChainId, 3, 150)) {
              return;
            }
            await wait(150);
            continue;
          }
          throw switchAfterAddError;
        }

        if (await waitForActiveChainId(provider, requiredChainId, 3, 150)) {
          return;
        }

        await wait(150);
        continue;
      }

      if (providerHasPendingRequest(error)) {
        if (await waitForActiveChainId(provider, requiredChainId, 3, 150)) {
          return;
        }

        await wait(150);
        continue;
      }

      throw new Error(
        `Wrong network. Switch to ${chainName(requiredChainId)} and retry.`,
      );
    }
  }

  throw new Error(
    `Wrong network. Switch to ${chainName(requiredChainId)} and retry.`,
  );
}

async function assertContractCodeAvailable(
  provider: EthereumProvider,
  requiredChainId: number,
  contractAddress: Address,
) {
  const code = await provider.request({
    method: "eth_getCode",
    params: [contractAddress, "latest"],
  });

  if (typeof code !== "string") {
    throw new Error(
      `Wallet RPC could not verify contract ${shortenAddress(contractAddress)} on ${chainName(requiredChainId)}.`,
    );
  }

  const normalized = code.trim().toLowerCase();
  if (normalized === "0x" || /^0x0+$/.test(normalized)) {
    throw new Error(
      `${chainName(requiredChainId)} is selected, but ${shortenAddress(contractAddress)} is not deployed on the wallet's active RPC. Re-add or switch the network and retry.`,
    );
  }
}

function addEthereumChainParameters(requiredChainId: number) {
  const chain = walletChain(requiredChainId);
  if (chain === null) return null;

  const blockExplorerUrls = Object.values(chain.blockExplorers ?? {}).map(
    (blockExplorer) => blockExplorer.url,
  );

  return {
    chainId: toHexChainId(requiredChainId),
    chainName: chain.name,
    rpcUrls: chain.rpcUrls.default.http,
    nativeCurrency: chain.nativeCurrency,
    ...(blockExplorerUrls.length > 0 ? { blockExplorerUrls } : {}),
  };
}

export function WalletSessionProvider({ children }: PropsWithChildren) {
  const config = useConfig();
  const connection = useConnection();
  const connectors = useConnectors();
  const { connectAsync } = useConnect();
  const { disconnectAsync } = useDisconnect();

  const [availableWallets, setAvailableWallets] = useState<WalletOption[]>([]);
  const [clientReady, setClientReady] = useState(false);
  const [rememberedConnection, setRememberedConnection] = useState(false);
  const [preferredWalletId, setPreferredWalletId] = useState<string | null>(
    null,
  );
  const [disconnecting, setDisconnecting] = useState(false);
  const [switchBusy, setSwitchBusy] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const mountedRef = useRef(false);
  const rememberedConnectionRef = useRef(rememberedConnection);
  const connectIntentRef = useRef(false);
  const preferredWalletIdRef = useRef<string | null>(preferredWalletId);
  const connectorRef = useRef<Connector | undefined>(connection.connector);
  const rawAddress =
    connection.address === undefined
      ? null
      : parseWalletAddress(connection.address);
  const address = disconnecting || !rememberedConnection ? null : rawAddress;
  const chainId =
    disconnecting || !rememberedConnection
      ? null
      : (connection.chainId ?? null);
  const addressRef = useRef<Address | null>(address);
  const chainIdRef = useRef<number | null>(chainId);

  function setRememberedConnectionState(nextRememberedConnection: boolean) {
    rememberedConnectionRef.current = nextRememberedConnection;
    setRememberedConnection(nextRememberedConnection);
  }

  function setPreferredWalletIdState(nextPreferredWalletId: string | null) {
    preferredWalletIdRef.current = nextPreferredWalletId;
    setPreferredWalletId(nextPreferredWalletId);
  }

  useEffect(() => {
    mountedRef.current = true;
    setRememberedConnectionState(readRememberedConnection());
    setPreferredWalletIdState(readPreferredWalletId());
    setClientReady(true);

    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    connectorRef.current = connection.connector;
    addressRef.current = address;
    chainIdRef.current = chainId;
  }, [address, chainId, connection.connector]);

  useEffect(() => {
    let cancelled = false;

    async function refreshAvailability() {
      const nextWallets = await listAvailableWalletConnectors(connectors);
      if (cancelled || !mountedRef.current) return;
      setAvailableWallets(
        nextWallets.map((wallet) => ({ id: wallet.id, name: wallet.name })),
      );
    }

    void refreshAvailability();

    return () => {
      cancelled = true;
    };
  }, [connectors]);

  useEffect(() => {
    if (connection.status === "connected") {
      if (!rememberedConnectionRef.current && !connectIntentRef.current) {
        clearWalletConfigStorage();
        void disconnectAsync({ connector: connection.connector }).catch(
          () => {},
        );
        return;
      }

      setDisconnecting(false);
      if (connection.connector?.id) {
        writePreferredWalletId(connection.connector.id);
        setPreferredWalletIdState(connection.connector.id);
      }
      setSwitchBusy(false);
      setErrorMessage(null);
      return;
    }

    if (
      connection.status === "connecting" ||
      connection.status === "reconnecting"
    ) {
      setDisconnecting(false);
      setErrorMessage(null);
      return;
    }

    setDisconnecting(false);
    setSwitchBusy(false);
  }, [connection.connector?.id, connection.status]);

  async function connect(
    connectorId?: string,
    requiredChainId: number | null = configuredWalletChainId(),
  ): Promise<ConnectedWallet> {
    const nextWallets = await listAvailableWalletConnectors(connectors);
    const nextAvailableWallets = nextWallets.map((wallet) => ({
      id: wallet.id,
      name: wallet.name,
    }));

    setAvailableWallets(nextAvailableWallets);

    if (nextWallets.length === 0) {
      const message = noWalletMessage();
      setErrorMessage(message);
      throw new Error(message);
    }

    const directWallet = resolveDirectWallet(
      nextAvailableWallets,
      preferredWalletIdRef.current,
    );
    const selectedWallet =
      connectorId !== undefined
        ? (nextWallets.find((wallet) => wallet.id === connectorId) ?? null)
        : directWallet === null
          ? null
          : (nextWallets.find((wallet) => wallet.id === directWallet.id) ??
            null);

    if (connectorId !== undefined && selectedWallet === null) {
      const message =
        "Selected wallet is unavailable. Choose a different wallet and retry.";
      setErrorMessage(message);
      throw new Error(message);
    }

    if (selectedWallet === null) {
      const message = "Choose a wallet before connecting.";
      setErrorMessage(message);
      throw new Error(message);
    }

    setErrorMessage(null);
    setDisconnecting(false);
    connectIntentRef.current = true;

    try {
      const result = await connectAsync({
        connector: selectedWallet.connector,
        ...(selectedWallet.connector.id === WALLETCONNECT_CONNECTOR_ID &&
        requiredChainId !== null
          ? { chainId: requiredChainId }
          : {}),
      });
      const [connectedAccount] = result.accounts;
      if (connectedAccount === undefined) {
        throw new Error("Connected wallet did not provide an account.");
      }
      const connectedWallet = {
        address: parseWalletAddress(connectedAccount),
        chainId: result.chainId,
      } satisfies ConnectedWallet;

      addressRef.current = connectedWallet.address;
      chainIdRef.current = connectedWallet.chainId;
      connectorRef.current = selectedWallet.connector;

      writeRememberedConnection(true);
      setRememberedConnectionState(true);
      writePreferredWalletId(selectedWallet.id);
      setPreferredWalletIdState(selectedWallet.id);

      if (requiredChainId !== null) {
        await ensureRequiredChain(requiredChainId);
      }

      return {
        address: connectedWallet.address,
        chainId: chainIdRef.current ?? connectedWallet.chainId,
      } satisfies ConnectedWallet;
    } catch (error) {
      if (addressRef.current !== null) {
        writeRememberedConnection(false);
        setRememberedConnectionState(false);
        setDisconnecting(true);
        await disconnectAsync({ connector: selectedWallet.connector }).catch(
          () => {},
        );
        setDisconnecting(false);
        connectorRef.current = undefined;
        addressRef.current = null;
        chainIdRef.current = null;
      }
      const message = connectionRejectedMessage(error);
      if (mountedRef.current) {
        setErrorMessage(message);
      }
      throw new Error(message);
    } finally {
      connectIntentRef.current = false;
    }
  }

  async function forgetConnection() {
    setErrorMessage(null);
    setSwitchBusy(false);
    setDisconnecting(true);
    writeRememberedConnection(false);
    setRememberedConnectionState(false);
    clearWalletConfigStorage();
    try {
      await disconnectAsync();
    } catch {
      if (mountedRef.current) {
        setDisconnecting(false);
      }
    }
  }

  function clearError() {
    setErrorMessage(null);
  }

  const status =
    !clientReady ||
    disconnecting ||
    ((connection.status === "connected" ||
      connection.status === "reconnecting") &&
      !rememberedConnection)
      ? "disconnected"
      : (connection.status as WalletStatus);
  const providerAvailable = availableWallets.length > 0;

  async function switchChain(requiredChainId: number): Promise<void> {
    const connectedAddress = addressRef.current;
    if (connectedAddress === null) {
      throw new Error("Connect a wallet before continuing.");
    }

    const addParameters = addEthereumChainParameters(requiredChainId);
    if (addParameters === null) {
      const message = `${chainName(requiredChainId)} is not configured in this frontend build. Set PUBLIC_CHAIN_RPC_URL and retry.`;
      setErrorMessage(message);
      throw new Error(message);
    }

    if (chainIdRef.current === requiredChainId) {
      setSwitchBusy(false);
      setErrorMessage(null);
      return;
    }

    setSwitchBusy(true);
    setErrorMessage(null);

    try {
      const provider = await connectedProvider(connectorRef.current);
      if (await waitForActiveChainId(provider, requiredChainId, 1, 0)) {
        chainIdRef.current = requiredChainId;
        if (mountedRef.current) {
          setSwitchBusy(false);
        }
        return;
      }

      await ensureProviderChain(provider, requiredChainId, addParameters);

      const nextChainId = await readActiveChainId(provider);
      if (nextChainId !== requiredChainId) {
        throw new Error(
          `Wrong network. Expected ${chainName(requiredChainId)} before continuing.`,
        );
      }

      if (!mountedRef.current) return;
      chainIdRef.current = nextChainId;
      setSwitchBusy(false);
      setErrorMessage(null);
    } catch (error) {
      const message = providerErrorMessage(
        error,
        `Wrong network. Switch to ${chainName(requiredChainId)} and retry.`,
      );
      if (!mountedRef.current) throw new Error(message);
      setSwitchBusy(false);
      setErrorMessage(message);
      throw new Error(message);
    }
  }

  async function ensureRequiredChain(requiredChainId: number): Promise<void> {
    if (chainIdRef.current === requiredChainId) return;
    await switchChain(requiredChainId);
  }

  async function signClaimBatchTypedData(
    expectedAddress: Address,
    typedData: ClaimBatchTypedData,
    signal?: AbortSignal,
  ): Promise<Hex> {
    const connectedAddress = addressRef.current;
    if (connectedAddress === null) {
      throw new Error("Connect a wallet before continuing.");
    }

    if (!sameAddress(connectedAddress, expectedAddress)) {
      throw new Error(
        `Connected wallet (${shortenAddress(connectedAddress)}) does not match checked address (${shortenAddress(expectedAddress)}).`,
      );
    }

    await assertSignalNotAborted(signal);
    await ensureRequiredChain(typedData.domain.chainId);
    await assertSignalNotAborted(signal);

    const walletClient = await getWalletClient(config, {
      chainId: typedData.domain.chainId,
    });
    if (!walletClient) {
      throw new Error("Connected wallet client is unavailable.");
    }

    return walletClient.signTypedData({
      account: connectedAddress,
      domain: typedData.domain,
      types: CLAIM_BATCH_TYPES,
      primaryType: "ClaimBatch",
      message: {
        ...typedData.message,
        expiry: BigInt(typedData.message.expiry),
        nonce: BigInt(typedData.message.nonce),
      },
    });
  }

  async function sendPreparedTransaction(
    expectedAddress: Address,
    prepared: PreparedWalletTransaction,
    signal?: AbortSignal,
  ): Promise<Hex> {
    const connectedAddress = addressRef.current;
    if (connectedAddress === null) {
      throw new Error("Connect a wallet before continuing.");
    }

    if (!sameAddress(connectedAddress, expectedAddress)) {
      throw new Error(
        `Connected wallet (${shortenAddress(connectedAddress)}) does not match checked address (${shortenAddress(expectedAddress)}).`,
      );
    }

    await assertSignalNotAborted(signal);
    await ensureRequiredChain(prepared.chainId);
    await assertSignalNotAborted(signal);
    const provider = await connectedProvider(connectorRef.current);
    await assertContractCodeAvailable(
      provider,
      prepared.chainId,
      prepared.contractAddress,
    );
    await assertSignalNotAborted(signal);

    const walletClient = await getWalletClient(config, {
      chainId: prepared.chainId,
    });
    if (!walletClient) {
      throw new Error("Connected wallet client is unavailable.");
    }

    const txHash = await walletClient.sendTransaction({
      account: connectedAddress,
      data: prepared.callData,
      to: prepared.contractAddress,
    });

    // Don't hand the hash to the backend until the tx is buried
    // `MIN_TX_CONFIRMATIONS` deep — the backend enforces the same threshold
    // and only accepts the submission if the tx is final.
    await assertSignalNotAborted(signal);
    await waitForTransactionReceipt(config, {
      chainId: prepared.chainId,
      hash: txHash,
      confirmations: MIN_TX_CONFIRMATIONS,
    });
    await assertSignalNotAborted(signal);

    return txHash;
  }

  return (
    <WalletSessionContext.Provider
      value={{
        status,
        providerAvailable,
        availableWallets,
        preferredWalletId,
        address,
        chainId,
        switchBusy,
        errorMessage,
        connect,
        forgetConnection,
        clearError,
        switchChain,
        ensureRequiredChain,
        signClaimBatchTypedData,
        sendPreparedTransaction,
      }}
    >
      {children}
    </WalletSessionContext.Provider>
  );
}

export function useWalletSession(): WalletSessionContextValue {
  const value = useContext(WalletSessionContext);
  if (value === null) {
    throw new Error("Wallet session context is unavailable.");
  }
  return value;
}
