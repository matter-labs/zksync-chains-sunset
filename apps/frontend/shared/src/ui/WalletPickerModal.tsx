import styles from "./WalletPickerModal.module.css";
import { useRef } from "react";
import { createPortal } from "react-dom";

import { ActionButton } from "./ActionButton";
import { walletActionLabel } from "./wallet-labels";
import { useModalAccessibility } from "./useModalAccessibility";
import type { WalletOption } from "../wallet/session";

type Props = {
  isOpen: boolean;
  wallets: WalletOption[];
  onRequestClose: () => void;
  onSelectWallet: (walletId: string) => void;
};

export function WalletPickerModal({
  isOpen,
  wallets,
  onRequestClose,
  onSelectWallet,
}: Props) {
  const cardRef = useRef<HTMLElement>(null);
  const firstWalletRef = useRef<HTMLButtonElement>(null);

  useModalAccessibility({
    isOpen,
    busy: false,
    rootRef: cardRef,
    initialFocusRef: firstWalletRef,
    onRequestClose,
  });

  if (!isOpen || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="claim-modal-backdrop"
      aria-hidden={false}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        onRequestClose();
      }}
    >
      <section
        ref={cardRef}
        className={`claim-modal ${styles.modal}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="wallet-picker-title"
      >
        <div className={styles.header}>
          <div>
            <div className={styles.eyebrow}>Wallet access</div>
            <h2 className={styles.title} id="wallet-picker-title">
              Choose a wallet
            </h2>
          </div>
          <button
            className={styles.dismiss}
            type="button"
            onClick={onRequestClose}
            aria-label="Close wallet picker"
          >
            Close
          </button>
        </div>

        <div className={styles.options}>
          {wallets.map((wallet, index) => (
            <div className={styles.optionRow} key={wallet.id}>
              <ActionButton
                ref={index === 0 ? firstWalletRef : undefined}
                variant="secondary"
                block
                className={styles.option}
                aria-label={walletActionLabel(wallet)}
                onClick={() => {
                  onSelectWallet(wallet.id);
                }}
              >
                <span className={styles.optionLabel}>
                  {walletActionLabel(wallet)}
                </span>
              </ActionButton>
            </div>
          ))}
        </div>
      </section>
    </div>,
    document.body,
  );
}
