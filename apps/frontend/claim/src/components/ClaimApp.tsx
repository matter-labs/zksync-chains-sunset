import { useEffect, useState } from "react";

import { WalletReactProvider } from "@shared/wallet/WalletReactProvider";
import {
  WalletSessionProvider,
  useWalletSession,
} from "@shared/wallet/session";

import {
  getFrontendConfig,
  type Address,
  type ClaimFrontendMode,
} from "../lib/api";
import { migrateAppUrl } from "../lib/app-links";
import {
  type ApiAvailability,
  useApiAvailability,
} from "../lib/useApiAvailability";
import { AppShell } from "./app-shell/AppShell";
import { emptyIdentityState } from "./app-shell/types";
import ClaimWidget from "./ClaimWidget";

type ClaimAppProps = {
  migrateAppHref?: string;
};

type ClaimUnavailableReason =
  | "api_unhealthy"
  | "config_failed"
  | "product_disabled";

type ClaimConfigState =
  | { kind: "loading" }
  | { kind: "ready"; mode: ClaimFrontendMode }
  | { kind: "error" };

type ClaimUiState =
  | { kind: "checking" }
  | { kind: "unavailable"; reason: ClaimUnavailableReason }
  | { kind: "sponsored" }
  | { kind: "paid" };

function resolveClaimUiState(
  apiAvailability: ApiAvailability,
  claimConfig: ClaimConfigState,
): ClaimUiState {
  if (apiAvailability === "checking") return { kind: "checking" };
  if (apiAvailability === "unavailable") {
    return { kind: "unavailable", reason: "api_unhealthy" };
  }
  if (claimConfig.kind === "loading") return { kind: "checking" };
  if (claimConfig.kind === "error") {
    return { kind: "unavailable", reason: "config_failed" };
  }
  if (claimConfig.mode === "unavailable") {
    return { kind: "unavailable", reason: "product_disabled" };
  }
  return { kind: claimConfig.mode };
}

function ClaimingAvailabilityBanner() {
  return (
    <div className="claiming-disabled" role="status">
      <p className="claiming-disabled__title">
        Checking withdrawal availability
      </p>
      <p className="claiming-disabled__message">
        The withdraw form will load when the service is ready.
      </p>
    </div>
  );
}

function ClaimingDisabledBanner({
  reason,
}: {
  reason: ClaimUnavailableReason;
}) {
  return (
    <div
      className="claiming-disabled"
      data-claim-unavailable-reason={reason}
      role="alert"
    >
      <p className="claiming-disabled__title">
        Withdrawals will be enabled soon.
      </p>
      <p className="claiming-disabled__message">
        Please check the{" "}
        <a
          className="claiming-disabled__link"
          href="https://www.zksync.io/blog/zksync-lite-sunset"
          target="_blank"
          rel="noreferrer"
        >
          blog post
        </a>{" "}
        and{" "}
        <a
          className="claiming-disabled__link"
          href="https://x.com/zksync"
          target="_blank"
          rel="noreferrer"
        >
          X
        </a>{" "}
        for more information.
      </p>
    </div>
  );
}

function ClaimAppContent({ migrateAppHref }: ClaimAppProps) {
  const wallet = useWalletSession();
  const apiAvailability = useApiAvailability();
  const [claimConfig, setClaimConfig] = useState<ClaimConfigState>({
    kind: "loading",
  });
  const [selectedAddress, setSelectedAddress] = useState<Address | null>(null);
  const [clearIdentityVersion, setClearIdentityVersion] = useState(0);
  const [, setRequiredChainId] = useState<number | null>(null);

  useEffect(() => {
    if (apiAvailability !== "available") {
      setClaimConfig({ kind: "loading" });
      return;
    }

    const abortController = new AbortController();
    setClaimConfig({ kind: "loading" });

    void getFrontendConfig(abortController.signal)
      .then((config) => {
        setClaimConfig({ kind: "ready", mode: config.claimMode });
      })
      .catch((err) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
        setClaimConfig({ kind: "error" });
      });

    return () => {
      abortController.abort();
    };
  }, [apiAvailability]);

  const identity =
    wallet.address !== null
      ? { source: "wallet" as const, address: wallet.address }
      : selectedAddress !== null
        ? { source: "selected_address" as const, address: selectedAddress }
        : emptyIdentityState;

  const claimUiState = resolveClaimUiState(apiAvailability, claimConfig);
  const claimingDisabled = claimUiState.kind === "unavailable";
  const checkingAvailability = claimUiState.kind === "checking";

  return (
    <AppShell
      identity={identity}
      title="Withdraw Your Funds"
      eyebrow={
        claimingDisabled ? (
          <>
            Withdrawals will be available soon. See{" "}
            <a
              className="pill__link"
              href="https://www.zksync.io/blog/zksync-lite-sunset"
              target="_blank"
              rel="noreferrer"
            >
              Blog Post
            </a>
            .
          </>
        ) : (
          <>
            ZKsync Lite has been sunsetted. See{" "}
            <a
              className="pill__link"
              href="https://www.zksync.io/blog/zksync-lite-sunset"
              target="_blank"
              rel="noreferrer"
            >
              Blog Post
            </a>
            .
          </>
        )
      }
      panelCopy="Check remaining Lite funds by connecting your wallet or entering your address below."
      footerNote="Need to migrate a specific token instead?"
      footerHref={migrateAppHref ?? migrateAppUrl()}
      footerLabel="Migrate Token"
      disabledBanner={
        checkingAvailability ? (
          <ClaimingAvailabilityBanner />
        ) : claimingDisabled ? (
          <ClaimingDisabledBanner reason={claimUiState.reason} />
        ) : undefined
      }
      pillVariant={claimingDisabled ? "warning" : "default"}
      onClearIdentity={() => {
        setRequiredChainId(null);
        setSelectedAddress(null);
        if (identity.source === "wallet") {
          void wallet.forgetConnection();
        }
        setClearIdentityVersion((value) => value + 1);
      }}
    >
      {claimUiState.kind === "sponsored" || claimUiState.kind === "paid" ? (
        <ClaimWidget
          key={clearIdentityVersion}
          identity={identity}
          claimMode={claimUiState.kind}
          onRequiredChainChange={setRequiredChainId}
          onSelectedAddressChange={setSelectedAddress}
        />
      ) : null}
    </AppShell>
  );
}

export default function ClaimApp(props: ClaimAppProps) {
  return (
    <WalletReactProvider>
      <WalletSessionProvider>
        <ClaimAppContent {...props} />
      </WalletSessionProvider>
    </WalletReactProvider>
  );
}
