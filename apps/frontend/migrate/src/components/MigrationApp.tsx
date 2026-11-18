import { useRef, useState } from "react";

import { WalletReactProvider } from "@shared/wallet/WalletReactProvider";
import {
  WalletSessionProvider,
  useWalletSession,
} from "@shared/wallet/session";

import { claimAppUrl } from "../lib/app-links";
import { AppShell } from "./app-shell/AppShell";
import { emptyIdentityState } from "./app-shell/types";
import MigrationWidget, { type MigrationWidgetHandle } from "./MigrationWidget";

type MigrationAppProps = {
  claimAppHref?: string;
};

function MigrationAppContent({ claimAppHref }: MigrationAppProps) {
  const wallet = useWalletSession();
  const [, setRequiredChainId] = useState<number | null>(null);
  const widgetRef = useRef<MigrationWidgetHandle | null>(null);

  const identity =
    wallet.address !== null
      ? { source: "wallet" as const, address: wallet.address }
      : emptyIdentityState;

  return (
    <AppShell
      identity={identity}
      title="Migrate Token"
      eyebrow="Token-level migration flow."
      panelCopy="Show the full migration token list, see whether each token has already been migrated, and submit the migration transaction for any token that is still pending. If you only need to recover balances, go back and use withdraw."
      footerNote="Looking for the withdraw flow instead?"
      footerHref={claimAppHref ?? claimAppUrl()}
      footerLabel="Back to withdraw"
      onClearIdentity={() => {
        setRequiredChainId(null);
        void wallet.forgetConnection();
        widgetRef.current?.reset();
      }}
    >
      <MigrationWidget
        ref={widgetRef}
        identity={identity}
        onRequiredChainChange={setRequiredChainId}
      />
    </AppShell>
  );
}

export default function MigrationApp(props: MigrationAppProps) {
  return (
    <WalletReactProvider>
      <WalletSessionProvider>
        <MigrationAppContent {...props} />
      </WalletSessionProvider>
    </WalletReactProvider>
  );
}
