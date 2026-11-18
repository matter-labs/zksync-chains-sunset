import { useRef, useState } from "react";

import { formatExactTokenAmount } from "@shared/ui/token-amount";

import type { ActionKind, Address, EligibleToken } from "../../lib/api";
import { shortenAddress } from "../../lib/validators";
import { actionStateFor, type TokenActionState } from "./types";
import type { ClaimMode } from "./useClaimWidgetController";
import { isClaimable, isMigratable } from "./utils";

type Props = {
  tokens: EligibleToken[];
  actionStates: Record<string, TokenActionState>;
  actionBusy: boolean;
  enabledActions: ActionKind[];
  allowBatchClaimAll?: boolean;
  walletConnected: boolean;
  claimMode: ClaimMode;
  onRunAction: (
    action: ActionKind,
    tokenAddress: Address,
    mode?: ClaimMode,
  ) => void;
  onRunAllAction: (action: ActionKind, mode?: ClaimMode) => void;
};

function CopyAddressButton({ tokenAddress }: { tokenAddress: Address }) {
  const [status, setStatus] = useState<"idle" | "copied" | "error">("idle");
  const resetTimeoutRef = useRef<number | null>(null);

  function scheduleReset() {
    if (resetTimeoutRef.current !== null) {
      window.clearTimeout(resetTimeoutRef.current);
    }
    resetTimeoutRef.current = window.setTimeout(() => {
      resetTimeoutRef.current = null;
      setStatus("idle");
    }, 1500);
  }

  async function copyAddress() {
    try {
      await navigator.clipboard.writeText(tokenAddress);
      setStatus("copied");
      scheduleReset();
    } catch {
      setStatus("error");
      scheduleReset();
    }
  }

  const title =
    status === "copied"
      ? "Copied"
      : status === "error"
        ? "Unavailable"
        : "Copy";

  return (
    <button
      className="card__copy"
      type="button"
      data-action="copy-token-address"
      data-token-address={tokenAddress}
      aria-label={`Copy token address ${tokenAddress}`}
      title={title}
      onClick={() => {
        void copyAddress();
      }}
    >
      {status === "copied" ? (
        <svg
          aria-hidden="true"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ) : (
        <svg
          aria-hidden="true"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      )}
    </button>
  );
}

function BalanceAmount({ token }: { token: EligibleToken }) {
  const exactBalance = formatExactTokenAmount(token.rawBalance, token.decimals);
  const exactBalanceLabel = `${exactBalance} ${token.symbol}`;

  return (
    <span
      className="card__balance"
      title={`Exact balance: ${exactBalanceLabel}`}
      aria-label={`${token.displayBalance} ${token.symbol}. Exact balance: ${exactBalanceLabel}`}
    >
      {token.displayBalance}
    </span>
  );
}

function actionButtonState(
  token: EligibleToken,
  action: ActionKind,
  actionStates: Record<string, TokenActionState>,
  actionBusy: boolean,
  idleLabel: string,
  busyLabel: string,
  variant?: string,
): { text: string; disabled: boolean } {
  const state = actionStateFor(
    actionStates,
    action,
    token.tokenAddress,
    variant,
  );
  if (state.kind === "signing") return { text: "Signature...", disabled: true };
  if (state.kind === "submitting") {
    return { text: "Submitting...", disabled: true };
  }
  if (state.kind === "polling") return { text: busyLabel, disabled: true };
  if (state.kind === "error")
    return { text: `Retry ${idleLabel.toLowerCase()}`, disabled: false };
  return { text: idleLabel, disabled: actionBusy };
}

function RowActionButton({
  token,
  action,
  label,
  busyLabel,
  mode,
  actionStates,
  actionBusy,
  onRunAction,
}: {
  token: EligibleToken;
  action: ActionKind;
  label: string;
  busyLabel: string;
  mode?: ClaimMode;
  actionStates: Record<string, TokenActionState>;
  actionBusy: boolean;
  onRunAction: (
    action: ActionKind,
    tokenAddress: Address,
    mode?: ClaimMode,
  ) => void;
}) {
  const { text, disabled } = actionButtonState(
    token,
    action,
    actionStates,
    actionBusy,
    label,
    busyLabel,
    mode,
  );

  return (
    <button
      className="btn btn--secondary btn--sm"
      type="button"
      data-action={action}
      data-claim-mode={mode}
      data-token-address={token.tokenAddress}
      disabled={disabled}
      onClick={() => onRunAction(action, token.tokenAddress, mode)}
    >
      {text}
    </button>
  );
}

export function EligibilityCards({
  tokens,
  actionStates,
  actionBusy,
  enabledActions,
  allowBatchClaimAll = false,
  walletConnected,
  claimMode,
  onRunAction,
  onRunAllAction,
}: Props) {
  if (tokens.length === 0) {
    return (
      <div className="notice">No eligible balances found for this address.</div>
    );
  }

  const claimEnabled = enabledActions.includes("claim") && walletConnected;
  const migrateEnabled = enabledActions.includes("migrate");
  const claimableCount = claimEnabled ? tokens.filter(isClaimable).length : 0;
  const anyClaimable = claimableCount > 0;
  const showBatchActions = allowBatchClaimAll && claimableCount > 1;

  return (
    <>
      {showBatchActions ? (
        <div className="card">
          <div className="card__meta">
            <div className="card__title">Batch actions</div>
            <div className="card__sub">
              Withdraw every eligible token in one transaction.
            </div>
          </div>
          <div className="card__right">
            {allowBatchClaimAll ? (
              <button
                className="btn btn--secondary btn--sm"
                type="button"
                data-action="claim-all"
                data-claim-mode={claimMode}
                disabled={actionBusy || !anyClaimable}
                onClick={() => onRunAllAction("claim", claimMode)}
              >
                Withdraw all
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {tokens.map((token) => {
        const claimState = actionStateFor(
          actionStates,
          "claim",
          token.tokenAddress,
          claimMode,
        );
        const migrateState = actionStateFor(
          actionStates,
          "migrate",
          token.tokenAddress,
        );
        const rowError =
          claimState.kind === "error"
            ? claimState.message
            : migrateState.kind === "error"
              ? migrateState.message
              : null;

        return (
          <div className="card card--token" key={token.tokenAddress}>
            <div className="card__top">
              <div className="card__identity">
                <div className="card__title">{token.symbol}</div>
                <div className="card__addressRow">
                  <code className="card__address">
                    {shortenAddress(token.tokenAddress)}
                  </code>
                  <CopyAddressButton tokenAddress={token.tokenAddress} />
                </div>
              </div>

              <div className="card__right">
                {token.claimed ? (
                  <span className="tag tag--success">Withdrawn</span>
                ) : null}
                {claimEnabled && isClaimable(token) ? (
                  <RowActionButton
                    token={token}
                    action="claim"
                    label="Withdraw"
                    busyLabel="In progress..."
                    mode={claimMode}
                    actionStates={actionStates}
                    actionBusy={actionBusy}
                    onRunAction={onRunAction}
                  />
                ) : null}
                {migrateEnabled && isMigratable(token) ? (
                  <RowActionButton
                    token={token}
                    action="migrate"
                    label="Migrate token"
                    busyLabel="In progress..."
                    actionStates={actionStates}
                    actionBusy={actionBusy}
                    onRunAction={onRunAction}
                  />
                ) : null}
              </div>
            </div>

            <div className="card__sub">
              Balance: <BalanceAmount token={token} />
            </div>
            {rowError ? (
              <div className="card__hint card__hint--error">{rowError}</div>
            ) : null}
          </div>
        );
      })}
    </>
  );
}
