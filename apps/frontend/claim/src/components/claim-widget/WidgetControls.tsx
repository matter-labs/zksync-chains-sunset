import type { RefObject } from "react";

import { ActionButton } from "@shared/ui/ActionButton";

import type { IdentityState } from "../app-shell/types";

type Props = {
  addressInput: string;
  identity: IdentityState;
  showCheckButton: boolean;
  showConnectButton: boolean;
  actionBusy: boolean;
  checkBusy: boolean;
  connectWalletBusy: boolean;
  addressInputRef: RefObject<HTMLInputElement | null>;
  onAddressChange: (value: string) => void;
  onCheck: () => void;
  onPaste: () => void;
  onClear: () => void;
  onConnectWallet: () => void;
};

export function WidgetControls({
  addressInput,
  identity,
  showCheckButton,
  showConnectButton,
  actionBusy,
  checkBusy,
  connectWalletBusy,
  addressInputRef,
  onAddressChange,
  onCheck,
  onPaste,
  onClear,
  onConnectWallet,
}: Props) {
  if (identity.source !== "none") return null;

  const hasAddressInput = addressInput.trim().length > 0;
  const controlsBusy = actionBusy || checkBusy || connectWalletBusy;

  return (
    <div className="widget__row">
      <label className="sr-only" htmlFor="address">
        Ethereum address
      </label>
      <div className="field">
        <input
          id="address"
          name="address"
          className="field__input"
          inputMode="text"
          autoComplete="off"
          spellCheck={false}
          placeholder="Enter your address"
          value={addressInput}
          onChange={(event) => onAddressChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            onCheck();
          }}
          disabled={actionBusy}
          ref={addressInputRef}
        />
        <button
          className="field__clear"
          type="button"
          data-action={hasAddressInput ? "clear" : "paste"}
          onClick={hasAddressInput ? onClear : onPaste}
          disabled={controlsBusy}
        >
          {hasAddressInput ? "Clear" : "Paste"}
        </button>
      </div>

      {showCheckButton && !checkBusy ? (
        <button
          className="btn btn--primary widget__animated"
          type="button"
          data-action="check"
          onClick={onCheck}
          disabled={controlsBusy}
        >
          Check Remaining Funds
        </button>
      ) : null}

      {showConnectButton ? (
        <ActionButton
          variant="secondary"
          animated
          data-action="connect-wallet"
          onClick={onConnectWallet}
          disabled={controlsBusy}
        >
          {connectWalletBusy ? "Connecting..." : "Connect Wallet"}
        </ActionButton>
      ) : null}
    </div>
  );
}
