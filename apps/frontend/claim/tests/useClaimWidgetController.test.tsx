import { afterEach, beforeEach, describe, expect, it, mock } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";

import { emptyIdentityState } from "../../shared/src/identity/types";
import { useClaimWidgetController } from "../src/components/claim-widget/useClaimWidgetController";
import {
  jsonResponse,
  OTHER_ADDRESS,
  TEST_ADDRESS,
  TEST_TOKEN,
  walletWrapper,
} from "../../tests/support/unit-helpers";

describe("useClaimWidgetController", () => {
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
    globalThis.fetch = fetchMock as unknown as typeof fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  it("auto-checks a wallet identity and loads its eligibility once", async () => {
    const onRequiredChainChange = mock(() => {});
    const onSelectedAddressChange = mock(() => {});
    const zeroToken = {
      ...TEST_TOKEN,
      claimIndex: 1,
      tokenAddress: OTHER_ADDRESS,
      displayBalance: "0.00",
      rawBalance: "0",
    };

    fetchMock.mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = input instanceof Request ? input.url : String(input);
        expect(url).toContain("/api/eligibility");
        expect(init?.method).toBe("POST");
        expect(JSON.parse(String(init?.body))).toEqual({
          address: TEST_ADDRESS,
        });
        return jsonResponse({
          address: TEST_ADDRESS,
          tokens: [zeroToken, TEST_TOKEN],
        });
      },
    );

    const { result } = renderHook(
      () =>
        useClaimWidgetController({
          identity: { source: "wallet", address: TEST_ADDRESS },
          onRequiredChainChange,
          onSelectedAddressChange,
        }),
      { wrapper: walletWrapper },
    );

    await waitFor(() => {
      expect(result.current.ui.state.view.kind).toBe("loaded");
    });

    expect(result.current.ui.state.view).toMatchObject({
      kind: "loaded",
      address: TEST_ADDRESS,
      tokens: [TEST_TOKEN],
    });
    expect(result.current.ui.addressInput).toBe(TEST_ADDRESS);
    expect(result.current.ui.eligibilityModalState).toEqual({
      kind: "results",
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(onSelectedAddressChange).toHaveBeenCalledWith(null);
  });

  it("clears a selected-address lookup back to idle state", async () => {
    const onRequiredChainChange = mock(() => {});
    const onSelectedAddressChange = mock(() => {});

    fetchMock.mockResolvedValue(
      jsonResponse({
        address: OTHER_ADDRESS,
        tokens: [TEST_TOKEN],
      }),
    );

    const { result } = renderHook(
      () =>
        useClaimWidgetController({
          identity: emptyIdentityState,
          onRequiredChainChange,
          onSelectedAddressChange,
        }),
      { wrapper: walletWrapper },
    );

    act(() => {
      result.current.actions.setAddressInput(OTHER_ADDRESS);
    });
    act(() => {
      result.current.actions.checkCurrentAddress();
    });

    await waitFor(() => {
      expect(result.current.ui.state.view.kind).toBe("loaded");
    });

    act(() => {
      result.current.actions.clear();
    });

    expect(result.current.ui.state.view).toEqual({ kind: "idle" });
    expect(result.current.ui.addressInput).toBe("");
    expect(result.current.ui.eligibilityModalState).toEqual({
      kind: "closed",
    });
    expect(onSelectedAddressChange).toHaveBeenCalledWith(OTHER_ADDRESS);
    expect(onSelectedAddressChange).toHaveBeenLastCalledWith(null);
    expect(onRequiredChainChange).toHaveBeenLastCalledWith(null);
  });
});
