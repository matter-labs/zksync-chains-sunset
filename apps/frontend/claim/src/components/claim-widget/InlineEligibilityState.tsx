import type { ReactNode } from "react";

import type { ActionKind, Address } from "../../lib/api";
import { shortenAddress } from "../../lib/validators";
import type { IdentityState } from "../app-shell/types";
import { EligibilityCards } from "./EligibilityCards";
import type {
  EligibilityModalState,
  TokenActionState,
  ViewState,
} from "./types";
import type { ClaimMode } from "./useClaimWidgetController";

type Props = {
  identity: IdentityState;
  state: EligibilityModalState;
  view: ViewState;
  actionStates: Record<string, TokenActionState>;
  actionBusy: boolean;
  connectWalletBusy: boolean;
  enabledActions: ActionKind[];
  allowBatchClaimAll?: boolean;
  walletConnected: boolean;
  claimMode: ClaimMode;
  loadingDescription: string;
  selectedAddressTitle: string;
  selectedAddressMessage: ReactNode;
  walletMessage: ReactNode;
  onConnectWallet: () => void;
  onRunAction: (
    action: ActionKind,
    tokenAddress: Address,
    mode?: ClaimMode,
  ) => void;
  onRunAllAction: (action: ActionKind, mode?: ClaimMode) => void;
};

function IdentitySummary({
  identity,
  selectedAddressTitle,
  selectedAddressMessage,
  walletMessage,
  connectWalletBusy,
  onConnectWallet,
}: Pick<
  Props,
  | "identity"
  | "selectedAddressTitle"
  | "selectedAddressMessage"
  | "walletMessage"
  | "connectWalletBusy"
  | "onConnectWallet"
>) {
  if (identity.source === "none" || identity.address === null) return null;

  if (identity.source === "wallet") {
    return (
      <div className="notice notice--identity" data-identity-mode="wallet">
        <div className="notice__title">Connected wallet</div>
        <div className="notice__message">
          {shortenAddress(identity.address)}. {walletMessage}
        </div>
      </div>
    );
  }

  return (
    <div
      className="notice notice--identity"
      data-identity-mode="selected-address"
    >
      <div className="notice__title">{selectedAddressTitle}</div>
      <div className="notice__message">
        {shortenAddress(identity.address)}. {selectedAddressMessage}
      </div>
      <button
        className="btn btn--secondary btn--sm notice__action"
        type="button"
        data-action="connect-wallet-from-selected-address"
        onClick={onConnectWallet}
        disabled={connectWalletBusy}
      >
        {connectWalletBusy ? "Connecting..." : "Connect Wallet"}
      </button>
    </div>
  );
}

export function InlineEligibilityState({
  identity,
  state,
  view,
  actionStates,
  actionBusy,
  connectWalletBusy,
  enabledActions,
  allowBatchClaimAll = false,
  walletConnected,
  claimMode,
  loadingDescription,
  selectedAddressTitle,
  selectedAddressMessage,
  walletMessage,
  onConnectWallet,
  onRunAction,
  onRunAllAction,
}: Props) {
  if (state.kind === "closed") return null;

  if (
    state.kind === "connecting_wallet" ||
    state.kind === "checking_eligibility"
  ) {
    return (
      <div className="widget__results" data-role="status-inline">
        <div className="notice notice--loading">
          <div className="notice__title">
            {state.kind === "connecting_wallet"
              ? "Connecting wallet"
              : "Checking eligibility"}
          </div>
          <div className="notice__message">
            {state.kind === "connecting_wallet"
              ? "Approve the connection request in your wallet."
              : loadingDescription}
          </div>
        </div>
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div className="widget__results" data-role="status-inline">
        <div className="notice notice--error">
          <div className="notice__title">{state.title}</div>
          <div className="notice__message">{state.message}</div>
          {state.title === "Wallet connection failed" ? (
            <button
              className="btn btn--secondary btn--sm notice__action"
              type="button"
              data-action="retry-connect-wallet"
              onClick={onConnectWallet}
              disabled={connectWalletBusy}
            >
              {connectWalletBusy ? "Connecting..." : "Retry"}
            </button>
          ) : null}
        </div>
      </div>
    );
  }

  if (state.kind === "results" && view.kind === "loaded") {
    return (
      <div className="widget__results" data-role="results-inline">
        <IdentitySummary
          identity={identity}
          selectedAddressTitle={selectedAddressTitle}
          selectedAddressMessage={selectedAddressMessage}
          walletMessage={walletMessage}
          connectWalletBusy={connectWalletBusy}
          onConnectWallet={onConnectWallet}
        />
        <div className="widget__results" data-role="results">
          <EligibilityCards
            tokens={view.tokens}
            actionStates={actionStates}
            actionBusy={actionBusy}
            enabledActions={enabledActions}
            allowBatchClaimAll={allowBatchClaimAll}
            walletConnected={walletConnected}
            claimMode={claimMode}
            onRunAction={onRunAction}
            onRunAllAction={onRunAllAction}
          />
        </div>
      </div>
    );
  }

  return null;
}
