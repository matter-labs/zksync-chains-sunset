import { describe, expect, it, mock } from "bun:test";
import { render } from "@testing-library/react";

import { TxModal } from "../src/components/claim-widget/TxModal";
import { TEST_TOKEN } from "../../tests/support/unit-helpers";

const TX_HASH = `0x${"c".repeat(64)}` as const;

function explorerLink(): HTMLAnchorElement | null {
  return document.body.querySelector<HTMLAnchorElement>(
    ".claim-modal__explorer",
  );
}

function renderWithdrawSuccess(chainId: number) {
  render(
    <TxModal
      state={{
        kind: "claim_success",
        tokens: [TEST_TOKEN],
        txHash: TX_HASH,
        chainId,
        receipt: {
          blockNumber: 123,
          blockHash: `0x${"d".repeat(64)}`,
        },
      }}
      onRequestClose={mock(() => {})}
    />,
  );
}

describe("TxModal", () => {
  it("links mainnet withdraw success transactions to Etherscan", () => {
    renderWithdrawSuccess(1);

    const link = explorerLink();
    expect(link?.textContent).toBe("View withdrawal transaction");
    expect(link?.getAttribute("href")).toBe(
      `https://etherscan.io/tx/${TX_HASH}`,
    );
  });

  it("links Sepolia withdraw success transactions to Sepolia Etherscan", () => {
    renderWithdrawSuccess(11155111);

    const link = explorerLink();
    expect(link?.textContent).toBe("View withdrawal transaction");
    expect(link?.getAttribute("href")).toBe(
      `https://sepolia.etherscan.io/tx/${TX_HASH}`,
    );
  });

  it("falls back to mainnet Etherscan for unsupported chains", () => {
    renderWithdrawSuccess(31338);

    const link = explorerLink();
    expect(link?.textContent).toBe("View withdrawal transaction");
    expect(link?.getAttribute("href")).toBe(
      `https://etherscan.io/tx/${TX_HASH}`,
    );
  });
});
