import { afterEach, beforeEach, describe, expect, it, mock } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";

import { emptyIdentityState } from "../../shared/src/identity/types";
import {
  deferred,
  jsonResponse,
  OTHER_ADDRESS,
  TEST_MIGRATION_TOKEN,
  walletWrapper,
} from "../../tests/support/unit-helpers";

const ETH_SENTINEL = "0x0000000000000000000000000000000000000000";
const MIGRATION_CONTRACT = "0x3333333333333333333333333333333333333333";
const DEFAULT_TOKEN_BALANCE = 1_000_000_000_000_000_000n;

let erc20Balances: bigint[] = [DEFAULT_TOKEN_BALANCE];
let ethBalance = 0n;
let migratedByAddress = new Map<string, boolean>();

const getBalanceMock = mock(async () => ({ value: ethBalance }));
const multicallMock = mock(async () => erc20Balances);
const fetchMigratedFlagsMock = mock(async (addresses: string[]) => {
  return new Map(
    addresses.map((address) => [
      address.toLowerCase(),
      migratedByAddress.get(address.toLowerCase()) ?? false,
    ]),
  );
});

mock.module("wagmi/actions", () => ({
  getBalance: getBalanceMock,
  multicall: multicallMock,
}));

mock.module("@shared/migration/onChain", () => ({
  ETH_SENTINEL,
  fetchMigratedFlags: fetchMigratedFlagsMock,
  resolveMigrationChainId: () => 1,
  resolveMigrationContract: () => MIGRATION_CONTRACT,
}));

const { useMigrationWidgetController } =
  await import("../src/components/claim-widget/useMigrationWidgetController");

const expectedVisibleToken = {
  tokenAddress: TEST_MIGRATION_TOKEN.tokenAddress,
  symbol: TEST_MIGRATION_TOKEN.symbol,
  decimals: TEST_MIGRATION_TOKEN.decimals,
  rawBalance: DEFAULT_TOKEN_BALANCE.toString(),
  displayBalance: "1.00",
  migrated: false,
  claimed: false,
};

describe("useMigrationWidgetController", () => {
  const originalFetch = globalThis.fetch;
  const fetchMock = mock(
    async (
      _input: RequestInfo | URL,
      _init?: RequestInit,
    ): Promise<Response> => {
      throw new Error("Unexpected fetch.");
    },
  );

  beforeEach(() => {
    fetchMock.mockReset();
    erc20Balances = [DEFAULT_TOKEN_BALANCE];
    ethBalance = 0n;
    migratedByAddress = new Map();
    getBalanceMock.mockClear();
    multicallMock.mockClear();
    fetchMigratedFlagsMock.mockClear();
    globalThis.fetch = fetchMock as unknown as typeof fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  it("loads migration tokens into the results view", async () => {
    const onRequiredChainChange = mock(() => {});

    fetchMock.mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = input instanceof Request ? input.url : String(input);
        expect(url).toContain("/api/migrations/tokens");
        expect(init?.method ?? "GET").toBe("GET");
        return jsonResponse({
          tokens: [
            {
              ...TEST_MIGRATION_TOKEN,
              tokenAddress: OTHER_ADDRESS,
              symbol: "ZERO",
            },
            TEST_MIGRATION_TOKEN,
          ],
        });
      },
    );
    erc20Balances = [0n, DEFAULT_TOKEN_BALANCE];

    const { result } = renderHook(
      () =>
        useMigrationWidgetController({
          identity: emptyIdentityState,
          onRequiredChainChange,
        }),
      { wrapper: walletWrapper },
    );

    act(() => {
      result.current.actions.showTokens();
    });

    await waitFor(() => {
      expect(result.current.ui.state.view.kind).toBe("loaded");
    });

    expect(result.current.ui.state.view).toMatchObject({
      kind: "loaded",
      tokens: [expectedVisibleToken],
    });
    expect(result.current.ui.eligibilityModalState).toEqual({
      kind: "results",
    });
    expect(result.current.derived.showTokensBusy).toBe(false);
  });

  it("keeps loaded results visible when reset is triggered", async () => {
    const onRequiredChainChange = mock(() => {});

    fetchMock.mockResolvedValue(
      jsonResponse({ tokens: [TEST_MIGRATION_TOKEN] }),
    );

    const { result } = renderHook(
      () =>
        useMigrationWidgetController({
          identity: emptyIdentityState,
          onRequiredChainChange,
        }),
      { wrapper: walletWrapper },
    );

    act(() => {
      result.current.actions.showTokens();
    });

    await waitFor(() => {
      expect(result.current.ui.state.view.kind).toBe("loaded");
    });

    act(() => {
      result.current.actions.resetWidget();
    });

    expect(result.current.ui.state.view).toMatchObject({
      kind: "loaded",
      tokens: [expectedVisibleToken],
    });
    expect(result.current.ui.state.modal).toEqual({ kind: "closed" });
    expect(result.current.ui.eligibilityModalState).toEqual({
      kind: "results",
    });
    expect(onRequiredChainChange).toHaveBeenLastCalledWith(null);
  });

  it("ignores stale token results after a reset during loading", async () => {
    const onRequiredChainChange = mock(() => {});
    const pending = deferred<Response>();

    fetchMock.mockReturnValue(pending.promise);

    const { result } = renderHook(
      () =>
        useMigrationWidgetController({
          identity: emptyIdentityState,
          onRequiredChainChange,
        }),
      { wrapper: walletWrapper },
    );

    act(() => {
      result.current.actions.showTokens();
    });

    await waitFor(() => {
      expect(result.current.ui.state.view.kind).toBe("loading");
    });

    act(() => {
      result.current.actions.resetWidget();
    });

    expect(result.current.ui.state.view).toEqual({ kind: "idle" });
    expect(result.current.ui.eligibilityModalState).toEqual({
      kind: "closed",
    });

    pending.resolve(jsonResponse({ tokens: [TEST_MIGRATION_TOKEN] }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    expect(result.current.ui.state.view).toEqual({ kind: "idle" });
    expect(result.current.ui.eligibilityModalState).toEqual({
      kind: "closed",
    });
  });
});
