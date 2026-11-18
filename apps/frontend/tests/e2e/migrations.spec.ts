import { expect, test } from "@playwright/test";

import {
  addChainCount,
  DAI,
  USDC,
  VITALIK,
  clipboardText,
  collectErrors,
  expectNoErrors,
  gotoMigrate,
  headerIdentitySelector,
  installClipboardMock,
  installWalletMock,
  mockMigrationFlow,
  requestAccountsCount,
  sendTransactionCount,
  signTypedDataCount,
  showMigrationTokens,
  switchChainCount,
  txModalSelector,
  walletState,
} from "./support/sunset";

test("migrate token page completes wallet-paid migration flow", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page, {
    initialChainId: 1,
    knownChainIds: [1],
  });

  let tokensRequestCount = 0;
  let prepareRequest: unknown | null = null;
  let submitRequest: unknown | null = null;
  let statusRequestId: string | null = null;
  await mockMigrationFlow(page, {
    migrationId: "mig_switch",
    tokenAddress: DAI,
    onTokensRequest: () => {
      tokensRequestCount += 1;
    },
    onPrepareRequest: (body) => {
      prepareRequest = body;
    },
    onSubmitRequest: (body) => {
      submitRequest = body;
    },
    onStatusRequest: (migrationId) => {
      statusRequestId = migrationId;
    },
  });

  await gotoMigrate(page);
  await showMigrationTokens(page);
  expect(tokensRequestCount).toBe(1);
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await page.getByRole("button", { name: "Migrate token" }).click();

  await expect(
    page.locator(`${txModalSelector} .claim-modal__title`),
  ).toContainText("Migration complete");
  expect(prepareRequest).toEqual({
    address: VITALIK,
    tokenAddress: DAI,
  });
  expect(submitRequest).toEqual({
    migrationId: "mig_switch",
    txHash: `0x${"2".repeat(64)}`,
  });
  expect(statusRequestId).toBe("mig_switch");
  await expect.poll(async () => sendTransactionCount(page)).toBe(1);
  await expect.poll(async () => signTypedDataCount(page)).toBe(0);
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);
  await expect.poll(async () => addChainCount(page)).toBe(1);
  await expect.poll(async () => switchChainCount(page)).toBe(1);

  await expectNoErrors(errors);
});

test("connect wallet switches to the configured local seeded fork", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page, {
    initialChainId: 11155111,
    knownChainIds: [11155111],
  });

  await gotoMigrate(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect.poll(async () => addChainCount(page)).toBe(1);
  await expect.poll(async () => switchChainCount(page)).toBe(1);
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          JSON.parse(
            window.localStorage.getItem("__wallet_mock_state__") ?? "{}",
          ).chainId ?? null,
      ),
    )
    .toBe(31338);

  await expectNoErrors(errors);
});

test("migrate token page shows all tokens in the list view", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installClipboardMock(page);

  await mockMigrationFlow(page, {
    tokens: [DAI, USDC],
  });

  await gotoMigrate(page);
  await showMigrationTokens(page);

  await expect(page.getByText(DAI)).toBeVisible();
  await expect(page.getByText(USDC)).toBeVisible();
  await page
    .locator(`[data-action="copy-token-address"][data-token-address="${DAI}"]`)
    .click();
  await expect.poll(async () => clipboardText(page)).toBe(DAI);

  await expectNoErrors(errors);
});

test("migrate token page uses wallet transaction instead of typed-data signature", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page, { initialChainId: 1 });
  let submitRequest: unknown | null = null;

  await mockMigrationFlow(page, {
    migrationId: "mig_wallet",
    tokenAddress: DAI,
    onSubmitRequest: (body) => {
      submitRequest = body;
    },
  });

  await gotoMigrate(page);
  await showMigrationTokens(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible();
  await page.getByRole("button", { name: "Migrate token" }).click();

  const modal = page.locator(txModalSelector);
  await expect(modal.locator(".claim-modal__title")).toContainText(
    "Migration complete",
  );
  await expect.poll(async () => signTypedDataCount(page)).toBe(0);
  await expect.poll(async () => sendTransactionCount(page)).toBe(1);
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);
  expect(submitRequest).toEqual({
    migrationId: "mig_wallet",
    txHash: `0x${"2".repeat(64)}`,
  });

  await expectNoErrors(errors);
});

test("migrate token page tolerates a pending switch request after adding the local chain", async ({
  page,
}) => {
  const errors = collectErrors(page);
  await installWalletMock(page, {
    initialChainId: 1,
    knownChainIds: [1],
    pendingSwitchCount: 1,
  });

  await mockMigrationFlow(page, {
    migrationId: "mig_pending_switch",
    tokenAddress: DAI,
  });

  await gotoMigrate(page);
  await showMigrationTokens(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await page.getByRole("button", { name: "Migrate token" }).click();

  await expect(
    page.locator(`${txModalSelector} .claim-modal__title`),
  ).toContainText("Migration complete");
  await expect.poll(async () => addChainCount(page)).toBe(1);
  await expect.poll(async () => switchChainCount(page)).toBe(1);

  await expectNoErrors(errors);
});

test("migrate page reconnects wallet on reload", async ({ page }) => {
  const errors = collectErrors(page);
  await installWalletMock(page);

  await gotoMigrate(page);
  await page.getByRole("button", { name: "Connect Wallet" }).click();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );

  await page.reload();

  await expect(page.locator(headerIdentitySelector)).toContainText(
    "Connected wallet",
  );
  await expect.poll(async () => requestAccountsCount(page)).toBe(1);

  await expectNoErrors(errors);
});
