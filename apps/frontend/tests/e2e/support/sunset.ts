import { expect, type Page } from "@playwright/test";
import {
  decodeAbiParameters,
  encodeAbiParameters,
  toFunctionSelector,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";

const TEST_PRIVATE_KEY =
  "0x0707070707070707070707070707070707070707070707070707070707070707";
const TEST_ACCOUNT = privateKeyToAccount(TEST_PRIVATE_KEY);
const SECONDARY_PRIVATE_KEY =
  "0x0808080808080808080808080808080808080808080808080808080808080808";
const SECONDARY_ACCOUNT = privateKeyToAccount(SECONDARY_PRIVATE_KEY);
const FUNDED_PRIVATE_KEY =
  process.env.PLAYWRIGHT_E2E_PRIVATE_KEY ??
  "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const FUNDED_ACCOUNT = privateKeyToAccount(FUNDED_PRIVATE_KEY);
const CHAIN_RPC_URL =
  process.env.PLAYWRIGHT_CHAIN_RPC_URL ?? "http://127.0.0.1:5010";

export const VITALIK = TEST_ACCOUNT.address;
export const SECONDARY = SECONDARY_ACCOUNT.address;
export const FUNDED_WALLET = FUNDED_ACCOUNT.address;
export const USDC = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
export const DAI = "0x6B175474E89094C44Da98b954EedeAC495271d0F";
export const WETH = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
export const ETH = "0x0000000000000000000000000000000000000000";

const VERIFYING_CONTRACT = "0x1111111111111111111111111111111111111111";

export const txModalSelector = ".claim-modal:not(.eligibility-modal)";
export const headerIdentitySelector = ".header-identity";

export type ClaimFrontendMode = "unavailable" | "sponsored" | "paid";

type WalletRequestPayload = {
  method: string;
  params?: unknown[];
};

type WalletRequestResult =
  | { ok: true; value: unknown }
  | { ok: false; error: { message: string; code?: number } };

type WalletState = {
  activeChainId: number;
  addChainCount: number;
  switchCount: number;
  signTypedDataCount: number;
  sendTransactionCount: number;
};

function defaultRpcKeyForChainId(chainId: number): string | null {
  if (chainId === 31338) return CHAIN_RPC_URL;
  if (chainId === 31337) return "http://127.0.0.1:8545";
  return null;
}

export function collectErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", (err) => errors.push(err.message));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  return errors;
}

export async function expectNoErrors(errors: string[]) {
  expect(errors).toEqual([]);
}

export async function mockApiReady(page: Page) {
  await page.route("**/readyz", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/plain", body: "ok" });
  });
}

export async function mockClaimConfig(
  page: Page,
  claimMode: ClaimFrontendMode = "sponsored",
) {
  await page.route("**/api/config", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ claimMode }),
    });
  });
}

export async function gotoHome(
  page: Page,
  opts?: { claimMode?: ClaimFrontendMode; mockConfig?: boolean },
) {
  await mockApiReady(page);
  if (opts?.mockConfig ?? true) {
    await mockClaimConfig(page, opts?.claimMode ?? "sponsored");
  }
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Withdraw Your Funds" }),
  ).toBeVisible();
}

export async function gotoMigrate(page: Page) {
  await page.goto(
    process.env.PLAYWRIGHT_MIGRATE_APP_URL ?? "http://127.0.0.1:4322/",
  );
  await expect(
    page.getByRole("heading", { name: "Migrate Token" }),
  ).toBeVisible();
}

export async function fillAndCheck(page: Page, address: string = VITALIK) {
  await page.getByLabel("Ethereum address").fill(address);
  await page.getByRole("button", { name: "Check Remaining Funds" }).click();
}

export async function showMigrationTokens(page: Page) {
  await page.getByRole("button", { name: "Show Tokens" }).click();
}

export async function installClipboardMock(page: Page) {
  await page.addInitScript(() => {
    let clipboardValue = "";

    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        readText: async () => clipboardValue,
        writeText: async (value: string) => {
          clipboardValue = value;
        },
      },
    });
  });
}

export async function clipboardText(page: Page): Promise<string> {
  return page.evaluate(() => navigator.clipboard.readText());
}

export async function installWalletMock(
  page: Page,
  opts?: {
    signDelayMs?: number;
    initialChainId?: number;
    switchReject?: boolean;
    initialAccounts?: string[];
    knownChainIds?: number[];
    addChainSwitches?: boolean;
    pendingSwitchCount?: number;
  },
) {
  const delayMs = opts?.signDelayMs ?? 0;
  const initialChainId = opts?.initialChainId ?? 1;
  const switchReject = opts?.switchReject ?? false;
  const initialAccounts = opts?.initialAccounts ?? [VITALIK];
  const knownChainIds = opts?.knownChainIds ?? [initialChainId];
  const addChainSwitches = opts?.addChainSwitches ?? false;
  const pendingSwitchCount = opts?.pendingSwitchCount ?? 0;
  await page.addInitScript(
    ({
      accounts,
      signature,
      delay,
      chainId,
      rejectSwitch,
      supportedChainIds,
      addChainChangesNetwork,
      initialPendingSwitchCount,
      verifyingContract,
    }) => {
      const sleep = (ms: number) =>
        new Promise((resolve) => {
          setTimeout(resolve, ms);
        });
      const publicSepoliaSentinel = "public-sepolia";
      const defaultRpcKey = (nextChainId: number) =>
        nextChainId === 11155111
          ? publicSepoliaSentinel
          : `chain:${nextChainId}`;
      let activeRpcKey = defaultRpcKey(chainId);
      const addedRpcByChainId = new Map<number, string>();
      let remainingPendingSwitchCount = initialPendingSwitchCount;

      const storageKey = "__wallet_mock_state__";
      const readState = () => {
        const raw = window.localStorage.getItem(storageKey);
        if (!raw) {
          return {
            accounts,
            chainId,
            knownChainIds: supportedChainIds,
            switchCount: 0,
            addChainCount: 0,
            requestAccountsCount: 0,
            signTypedDataCount: 0,
            sendTransactionCount: 0,
          };
        }

        return JSON.parse(raw) as {
          accounts: string[];
          chainId: number;
          knownChainIds: number[];
          switchCount: number;
          addChainCount: number;
          requestAccountsCount: number;
          signTypedDataCount: number;
          sendTransactionCount: number;
        };
      };
      const writeState = (state: ReturnType<typeof readState>) => {
        window.localStorage.setItem(storageKey, JSON.stringify(state));
      };
      const toHexChainId = (value: number) => `0x${value.toString(16)}`;
      const listeners = new Map<string, Set<(...args: unknown[]) => void>>();
      const emit = (event: string, ...args: unknown[]) => {
        const registered = listeners.get(event);
        if (!registered) return;
        for (const listener of registered) {
          listener(...args);
        }
      };
      const updateState = (
        next: (
          state: ReturnType<typeof readState>,
        ) => ReturnType<typeof readState>,
      ) => {
        const updated = next(readState());
        writeState(updated);
        return updated;
      };

      const initialState = readState();
      writeState({
        ...initialState,
        knownChainIds: Array.from(
          new Set([...initialState.knownChainIds, ...supportedChainIds]),
        ),
      });
      activeRpcKey = defaultRpcKey(initialState.chainId);

      (window as unknown as { ethereum?: unknown }).ethereum = {
        on: (event: string, listener: (...args: unknown[]) => void) => {
          const registered = listeners.get(event) ?? new Set();
          registered.add(listener);
          listeners.set(event, registered);
        },
        removeListener: (
          event: string,
          listener: (...args: unknown[]) => void,
        ) => {
          listeners.get(event)?.delete(listener);
        },
        request: async ({
          method,
          params,
        }: {
          method: string;
          params?: unknown[];
        }) => {
          if (method === "eth_accounts") return readState().accounts;
          if (method === "eth_requestAccounts") {
            return updateState((state) => ({
              ...state,
              requestAccountsCount: state.requestAccountsCount + 1,
            })).accounts;
          }
          if (method === "eth_chainId") {
            return toHexChainId(readState().chainId);
          }
          if (method === "wallet_switchEthereumChain") {
            if (rejectSwitch) {
              throw Object.assign(new Error("User rejected"), { code: 4001 });
            }
            const next = params?.[0] as { chainId?: string } | undefined;
            if (!next?.chainId || !next.chainId.startsWith("0x")) {
              throw Object.assign(new Error("Invalid chainId"), {
                code: -32602,
              });
            }
            const nextChainId = Number.parseInt(next.chainId, 16);
            const state = readState();
            if (!state.knownChainIds.includes(nextChainId)) {
              throw Object.assign(new Error("Unknown chain"), { code: 4902 });
            }
            if (remainingPendingSwitchCount > 0) {
              remainingPendingSwitchCount -= 1;
              throw Object.assign(new Error("Request already pending"), {
                code: -32002,
              });
            }
            updateState((current) => ({
              ...current,
              chainId: nextChainId,
              switchCount: current.switchCount + 1,
            }));
            activeRpcKey =
              addedRpcByChainId.get(nextChainId) ?? defaultRpcKey(nextChainId);
            emit("chainChanged", toHexChainId(nextChainId));
            emit("connect", { chainId: toHexChainId(nextChainId) });
            return null;
          }
          if (method === "wallet_addEthereumChain") {
            const next = params?.[0] as
              | { chainId?: string; rpcUrls?: string[] }
              | undefined;
            if (!next?.chainId || !next.chainId.startsWith("0x")) {
              throw Object.assign(new Error("Invalid chainId"), {
                code: -32602,
              });
            }
            const nextChainId = Number.parseInt(next.chainId, 16);
            const nextRpcUrl = next.rpcUrls?.[0];
            if (typeof nextRpcUrl === "string" && nextRpcUrl.length > 0) {
              addedRpcByChainId.set(nextChainId, nextRpcUrl);
            }
            updateState((current) => ({
              ...current,
              knownChainIds: Array.from(
                new Set([...current.knownChainIds, nextChainId]),
              ),
              addChainCount: current.addChainCount + 1,
              chainId: addChainChangesNetwork ? nextChainId : current.chainId,
            }));
            if (addChainChangesNetwork) {
              activeRpcKey =
                addedRpcByChainId.get(nextChainId) ??
                defaultRpcKey(nextChainId);
              emit("chainChanged", toHexChainId(nextChainId));
              emit("connect", { chainId: toHexChainId(nextChainId) });
            }
            return null;
          }
          if (method === "eth_getCode") {
            const targetAddress = params?.[0];
            if (
              activeRpcKey !== publicSepoliaSentinel &&
              typeof targetAddress === "string" &&
              targetAddress.toLowerCase() === verifyingContract.toLowerCase()
            ) {
              return "0x1234";
            }
            return "0x";
          }
          if (method === "eth_signTypedData_v4") {
            updateState((state) => ({
              ...state,
              signTypedDataCount: state.signTypedDataCount + 1,
            }));
            if (delay > 0) await sleep(delay);
            return signature;
          }
          if (method === "eth_sendTransaction") {
            if (
              readState().chainId === 11155111 &&
              activeRpcKey === publicSepoliaSentinel
            ) {
              throw new Error("insufficient funds on public Sepolia");
            }
            updateState((state) => ({
              ...state,
              sendTransactionCount: state.sendTransactionCount + 1,
            }));
            return `0x${"2".repeat(64)}`;
          }
          throw new Error(`Unsupported wallet method: ${method}`);
        },
      };

      (
        window as unknown as {
          __walletMock?: {
            emitAccountsChanged: (nextAccounts: string[]) => void;
            emitChainChanged: (nextChainId: number) => void;
            emitDisconnect: () => void;
          };
        }
      ).__walletMock = {
        emitAccountsChanged: (nextAccounts: string[]) => {
          updateState((state) => ({
            ...state,
            accounts: nextAccounts,
          }));
          emit("accountsChanged", nextAccounts);
        },
        emitChainChanged: (nextChainId: number) => {
          updateState((state) => ({
            ...state,
            chainId: nextChainId,
          }));
          activeRpcKey =
            addedRpcByChainId.get(nextChainId) ?? defaultRpcKey(nextChainId);
          emit("chainChanged", toHexChainId(nextChainId));
        },
        emitDisconnect: () => {
          updateState((state) => ({
            ...state,
            accounts: [],
          }));
          emit("accountsChanged", []);
          emit("disconnect", { code: 4900, message: "Disconnected" });
        },
      };
    },
    {
      accounts: initialAccounts,
      signature: `0x${"1".repeat(130)}`,
      delay: delayMs,
      chainId: initialChainId,
      rejectSwitch: switchReject,
      supportedChainIds: knownChainIds,
      addChainChangesNetwork: addChainSwitches,
      initialPendingSwitchCount: pendingSwitchCount,
      verifyingContract: VERIFYING_CONTRACT,
    },
  );
}

export async function installRpcBackedWallet(
  page: Page,
  opts?: {
    initialChainId?: number;
  },
) {
  const context = page.context();
  let activeChainId = opts?.initialChainId ?? 31338;
  let activeRpcKey = defaultRpcKeyForChainId(activeChainId);
  const knownChainIds = new Set<number>([activeChainId]);
  let addChainCount = 0;
  let switchCount = 0;
  let signTypedDataCount = 0;
  let sendTransactionCount = 0;
  const addedRpcByChainId = new Map<number, string>();

  await context.exposeFunction(
    "__e2eEthereumRequest",
    async (payload: WalletRequestPayload): Promise<WalletRequestResult> => {
      try {
        if (payload.method === "eth_requestAccounts") {
          return { ok: true, value: [FUNDED_WALLET] };
        }
        if (payload.method === "eth_chainId") {
          return { ok: true, value: `0x${activeChainId.toString(16)}` };
        }
        if (
          payload.method === "wallet_switchEthereumChain" ||
          payload.method === "wallet_addEthereumChain"
        ) {
          const next = payload.params?.[0] as { chainId?: string } | undefined;
          if (!next?.chainId || !next.chainId.startsWith("0x")) {
            throw new Error("wallet chain request is missing a hex chainId");
          }
          if (payload.method === "wallet_addEthereumChain") {
            const chainParams = payload.params?.[0] as
              | { rpcUrls?: string[] }
              | undefined;
            const nextRpcUrl = chainParams?.rpcUrls?.[0];
            if (typeof nextRpcUrl === "string" && nextRpcUrl.length > 0) {
              addedRpcByChainId.set(
                Number.parseInt(next.chainId, 16),
                nextRpcUrl,
              );
            }
            knownChainIds.add(Number.parseInt(next.chainId, 16));
            addChainCount += 1;
          } else {
            const nextChainId = Number.parseInt(next.chainId, 16);
            if (!knownChainIds.has(nextChainId)) {
              throw Object.assign(new Error("Unknown chain"), { code: 4902 });
            }
            switchCount += 1;
            activeChainId = nextChainId;
            activeRpcKey =
              addedRpcByChainId.get(activeChainId) ??
              defaultRpcKeyForChainId(activeChainId);
          }
          return { ok: true, value: null };
        }
        if (payload.method === "eth_getCode") {
          if (!activeRpcKey) {
            throw new Error(`no RPC URL configured for chain ${activeChainId}`);
          }

          return {
            ok: true,
            value: await jsonRpc(
              activeRpcKey,
              "eth_getCode",
              payload.params ?? [],
            ),
          };
        }
        if (payload.method === "eth_signTypedData_v4") {
          signTypedDataCount += 1;
          const typedDataRaw = payload.params?.[1];
          if (typeof typedDataRaw !== "string") {
            throw new Error(
              "eth_signTypedData_v4 payload was not a JSON string",
            );
          }
          const typedData = JSON.parse(typedDataRaw) as {
            domain: Record<string, unknown>;
            types: Record<string, Array<{ name: string; type: string }>>;
            primaryType: string;
            message: Record<string, unknown>;
          };
          const message = {
            ...typedData.message,
            expiry: BigInt(String(typedData.message.expiry)),
            nonce: BigInt(String(typedData.message.nonce)),
          };
          const { EIP712Domain: _domain, ...types } = typedData.types;
          return {
            ok: true,
            value: await FUNDED_ACCOUNT.signTypedData({
              domain: typedData.domain as never,
              types: types as never,
              primaryType: typedData.primaryType as never,
              message: message as never,
            }),
          };
        }
        if (payload.method === "eth_sendTransaction") {
          sendTransactionCount += 1;
          if (!activeRpcKey) {
            throw new Error(`no RPC URL configured for chain ${activeChainId}`);
          }
          return {
            ok: true,
            value: await jsonRpc(
              activeRpcKey,
              "eth_sendTransaction",
              payload.params ?? [],
            ),
          };
        }

        throw new Error(`Unsupported wallet method: ${payload.method}`);
      } catch (error) {
        return {
          ok: false,
          error: {
            message:
              error instanceof Error
                ? error.message
                : "Wallet request failed in the compose test harness.",
            code:
              typeof error === "object" &&
              error !== null &&
              "code" in error &&
              typeof error.code === "number"
                ? error.code
                : undefined,
          },
        };
      }
    },
  );

  await context.exposeFunction(
    "__e2eEthereumState",
    async (): Promise<WalletState> => ({
      activeChainId,
      addChainCount,
      switchCount,
      signTypedDataCount,
      sendTransactionCount,
    }),
  );

  await context.addInitScript(() => {
    type RequestArgs = { method: string; params?: unknown[] };
    type RequestResult =
      | { ok: true; value: unknown }
      | { ok: false; error: { message: string; code?: number } };
    const listeners = new Map<string, Set<(...args: unknown[]) => void>>();
    const emit = (event: string, ...args: unknown[]) => {
      const registered = listeners.get(event);
      if (!registered) return;
      for (const listener of registered) {
        listener(...args);
      }
    };
    const request = async (args: RequestArgs) => {
      const result = await (
        window as unknown as {
          __e2eEthereumRequest: (
            payload: RequestArgs,
          ) => Promise<RequestResult>;
        }
      ).__e2eEthereumRequest(args);

      if (!result.ok) {
        throw Object.assign(new Error(result.error.message), {
          ...(result.error.code !== undefined
            ? { code: result.error.code }
            : {}),
        });
      }

      const value = result.value;

      if (args.method === "eth_requestAccounts") {
        const accounts = Array.isArray(value) ? value : [];
        emit("accountsChanged", accounts);
      }

      if (args.method === "wallet_switchEthereumChain") {
        const chainId = (args.params?.[0] as { chainId?: string } | undefined)
          ?.chainId;
        if (typeof chainId === "string") {
          emit("chainChanged", chainId);
          emit("connect", { chainId });
        }
      }

      return value;
    };

    (window as unknown as { ethereum?: unknown }).ethereum = {
      on: (event: string, listener: (...args: unknown[]) => void) => {
        const registered = listeners.get(event) ?? new Set();
        registered.add(listener);
        listeners.set(event, registered);
      },
      removeListener: (
        event: string,
        listener: (...args: unknown[]) => void,
      ) => {
        listeners.get(event)?.delete(listener);
      },
      request,
    };
  });
}

export async function walletState(page: Page): Promise<WalletState> {
  return page.evaluate(() =>
    (
      window as unknown as { __e2eEthereumState: () => Promise<WalletState> }
    ).__e2eEthereumState(),
  );
}

export async function installMultiWalletMock(
  page: Page,
  opts?: {
    initialChainId?: number;
    initialAccounts?: string[];
  },
) {
  const initialChainId = opts?.initialChainId ?? 1;
  const initialAccounts = opts?.initialAccounts ?? [VITALIK];

  await page.addInitScript(
    ({ accounts, chainId }) => {
      const storageKey = "__multi_wallet_mock_state__";
      const toHexChainId = (value: number) => `0x${value.toString(16)}`;
      const readState = () => {
        const raw = window.localStorage.getItem(storageKey);
        if (!raw) {
          return {
            accounts,
            chainId,
            knownChainIds: [chainId],
            braveWalletConnectCount: 0,
            metaMaskConnectCount: 0,
          };
        }

        return JSON.parse(raw) as {
          accounts: string[];
          chainId: number;
          knownChainIds: number[];
          braveWalletConnectCount: number;
          metaMaskConnectCount: number;
        };
      };
      const writeState = (state: ReturnType<typeof readState>) => {
        window.localStorage.setItem(storageKey, JSON.stringify(state));
      };
      const updateState = (
        next: (
          state: ReturnType<typeof readState>,
        ) => ReturnType<typeof readState>,
      ) => {
        const updated = next(readState());
        writeState(updated);
        return updated;
      };

      const createProvider = (kind: "metaMask" | "braveWallet") => {
        const listeners = new Map<string, Set<(...args: unknown[]) => void>>();

        return {
          ...(kind === "metaMask"
            ? { isMetaMask: true, _events: {}, _state: {} }
            : { isBraveWallet: true }),
          on: (event: string, listener: (...args: unknown[]) => void) => {
            const registered = listeners.get(event) ?? new Set();
            registered.add(listener);
            listeners.set(event, registered);
          },
          removeListener: (
            event: string,
            listener: (...args: unknown[]) => void,
          ) => {
            listeners.get(event)?.delete(listener);
          },
          request: async ({
            method,
            params,
          }: {
            method: string;
            params?: unknown[];
          }) => {
            if (method === "eth_accounts") return readState().accounts;
            if (
              method === "wallet_requestPermissions" ||
              method === "eth_requestAccounts"
            ) {
              updateState((state) => ({
                ...state,
                [`${kind}ConnectCount`]: state[`${kind}ConnectCount`] + 1,
              }));

              if (method === "wallet_requestPermissions") {
                return [{ caveats: [{ value: readState().accounts }] }];
              }

              return readState().accounts;
            }
            if (method === "wallet_revokePermissions") return null;
            if (method === "eth_chainId") {
              return toHexChainId(readState().chainId);
            }
            if (method === "wallet_addEthereumChain") {
              const next = params?.[0] as { chainId?: string } | undefined;
              if (!next?.chainId || !next.chainId.startsWith("0x")) {
                throw Object.assign(new Error("Invalid chainId"), {
                  code: -32602,
                });
              }
              const nextChainId = Number.parseInt(next.chainId, 16);
              updateState((state) => ({
                ...state,
                knownChainIds: Array.from(
                  new Set([...state.knownChainIds, nextChainId]),
                ),
              }));
              return null;
            }
            if (method === "wallet_switchEthereumChain") {
              const next = params?.[0] as { chainId?: string } | undefined;
              if (!next?.chainId || !next.chainId.startsWith("0x")) {
                throw Object.assign(new Error("Invalid chainId"), {
                  code: -32602,
                });
              }
              const nextChainId = Number.parseInt(next.chainId, 16);
              if (!readState().knownChainIds.includes(nextChainId)) {
                throw Object.assign(new Error("Unknown chain"), { code: 4902 });
              }
              updateState((state) => ({
                ...state,
                chainId: nextChainId,
              }));
              emit("chainChanged", toHexChainId(nextChainId));
              emit("connect", { chainId: toHexChainId(nextChainId) });
              return null;
            }
            throw new Error(`Unsupported ${kind} wallet method: ${method}`);
          },
        };
      };

      const metaMaskProvider = createProvider("metaMask");
      const braveWalletProvider = createProvider("braveWallet");

      writeState(readState());

      (
        window as unknown as {
          ethereum?: {
            providers: unknown[];
            request: ({
              method,
            }: {
              method: string;
              params?: unknown[];
            }) => Promise<unknown>;
          };
        }
      ).ethereum = {
        providers: [metaMaskProvider, braveWalletProvider],
        request: async ({ method }: { method: string; params?: unknown[] }) => {
          throw new Error(
            `Generic injected provider should not be used in this test: ${method}`,
          );
        },
      };
    },
    {
      accounts: initialAccounts,
      chainId: initialChainId,
    },
  );
}

export async function signTypedDataCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(window.localStorage.getItem("__wallet_mock_state__") ?? "{}")
        .signTypedDataCount ?? 0,
  );
}

export async function sendTransactionCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(window.localStorage.getItem("__wallet_mock_state__") ?? "{}")
        .sendTransactionCount ?? 0,
  );
}

export async function requestAccountsCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(window.localStorage.getItem("__wallet_mock_state__") ?? "{}")
        .requestAccountsCount ?? 0,
  );
}

export async function metaMaskConnectCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(
        window.localStorage.getItem("__multi_wallet_mock_state__") ?? "{}",
      ).metaMaskConnectCount ?? 0,
  );
}

export async function braveWalletConnectCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(
        window.localStorage.getItem("__multi_wallet_mock_state__") ?? "{}",
      ).braveWalletConnectCount ?? 0,
  );
}

export async function switchChainCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(window.localStorage.getItem("__wallet_mock_state__") ?? "{}")
        .switchCount ?? 0,
  );
}

export async function addChainCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      JSON.parse(window.localStorage.getItem("__wallet_mock_state__") ?? "{}")
        .addChainCount ?? 0,
  );
}

export async function emitAccountsChanged(page: Page, accounts: string[]) {
  await page.evaluate((nextAccounts) => {
    (
      window as unknown as {
        __walletMock?: { emitAccountsChanged: (accounts: string[]) => void };
      }
    ).__walletMock?.emitAccountsChanged(nextAccounts);
  }, accounts);
}

export async function emitChainChanged(page: Page, chainId: number) {
  await page.evaluate((nextChainId) => {
    (
      window as unknown as {
        __walletMock?: { emitChainChanged: (chainId: number) => void };
      }
    ).__walletMock?.emitChainChanged(nextChainId);
  }, chainId);
}

export async function emitDisconnect(page: Page) {
  await page.evaluate(() => {
    (
      window as unknown as {
        __walletMock?: { emitDisconnect: () => void };
      }
    ).__walletMock?.emitDisconnect();
  });
}

async function jsonRpc(
  rpcUrl: string,
  method: string,
  params: unknown[],
): Promise<string> {
  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method,
      params,
    }),
  });
  if (!response.ok) {
    throw new Error(`rpc ${method} returned HTTP ${response.status}`);
  }

  const payload = (await response.json()) as {
    result?: string;
    error?: { message?: string };
  };
  if (payload.error) {
    throw new Error(
      `rpc ${method} failed: ${payload.error.message ?? "unknown error"}`,
    );
  }
  if (!payload.result || !payload.result.startsWith("0x")) {
    throw new Error(`rpc ${method} returned an invalid result`);
  }
  return payload.result;
}

const MULTICALL3_AGGREGATE3_SELECTOR = "0x82ad56cb";
const IS_TOKEN_MIGRATED_SELECTOR = toFunctionSelector(
  "isTokenMigrated(address)",
).toLowerCase();
const IS_CLAIMED_SELECTOR =
  toFunctionSelector("isClaimed(uint256)").toLowerCase();
const BALANCE_OF_SELECTOR =
  toFunctionSelector("balanceOf(address)").toLowerCase();

const ZERO_BOOL_RETURN: `0x${string}` = `0x${"0".repeat(64)}`;
const TRUE_BOOL_RETURN: `0x${string}` = `0x${"0".repeat(63)}1`;

const UINT256_TUPLE = [{ type: "uint256" as const }];

const MULTICALL3_RESULT_TUPLE = [
  {
    type: "tuple[]" as const,
    components: [
      { name: "success", type: "bool" as const },
      { name: "returnData", type: "bytes" as const },
    ],
  },
];

const MULTICALL3_CALLS_TUPLE = [
  {
    type: "tuple[]" as const,
    components: [
      { name: "target", type: "address" as const },
      { name: "allowFailure", type: "bool" as const },
      { name: "callData", type: "bytes" as const },
    ],
  },
];

type MulticallCall = {
  target: `0x${string}`;
  allowFailure: boolean;
  callData: `0x${string}`;
};

export type ChainRpcOptions = {
  /** Lower-cased token addresses that should report `isTokenMigrated = true`. */
  migratedTokens?: readonly string[];
  /** Decimal-string claim indices that should report `isClaimed = true`. */
  claimedIndices?: readonly (number | bigint | string)[];
  /** ERC20 balanceOf responses keyed by token contract address. */
  tokenBalances?: Readonly<Record<string, bigint | number | string>>;
  /** Native balance returned by eth_getBalance. */
  nativeBalance?: bigint | number | string;
};

function toBigInt(value: bigint | number | string): bigint {
  return typeof value === "bigint" ? value : BigInt(value);
}

function encodeUint256(value: bigint | number | string): `0x${string}` {
  return encodeAbiParameters(UINT256_TUPLE, [toBigInt(value)]);
}

function encodeQuantity(value: bigint | number | string): `0x${string}` {
  return `0x${toBigInt(value).toString(16)}` as `0x${string}`;
}

function tokenBalanceReturnData(
  tokenAddress: string | undefined,
  options: ChainRpcOptions,
): `0x${string}` | null {
  if (!tokenAddress) return null;
  const balanceEntry = Object.entries(options.tokenBalances ?? {}).find(
    ([address]) => address.toLowerCase() === tokenAddress.toLowerCase(),
  );
  const balance = balanceEntry?.[1];
  return balance === undefined ? null : encodeUint256(balance);
}

function decodeAggregate3Calls(input: string): MulticallCall[] | null {
  if (!input.toLowerCase().startsWith(MULTICALL3_AGGREGATE3_SELECTOR)) {
    return null;
  }
  try {
    const args = `0x${input.slice(2 + 8)}` as `0x${string}`;
    const decoded = decodeAbiParameters(MULTICALL3_CALLS_TUPLE, args);
    return decoded[0] as MulticallCall[];
  } catch {
    return null;
  }
}

function encodeAggregate3Results(
  results: Array<{ success: boolean; returnData: `0x${string}` }>,
): `0x${string}` {
  return encodeAbiParameters(MULTICALL3_RESULT_TUPLE, [results]);
}

function buildAggregate3Response(
  data: string,
  options: ChainRpcOptions,
): `0x${string}` {
  const calls = decodeAggregate3Calls(data);
  if (calls === null) {
    return ZERO_BOOL_RETURN;
  }

  const migratedSet = new Set(
    (options.migratedTokens ?? []).map((addr) => addr.toLowerCase()),
  );
  const claimedSet = new Set(
    (options.claimedIndices ?? []).map((idx) => BigInt(idx).toString()),
  );

  const results = calls.map((call) => {
    const callData = call.callData.toLowerCase();
    if (callData.startsWith(BALANCE_OF_SELECTOR)) {
      return {
        success: true,
        returnData:
          tokenBalanceReturnData(call.target, options) ?? ZERO_BOOL_RETURN,
      };
    }
    if (callData.startsWith(IS_TOKEN_MIGRATED_SELECTOR)) {
      // calldata: selector (4) + 32-byte address (left-padded)
      const tokenHex = `0x${callData.slice(10 + 24)}`;
      const isMigrated = migratedSet.has(tokenHex);
      return {
        success: true,
        returnData: isMigrated ? TRUE_BOOL_RETURN : ZERO_BOOL_RETURN,
      };
    }
    if (callData.startsWith(IS_CLAIMED_SELECTOR)) {
      const indexHex = callData.slice(10);
      const index = BigInt(`0x${indexHex}`);
      const isClaimed = claimedSet.has(index.toString());
      return {
        success: true,
        returnData: isClaimed ? TRUE_BOOL_RETURN : ZERO_BOOL_RETURN,
      };
    }
    return { success: true, returnData: ZERO_BOOL_RETURN };
  });

  return encodeAggregate3Results(results);
}

type JsonRpcRequest = {
  id?: number | string;
  method?: string;
  params?: unknown[];
};

function handleChainRpcRequest(req: JsonRpcRequest, options: ChainRpcOptions) {
  const id = req.id ?? 1;
  const ok = (result: unknown) => ({ jsonrpc: "2.0", id, result });

  if (req.method === "eth_chainId") return ok("0x7a6a");
  if (req.method === "net_version") return ok("31338");
  // Always return a comfortably high block height so viem's
  // `waitForTransactionReceipt({ confirmations: 4 })` resolves immediately
  // when paired with the canned receipt below.
  if (req.method === "eth_blockNumber") return ok("0x100");
  if (req.method === "eth_getBalance") {
    return ok(encodeQuantity(options.nativeBalance ?? 0n));
  }
  if (req.method === "eth_gasPrice") return ok("0x0");
  if (req.method === "eth_getCode") {
    return ok("0x60006000fd");
  }
  if (
    req.method === "eth_getTransactionReceipt" ||
    req.method === "eth_getTransactionByHash"
  ) {
    const hash =
      (req.params?.[0] as string | undefined) ?? `0x${"0".repeat(64)}`;
    return ok({
      transactionHash: hash,
      transactionIndex: "0x0",
      blockHash: `0x${"0".repeat(64)}`,
      blockNumber: "0x1",
      from: `0x${"0".repeat(40)}`,
      to: `0x${"0".repeat(40)}`,
      cumulativeGasUsed: "0x0",
      gasUsed: "0x0",
      contractAddress: null,
      logs: [],
      logsBloom: `0x${"0".repeat(512)}`,
      status: "0x1",
      type: "0x2",
      effectiveGasPrice: "0x0",
    });
  }
  if (req.method === "eth_call") {
    const call = req.params?.[0] as { data?: string; to?: string } | undefined;
    const data = (call?.data ?? "0x").toLowerCase();
    if (data.startsWith(MULTICALL3_AGGREGATE3_SELECTOR)) {
      return ok(buildAggregate3Response(data, options));
    }
    if (data.startsWith(BALANCE_OF_SELECTOR)) {
      return ok(tokenBalanceReturnData(call?.to, options) ?? ZERO_BOOL_RETURN);
    }
    return ok(ZERO_BOOL_RETURN);
  }
  return ok(null);
}

export async function mockChainRpc(page: Page, options: ChainRpcOptions = {}) {
  await page.route(/127\.0\.0\.1:5010/, async (route) => {
    const request = route.request();
    if (request.method() !== "POST") {
      await route.continue();
      return;
    }
    const raw = request.postData();
    if (!raw) {
      await route.continue();
      return;
    }

    let body: JsonRpcRequest | JsonRpcRequest[];
    try {
      body = JSON.parse(raw) as JsonRpcRequest | JsonRpcRequest[];
    } catch {
      await route.continue();
      return;
    }

    const response = Array.isArray(body)
      ? body.map((entry) => handleChainRpcRequest(entry, options))
      : handleChainRpcRequest(body, options);

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(response),
    });
  });
}

export async function mockMigrationFlow(
  page: Page,
  opts?: {
    migrationId?: string;
    tokenAddress?: string;
    tokens?: string[];
    onTokensRequest?: () => void;
    onPrepareRequest?: (body: unknown) => void;
    onSubmitRequest?: (body: unknown) => void;
    onStatusRequest?: (migrationId: string) => void;
  },
) {
  const migrationId = opts?.migrationId ?? "mig_test";
  const tokenAddress = opts?.tokenAddress ?? DAI;
  const tokens = opts?.tokens ?? [tokenAddress];
  const tokenSymbols = new Map<string, string>([
    [DAI, "DAI"],
    [USDC, "USDC"],
    [WETH, "WETH"],
    [ETH, "ETH"],
  ]);
  const mockTokenBalance = "4000000000000000000";
  const tokenBalances = Object.fromEntries(
    tokens
      .filter((address) => address.toLowerCase() !== ETH)
      .map((address) => [address.toLowerCase(), mockTokenBalance]),
  );

  await mockChainRpc(page, {
    nativeBalance: tokens.some((address) => address.toLowerCase() === ETH)
      ? mockTokenBalance
      : 0n,
    tokenBalances,
  });

  await page.route(/\/api\/migrations(?:\/.*)?(?:\?.*)?$/, async (route) => {
    const url = route.request().url();
    const method = route.request().method();

    if (method === "GET" && url.endsWith("/api/migrations/tokens")) {
      opts?.onTokensRequest?.();
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          tokens: tokens.map((address, index) => ({
            tokenAddress: address,
            symbol: tokenSymbols.get(address) ?? `TOKEN${index + 1}`,
            decimals: 18,
          })),
        }),
      });
      return;
    }

    if (url.includes("/api/migrations/prepare")) {
      opts?.onPrepareRequest?.(route.request().postDataJSON());
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          migrationId,
          address: VITALIK,
          chainId: 31338,
          contractAddress: "0x1111111111111111111111111111111111111111",
          item: {
            index: 1,
            tokenAddress,
            total: "4000000000000000000",
            merkleProof: [`0x${"f".repeat(64)}`],
          },
          claimRoot: `0x${"a".repeat(64)}`,
          tokenMigrationRoot: `0x${"b".repeat(64)}`,
          callData: `0x${"c".repeat(64)}`,
        }),
      });
      return;
    }

    if (method === "POST" && url.endsWith("/api/migrations")) {
      opts?.onSubmitRequest?.(route.request().postDataJSON());
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          migrationId,
          status: "submitted",
          txHash: `0x${"2".repeat(64)}`,
        }),
      });
      return;
    }

    const statusMatch = url.match(/\/api\/migrations\/([^/?#]+)/);
    if (method === "GET" && statusMatch) {
      opts?.onStatusRequest?.(
        decodeURIComponent(statusMatch[1] ?? migrationId),
      );
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          migrationId,
          status: "mined_success",
          txHash: `0x${"2".repeat(64)}`,
          receipt: {
            blockNumber: 2_234_567,
            blockHash: `0x${"3".repeat(64)}`,
          },
        }),
      });
      return;
    }

    await route.continue();
  });
}

export async function mockClaimFlow(
  page: Page,
  opts?: {
    claimId?: string;
    tokenAddresses?: string[];
    onPrepareRequest?: (body: unknown) => void;
    onSubmitRequest?: (body: unknown) => void;
    onStatusRequest?: (claimId: string) => void;
  },
) {
  const claimId = opts?.claimId ?? "claim_test";
  const tokenAddresses = opts?.tokenAddresses ?? [DAI];
  const items = tokenAddresses.map((tokenAddress, index) => ({
    index,
    tokenAddress,
    amount: `${(index + 1) * 1000}`,
    merkleProof: [`0x${"a".repeat(64)}`],
  }));

  await page.route(/\/api\/claims(?:\/.*)?(?:\?.*)?$/, async (route) => {
    const url = route.request().url();
    const method = route.request().method();

    if (url.includes("/api/claims/prepare")) {
      opts?.onPrepareRequest?.(route.request().postDataJSON());
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          address: VITALIK,
          chainId: 31338,
          verifyingContract: VERIFYING_CONTRACT,
          claimsHash: `0x${"b".repeat(64)}`,
          nonce: 0,
          expiry: 1_900_000_000,
          items,
          claimId,
          directTx: {
            chainId: 31338,
            contractAddress: VERIFYING_CONTRACT,
            callData: `0x${"e".repeat(8)}`,
          },
          typedData: {
            primaryType: "ClaimBatch",
            domain: {
              name: "ZkSyncLiteSunsetClaimDistributor",
              version: "1",
              chainId: 31338,
              verifyingContract: VERIFYING_CONTRACT,
            },
            message: {
              claimant: VITALIK,
              claimsHash: `0x${"b".repeat(64)}`,
              nonce: 0,
              expiry: 1_900_000_000,
            },
          },
        }),
      });
      return;
    }

    if (method === "POST" && url.includes("/api/claims/direct")) {
      opts?.onSubmitRequest?.(route.request().postDataJSON());
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          claimId,
          status: "submitted",
          txHash: `0x${"c".repeat(64)}`,
        }),
      });
      return;
    }

    if (method === "POST" && /\/api\/claims(?:\?.*)?$/.test(url)) {
      opts?.onSubmitRequest?.(route.request().postDataJSON());
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          claimId,
          status: "pending",
        }),
      });
      return;
    }

    const match = url.match(/\/api\/claims\/([^/?#]+)/);
    if (method === "GET" && match) {
      opts?.onStatusRequest?.(match[1]!);
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          claimId,
          status: "mined_success",
          txHash: `0x${"c".repeat(64)}`,
          receipt: {
            blockNumber: 123,
            blockHash: `0x${"d".repeat(64)}`,
          },
        }),
      });
      return;
    }

    await route.continue();
  });
}
