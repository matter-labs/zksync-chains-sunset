import { expect, test } from "@playwright/test";

import {
  USDC,
  DAI,
  ETH,
  SECONDARY,
  VITALIK,
  WETH,
  braveWalletConnectCount,
  clipboardText,
  collectErrors,
  emitAccountsChanged,
  emitDisconnect,
  expectNoErrors,
  fillAndCheck,
  gotoHome,
  headerIdentitySelector,
  installClipboardMock,
  installMultiWalletMock,
  installWalletMock,
  metaMaskConnectCount,
  mockChainRpc,
  mockClaimFlow,
  requestAccountsCount,
  sendTransactionCount,
  signTypedDataCount,
} from "./support/sunset";

test("eligibility flow does not error and renders dummy tokens", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installClipboardMock(page);

  await mockChainRpc(page, { migratedTokens: [USDC] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: USDC,
            symbol: "USDC",
            displayBalance: "1.00",
            rawBalance: "1000000",
            decimals: 6,
          },
          {
            claimIndex: 1,
            tokenAddress: WETH,
            symbol: "WETH",
            displayBalance: "0.75",
            rawBalance: "750000000000000000",
            decimals: 18,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await expect(
    page.getByRole("heading", { name: "Withdraw Your Funds" }),
  ).toBeVisible();

  await fillAndCheck(page);

  await expect
    .poll(async () => page.locator(".card").count(), { timeout: 30_000 })
    .toBeGreaterThan(0);
  await expect(page.locator('[data-action="claim"]')).toHaveCount(0);
  await expect(page.locator('[data-action="claim-all"]')).toHaveCount(0);
  await expect(page.locator('[data-action="migrate"]')).toHaveCount(0);
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Selected address",
  );
  await expect(
    page.getByText(`${USDC.slice(0, 6)}...${USDC.slice(-4)}`),
  ).toBeVisible();
  await page
    .locator(`[data-action="copy-token-address"][data-token-address="${USDC}"]`)
    .click();
  await expect.poll(async () => clipboardText(page)).toBe(USDC);

  await expectNoErrors(errors);
});

test("eligibility response without migrated balances exposes backend-supported actions", async ({
  page,
}) => {
  const errors = collectErrors(page);

  await mockChainRpc(page);
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: USDC,
            symbol: "USDC",
            displayBalance: "1.00",
            rawBalance: "1000000",
            decimals: 6,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await fillAndCheck(page);

  await expect
    .poll(async () => page.locator('[data-action="migrate"]').count(), {
      timeout: 30_000,
    })
    .toBe(0);
  await expect(page.locator('[data-role="results-inline"]')).toBeVisible();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Selected address",
  );

  await expectNoErrors(errors);
});

test("selected-address results expose a connect wallet path", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page);

  await mockChainRpc(page, { migratedTokens: [USDC] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: USDC,
            symbol: "USDC",
            displayBalance: "1.00",
            rawBalance: "1000000",
            decimals: 6,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await fillAndCheck(page);

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Selected address",
  );
  await page
    .locator('[data-action="connect-wallet-from-selected-address"]')
    .click();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect(page.locator('[data-role="results-inline"]')).toBeVisible();
  await expect(
    page.locator('[data-action="connect-wallet-from-selected-address"]'),
  ).toHaveCount(0);
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);

  await expectNoErrors(errors);
});

test("unavailable claim mode hides withdraw actions", async ({ page }) => {
  const errors = collectErrors(page);

  await gotoHome(page, { claimMode: "unavailable" });

  await expect(
    page.locator('[data-claim-unavailable-reason="product_disabled"]'),
  ).toBeVisible();
  await expect(page.locator("[data-claim-widget]")).toHaveCount(0);
  await expect(page.locator('[data-action="claim"]')).toHaveCount(0);
  await expect(page.locator('[data-action="claim-all"]')).toHaveCount(0);

  await expectNoErrors(errors);
});

test("unhealthy api readiness hides withdraw actions", async ({ page }) => {
  await page.route("**/readyz", async (route) => {
    await route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({ status: "error" }),
    });
  });

  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Withdraw Your Funds" }),
  ).toBeVisible();
  await expect(
    page.locator('[data-claim-unavailable-reason="api_unhealthy"]'),
  ).toBeVisible();
  await expect(page.locator("[data-claim-widget]")).toHaveCount(0);
  await expect(page.locator('[data-action="claim"]')).toHaveCount(0);
  await expect(page.locator('[data-action="claim-all"]')).toHaveCount(0);
});

test("claim all signs and submits the batch claim flow", async ({ page }) => {
  const errors = collectErrors(page);
  await installWalletMock(page, { initialChainId: 11155111 });

  let prepareRequest: unknown | null = null;
  let submitRequest: unknown | null = null;
  let statusRequestId: string | null = null;

  await mockChainRpc(page, { migratedTokens: [DAI, ETH] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: DAI,
            symbol: "DAI",
            displayBalance: "4.00",
            rawBalance: "4000000000000000000",
            decimals: 18,
          },
          {
            claimIndex: 1,
            tokenAddress: ETH,
            symbol: "ETH",
            displayBalance: "0.10",
            rawBalance: "100000000000000000",
            decimals: 18,
          },
        ],
      }),
    });
  });

  await mockClaimFlow(page, {
    claimId: "claim_batch",
    tokenAddresses: [DAI, ETH],
    onPrepareRequest: (body) => {
      prepareRequest = body;
    },
    onSubmitRequest: (body) => {
      submitRequest = body;
    },
    onStatusRequest: (claimId) => {
      statusRequestId = claimId;
    },
  });

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await page.getByRole("button", { name: "Withdraw all" }).click();

  await expect(
    page.locator(".claim-modal:not(.eligibility-modal) .claim-modal__title"),
  ).toContainText("Withdrawal complete");
  expect(prepareRequest).toEqual({
    address: VITALIK,
    allEligible: true,
  });
  expect(submitRequest).toMatchObject({
    address: VITALIK,
    items: [
      expect.objectContaining({ tokenAddress: DAI }),
      expect.objectContaining({ tokenAddress: ETH }),
    ],
    signature: expect.any(String),
  });
  expect(statusRequestId).toBe("claim_batch");
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);
  await expect.poll(async () => signTypedDataCount(page)).toBe(1);
  await expect.poll(async () => sendTransactionCount(page)).toBe(0);

  await expectNoErrors(errors);
});

test("paid config submits withdraw all through the wallet transaction path", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page, { initialChainId: 31338 });

  let prepareRequest: unknown | null = null;
  let submitRequest: unknown | null = null;

  await mockChainRpc(page, { migratedTokens: [DAI, ETH] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: DAI,
            symbol: "DAI",
            displayBalance: "4.00",
            rawBalance: "4000000000000000000",
            decimals: 18,
          },
          {
            claimIndex: 1,
            tokenAddress: ETH,
            symbol: "ETH",
            displayBalance: "0.10",
            rawBalance: "100000000000000000",
            decimals: 18,
          },
        ],
      }),
    });
  });

  await mockClaimFlow(page, {
    claimId: "claim_direct_batch",
    tokenAddresses: [DAI, ETH],
    onPrepareRequest: (body) => {
      prepareRequest = body;
    },
    onSubmitRequest: (body) => {
      submitRequest = body;
    },
  });

  await gotoHome(page, { claimMode: "paid" });
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect(
    page.locator('[data-action="claim-all"][data-claim-mode="paid"]'),
  ).toBeVisible();
  await expect(
    page.locator('[data-action="claim-all"][data-claim-mode="sponsored"]'),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "Withdraw all" }).click();

  await expect(
    page.locator(".claim-modal:not(.eligibility-modal) .claim-modal__title"),
  ).toContainText("Withdrawal complete");
  expect(prepareRequest).toEqual({
    address: VITALIK,
    allEligible: true,
  });
  expect(submitRequest).toMatchObject({
    claimId: "claim_direct_batch",
    txHash: expect.any(String),
  });
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);
  await expect.poll(async () => signTypedDataCount(page)).toBe(0);
  await expect.poll(async () => sendTransactionCount(page)).toBe(1);

  await expectNoErrors(errors);
});

test("invalid address shows a friendly inline error", async ({ page }) => {
  const errors = collectErrors(page);

  await gotoHome(page);
  await fillAndCheck(page, "not-an-address");

  await expect(page.locator('[data-role="status-inline"]')).toBeVisible();
  await expect(page.locator(".notice--error .notice__message")).toContainText(
    "Invalid",
  );

  await expectNoErrors(errors);
});

test("connect wallet shows a friendly inline error when no wallet is injected", async ({
  page,
}) => {
  const errors = collectErrors(page);

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();

  await expect(page.locator('[data-role="status-inline"]')).toBeVisible();
  await expect(page.locator(".notice--error .notice__title")).toContainText(
    "Wallet connection failed",
  );
  await expect(page.locator(".notice--error .notice__message")).toContainText(
    "No injected wallet found",
  );
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();

  await page.getByRole("button", { name: "Retry" }).click();

  await expect(page.locator(".notice--error .notice__title")).toContainText(
    "Wallet connection failed",
  );
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();

  await expectNoErrors(errors);
});

test("connect wallet success shows header identity and disconnect resets the view", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page);

  await mockChainRpc(page, { migratedTokens: [USDC] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: USDC,
            symbol: "USDC",
            displayBalance: "1.00",
            rawBalance: "1000000",
            decimals: 6,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();

  await expect(page.locator(headerIdentitySelector)).toBeVisible();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect(
    page.getByRole("button", { name: "Connect Wallet" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Check Remaining Funds" }),
  ).toHaveCount(0);
  await expect(page.getByLabel("Ethereum address")).toHaveCount(0);
  await expect(page.locator('[data-action="clear"]')).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible();
  await expect(page.locator('[data-role="results-inline"]')).toBeVisible();

  await page.getByRole("button", { name: "Disconnect" }).click();

  await expect(page.locator(headerIdentitySelector)).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Connect Wallet" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Check Remaining Funds" }),
  ).toBeVisible();
  await expect(page.getByLabel("Ethereum address")).toBeVisible();
  await expect(page.locator('[data-role="results-inline"]')).toHaveCount(0);

  await expectNoErrors(errors);
});

test("connect wallet shows a chooser for multiple injected wallets on every reconnect", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installMultiWalletMock(page);

  await mockChainRpc(page, { migratedTokens: [USDC] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: USDC,
            symbol: "USDC",
            displayBalance: "1.00",
            rawBalance: "1000000",
            decimals: 6,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();

  await expect(
    page.getByRole("dialog", { name: "Choose a wallet" }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: /MetaMask/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /Brave/i })).toBeVisible();

  await page.getByRole("button", { name: /MetaMask/i }).click();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect.poll(async () => metaMaskConnectCount(page)).toBe(1);
  await expect.poll(async () => braveWalletConnectCount(page)).toBe(0);

  await page.getByRole("button", { name: "Disconnect" }).click();

  await expect(
    page.getByRole("button", { name: "Connect Wallet" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(
    page.getByRole("dialog", { name: "Choose a wallet" }),
  ).toBeVisible();
  await page.getByRole("button", { name: /MetaMask/i }).click();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect.poll(async () => metaMaskConnectCount(page)).toBe(2);
  await expect.poll(async () => braveWalletConnectCount(page)).toBe(0);

  await expectNoErrors(errors);
});

test("connected wallet reconnects on reload and restores claim results", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page);

  await mockChainRpc(page, { migratedTokens: [DAI] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: DAI,
            symbol: "DAI",
            displayBalance: "2.00",
            rawBalance: "2000000000000000000",
            decimals: 18,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect(page.locator('[data-role="results-inline"]')).toBeVisible();
  await expect(page.getByText("DAI")).toBeVisible();

  await page.reload();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect(page.locator('[data-role="results-inline"]')).toBeVisible();
  await expect(page.getByText("DAI")).toBeVisible();
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);

  await expectNoErrors(errors);
});

test("claim view reacts to accountsChanged and refreshes wallet-bound results", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page);

  await mockChainRpc(page, { migratedTokens: [USDC, DAI] });
  await page.route("**/api/eligibility", async (route) => {
    const body = route.request().postDataJSON() as { address: string };
    const isSecondary = body.address.toLowerCase() === SECONDARY.toLowerCase();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: isSecondary ? SECONDARY : VITALIK,
        tokens: [
          {
            claimIndex: isSecondary ? 1 : 0,
            tokenAddress: isSecondary ? DAI : USDC,
            symbol: isSecondary ? "DAI" : "USDC",
            displayBalance: "1.00",
            rawBalance: isSecondary ? "1000000000000000000" : "1000000",
            decimals: isSecondary ? 18 : 6,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(page.getByText("USDC")).toBeVisible();

  await emitAccountsChanged(page, [SECONDARY]);

  await expect(page.locator(headerIdentitySelector)).toContainText(
    SECONDARY.slice(0, 6),
  );
  await expect(page.getByText("DAI")).toBeVisible();
  await expect(page.getByText("USDC")).toHaveCount(0);

  await expectNoErrors(errors);
});

test("claim view reacts to wallet disconnects", async ({ page }) => {
  const errors = collectErrors(page);
  await installWalletMock(page);

  await mockChainRpc(page, { migratedTokens: [USDC] });
  await page.route("**/api/eligibility", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        address: VITALIK,
        tokens: [
          {
            claimIndex: 0,
            tokenAddress: USDC,
            symbol: "USDC",
            displayBalance: "1.00",
            rawBalance: "1000000",
            decimals: 6,
          },
        ],
      }),
    });
  });

  await gotoHome(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );

  await emitDisconnect(page);

  await expect(page.locator(headerIdentitySelector)).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Connect Wallet" }),
  ).toBeVisible();

  await expectNoErrors(errors);
});
