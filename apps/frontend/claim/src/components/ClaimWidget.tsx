import { useState } from "react";

import { WalletPickerModal } from "@shared/ui/WalletPickerModal";
import { useWalletSession } from "@shared/wallet/session";

import type { Address } from "../lib/api";
import { InlineEligibilityState } from "./claim-widget/InlineEligibilityState";
import { TxModal } from "./claim-widget/TxModal";
import {
  type ClaimMode,
  useClaimWidgetController,
} from "./claim-widget/useClaimWidgetController";
import { WidgetControls } from "./claim-widget/WidgetControls";
import type { IdentityState } from "./app-shell/types";

type Props = {
  identity: IdentityState;
  claimMode: ClaimMode;
  onRequiredChainChange: (chainId: number | null) => void;
  onSelectedAddressChange: (address: Address | null) => void;
};

export default function ClaimWidget({
  identity,
  claimMode,
  onRequiredChainChange,
  onSelectedAddressChange,
}: Props) {
  const wallet = useWalletSession();
  const controller = useClaimWidgetController({
    identity,
    onRequiredChainChange,
    onSelectedAddressChange,
  });
  const [walletPickerOpen, setWalletPickerOpen] = useState(false);

  function handleConnectWallet() {
    if (!controller.actions.prepareWalletConnect()) return;

    if (wallet.availableWallets.length > 1) {
      setWalletPickerOpen(true);
      return;
    }

    controller.actions.connectWallet(wallet.availableWallets[0]?.id);
  }

  return (
    <>
      <div className="widget" data-claim-widget data-bound="true">
        <WidgetControls
          addressInput={controller.ui.addressInput}
          identity={identity}
          showCheckButton={controller.derived.showCheckButton}
          showConnectButton={controller.derived.showConnectButton}
          actionBusy={controller.derived.actionBusy}
          checkBusy={controller.derived.checkBusy}
          connectWalletBusy={controller.derived.connectWalletBusy}
          addressInputRef={controller.ui.addressInputRef}
          onAddressChange={controller.actions.setAddressInput}
          onCheck={controller.actions.checkCurrentAddress}
          onPaste={controller.actions.pasteAddress}
          onClear={controller.actions.clear}
          onConnectWallet={handleConnectWallet}
        />

        {controller.ui.state.modal.kind === "closed" ? (
          <InlineEligibilityState
            identity={identity}
            state={controller.ui.eligibilityModalState}
            view={controller.ui.state.view}
            actionStates={controller.ui.state.actionStates}
            actionBusy={controller.derived.actionBusy}
            connectWalletBusy={controller.derived.connectWalletBusy}
            enabledActions={["claim"]}
            allowBatchClaimAll
            walletConnected={wallet.status === "connected"}
            claimMode={claimMode}
            loadingDescription="Fetching token balances and migration status."
            selectedAddressTitle="Selected address"
            selectedAddressMessage="Connect this wallet to withdraw the funds shown below."
            walletMessage="This wallet can sign and submit withdrawals for the balances shown below."
            onConnectWallet={handleConnectWallet}
            onRunAction={controller.actions.runSingleAction}
            onRunAllAction={controller.actions.runAllAction}
          />
        ) : null}
      </div>

      <TxModal
        state={controller.ui.state.modal}
        onRequestClose={controller.actions.requestModalClose}
      />

      <WalletPickerModal
        isOpen={walletPickerOpen}
        wallets={wallet.availableWallets}
        onRequestClose={() => {
          setWalletPickerOpen(false);
        }}
        onSelectWallet={(walletId) => {
          setWalletPickerOpen(false);
          controller.actions.connectWallet(walletId);
        }}
      />
    </>
  );
}
