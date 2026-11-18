import { forwardRef, useImperativeHandle, useState } from "react";

import { WalletPickerModal } from "@shared/ui/WalletPickerModal";
import { useWalletSession } from "@shared/wallet/session";

import { InlineEligibilityState } from "./claim-widget/InlineEligibilityState";
import { TxModal } from "./claim-widget/TxModal";
import { useMigrationWidgetController } from "./claim-widget/useMigrationWidgetController";
import type { IdentityState } from "./app-shell/types";

type Props = {
  identity: IdentityState;
  onRequiredChainChange: (chainId: number | null) => void;
};

export type MigrationWidgetHandle = {
  reset: () => void;
};

const MigrationWidget = forwardRef<MigrationWidgetHandle, Props>(
  function MigrationWidget({ identity, onRequiredChainChange }: Props, ref) {
    const wallet = useWalletSession();
    const controller = useMigrationWidgetController({
      identity,
      onRequiredChainChange,
    });
    const [walletPickerOpen, setWalletPickerOpen] = useState(false);

    function handleConnectWallet() {
      if (wallet.availableWallets.length > 1) {
        setWalletPickerOpen(true);
        return;
      }

      controller.actions.connectWallet(wallet.availableWallets[0]?.id);
    }

    useImperativeHandle(
      ref,
      () => ({
        reset: controller.actions.resetWidget,
      }),
      [controller.actions.resetWidget],
    );

    return (
      <>
        <div className="widget" data-migration-widget data-bound="true">
          <div className="widget__row">
            {identity.source === "none" ? (
              <button
                className="btn btn--secondary widget__animated"
                type="button"
                data-action="connect-wallet"
                onClick={handleConnectWallet}
                disabled={controller.derived.controlsBusy}
              >
                {controller.derived.connectWalletBusy
                  ? "Connecting..."
                  : "Connect Wallet"}
              </button>
            ) : null}

            <button
              className="btn btn--primary widget__animated"
              type="button"
              data-action="show-tokens"
              onClick={controller.actions.showTokens}
              disabled={controller.derived.controlsBusy}
            >
              {controller.derived.showTokensBusy ? "Loading..." : "Show Tokens"}
            </button>
          </div>

          {controller.ui.state.modal.kind === "closed" ? (
            <InlineEligibilityState
              identity={identity}
              state={controller.ui.eligibilityModalState}
              view={controller.ui.state.view}
              actionStates={controller.ui.state.actionStates}
              actionBusy={controller.derived.actionBusy}
              enabledActions={controller.derived.enabledActions}
              walletConnected={wallet.status === "connected"}
              loadingTitle="Loading tokens"
              loadingDescription="Loading migration totals and onchain migration status for every token."
              selectedAddressTitle="Selected address"
              selectedAddressMessage="Connect a wallet before submitting a migration transaction."
              walletMessage="This wallet can pay gas and submit migration transactions for the tokens shown below."
              onRunAction={(_, tokenAddress) => {
                controller.actions.migrateToken(tokenAddress);
              }}
              onRunAllAction={() => {}}
            />
          ) : null}
          {controller.ui.state.modal.kind === "closed" &&
          controller.ui.state.view.kind === "loaded" &&
          identity.source !== "wallet" ? (
            <div className="widget__results" data-role="migration-wallet-hint">
              <div className="notice">
                <div className="notice__title">Connect wallet to migrate</div>
                <div className="notice__message">
                  Token listing is permissionless. Connect a wallet when you are
                  ready to pay gas and submit the migration transaction.
                </div>
              </div>
            </div>
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
  },
);

export default MigrationWidget;
