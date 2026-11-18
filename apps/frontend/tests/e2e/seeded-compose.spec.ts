import { expect, test, type Locator, type Page } from "@playwright/test";

import {
  collectErrors,
  expectNoErrors,
  gotoHome,
  gotoMigrate,
  headerIdentitySelector,
  installRpcBackedWallet,
  txModalSelector,
  walletState,
} from "./support/sunset";

async function connectWallet(page: Page) {
  const identity = page.locator(headerIdentitySelector);
  const walletPicker = page.getByRole("dialog", { name: "Choose a wallet" });
  const walletError = page.getByText("Wallet connection failed");

  await page.getByRole("button", { name: "Connect Wallet" }).click();
  await expect
    .poll(
      async () => {
        if (await walletPicker.isVisible()) return "picker";
        if ((await identity.count()) > 0) return "connected";
        if ((await walletError.count()) > 0) return "error";
        return "pending";
      },
      { timeout: 15_000 },
    )
    .not.toBe("pending");

  if ((await walletError.count()) > 0) {
    const errorMessage = await page
      .locator(".notice--error .notice__message")
      .first()
      .textContent();
    throw new Error(errorMessage ?? "Wallet connection failed");
  }

  if (await walletPicker.isVisible()) {
    const metaMaskButton = page.getByRole("button", { name: /MetaMask/i });
    if ((await metaMaskButton.count()) > 0) {
      await metaMaskButton.click();
    } else {
      await page.getByRole("button", { name: /Browser/i }).click();
    }
  }

  await expect(identity).toContainText("Connected wallet");
}

async function ensureWalletConnected(page: Page) {
  const identity = page.locator(headerIdentitySelector);
  if ((await identity.count()) > 0) {
    await expect(identity).toContainText("Connected wallet");
    return;
  }

  await connectWallet(page);
}

async function expectSuccessfulTxModal(
  page: Page,
  expectedTitle: string,
  failureTitle: string,
) {
  const modalTitle = page.locator(`${txModalSelector} .claim-modal__title`);
  await expect
    .poll(
      async () => {
        const title = (await modalTitle.textContent())?.trim() ?? "";
        if (title.includes(expectedTitle)) return "success";
        if (title.includes(failureTitle)) return "failure";
        return title.length > 0 ? "pending" : "empty";
      },
      { timeout: 120_000 },
    )
    .not.toBe("pending");

  const title = (await modalTitle.textContent())?.trim() ?? "";
  if (title.includes(failureTitle)) {
    const message = await page.locator(".claim-modal__message").textContent();
    const details = await page.locator(".claim-modal__details").textContent();
    throw new Error(
      `${failureTitle}: ${message?.trim() ?? ""} ${details?.trim() ?? ""}`.trim(),
    );
  }

  await expect(modalTitle).toContainText(expectedTitle);
}

function tokenCard(page: Page, symbol: string): Locator {
  return page
    .locator(".card", {
      has: page.locator(".card__title", { hasText: symbol }),
    })
    .first();
}

async function pickMigratableToken(page: Page): Promise<string> {
  const migratableCards = page
    .locator(".card")
    .filter({ has: page.getByRole("button", { name: "Migrate token" }) });

  await expect(migratableCards.first()).toBeVisible({ timeout: 30_000 });

  for (const symbol of ["DAI", "USDC", "USDT", "ETH"]) {
    const candidate = tokenCard(page, symbol).filter({
      has: page.getByRole("button", { name: "Migrate token" }),
    });
    if ((await candidate.count()) > 0) {
      return symbol;
    }
  }

  const fallbackCard = migratableCards.first();
  const fallbackSymbol = (
    await fallbackCard.locator(".card__title").first().textContent()
  )?.trim();
  if (!fallbackSymbol) {
    throw new Error("No migratable token card was available");
  }
  return fallbackSymbol;
}

async function waitForClaimTokenReady(
  page: Page,
  symbol: string,
): Promise<Locator> {
  let lastError: unknown = null;

  for (let attempt = 0; attempt < 6; attempt += 1) {
    const card = tokenCard(page, symbol);
    try {
      await expect(
        card.getByRole("button", { name: "Withdraw", exact: true }),
      ).toBeVisible({
        timeout: 5_000,
      });
      return card;
    } catch (error) {
      lastError = error;
    }

    await page.waitForTimeout(1_000);
    await page.reload();
    await expect(
      page.getByRole("heading", { name: "Withdraw Your Funds" }),
    ).toBeVisible();
    await ensureWalletConnected(page);
  }

  throw lastError ?? new Error(`Claim token ${symbol} never became ready`);
}

test("compose-backed seeded flow migrates and sponsored-claims through the L1 contracts", async ({
  page,
}) => {
  test.setTimeout(90_000);
  const errors = collectErrors(page);
  await installRpcBackedWallet(page, { initialChainId: 1 });

  await gotoMigrate(page);
  await connectWallet(page);
  await page.getByRole("button", { name: "Show Tokens" }).click();

  const migratedSymbol = await pickMigratableToken(page);
  const migrateTokenCard = tokenCard(page, migratedSymbol);
  await expect(migrateTokenCard.getByText("Not migrated")).toBeVisible({
    timeout: 30_000,
  });
  await migrateTokenCard.getByRole("button", { name: "Migrate token" }).click();

  await expectSuccessfulTxModal(page, "Migration complete", "Migration failed");
  await page.getByRole("button", { name: "Close" }).click();
  await expect(migrateTokenCard.getByText("Migrated")).toBeVisible();

  const claimPage = await page.context().newPage();
  const claimErrors = collectErrors(claimPage);
  await gotoHome(claimPage, { mockConfig: false });
  await ensureWalletConnected(claimPage);

  const claimTokenCard = await waitForClaimTokenReady(
    claimPage,
    migratedSymbol,
  );
  await claimTokenCard.getByRole("button", { name: "Withdraw" }).click();

  await expectSuccessfulTxModal(
    claimPage,
    "Withdrawal complete",
    "Withdrawal failed",
  );
  await claimPage.getByRole("button", { name: "Close" }).click();
  await expect(claimTokenCard.getByText("Withdrawn")).toBeVisible();
  await expect(claimTokenCard.locator('[data-action="claim"]')).toHaveCount(0);

  await expect
    .poll(() => walletState(claimPage))
    .toMatchObject({
      activeChainId: 31338,
      addChainCount: 1,
      switchCount: 1,
      signTypedDataCount: 1,
      sendTransactionCount: 1,
    });

  await expectNoErrors(errors);
  await expectNoErrors(claimErrors);
});
