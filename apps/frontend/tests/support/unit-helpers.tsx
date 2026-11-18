import type { PropsWithChildren } from "react";

import { WalletReactProvider } from "../../shared/src/wallet/WalletReactProvider";
import { WalletSessionProvider } from "../../shared/src/wallet/session";

export const TEST_ADDRESS =
  "0x1111111111111111111111111111111111111111" as const;
export const OTHER_ADDRESS =
  "0x2222222222222222222222222222222222222222" as const;

export const TEST_TOKEN = {
  claimIndex: 0,
  tokenAddress: TEST_ADDRESS,
  symbol: "TKN",
  displayBalance: "1.00",
  rawBalance: "1000000000000000000",
  decimals: 18,
  migrated: false,
  claimed: false,
};

export const TEST_MIGRATION_TOKEN = {
  tokenAddress: TEST_ADDRESS,
  symbol: "TKN",
  decimals: 18,
};

export function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
    },
  });
}

export function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

export function walletWrapper({ children }: PropsWithChildren) {
  return (
    <WalletReactProvider>
      <WalletSessionProvider>{children}</WalletSessionProvider>
    </WalletReactProvider>
  );
}
