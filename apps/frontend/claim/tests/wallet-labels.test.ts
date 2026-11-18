import { describe, expect, it } from "bun:test";

import { walletActionLabel } from "../../shared/src/ui/wallet-labels";

describe("walletActionLabel", () => {
  it("uses the connector-provided wallet name for discovered browser wallets", () => {
    expect(
      walletActionLabel({
        name: "Rabby Wallet",
      }),
    ).toBe("Rabby Wallet");

    expect(
      walletActionLabel({
        name: "Phantom",
      }),
    ).toBe("Phantom");
  });

  it("uses the connector-provided WalletConnect name", () => {
    expect(
      walletActionLabel({
        name: "WalletConnect",
      }),
    ).toBe("WalletConnect");
  });

  it("falls back to a generic browser wallet name when no connector name is available", () => {
    expect(walletActionLabel({ name: " " })).toBe("Browser Wallet");
  });
});
