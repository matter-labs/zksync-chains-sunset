import { useRef } from "react";
import { createPortal } from "react-dom";

import { useModalAccessibility } from "@shared/ui/useModalAccessibility";

import type { ActionKind } from "../../lib/api";
import type { TxModalState } from "./types";
import { isModalBusy } from "./types";
import { actionLabel } from "./utils";

type Props = {
  state: TxModalState;
  onRequestClose: () => void;
};

function stepState(
  modalState: TxModalState,
): [
  "pending" | "active" | "done" | "error",
  "pending" | "active" | "done" | "error",
  "pending" | "active" | "done" | "error",
] {
  if (modalState.kind === "awaiting_signature") {
    return ["active", "pending", "pending"];
  }
  if (modalState.kind === "submitting") {
    return ["done", "active", "pending"];
  }
  if (modalState.kind === "polling") {
    return ["done", "done", "active"];
  }
  if (
    modalState.kind === "claim_success" ||
    modalState.kind === "migrate_success"
  ) {
    return ["done", "done", "done"];
  }
  if (modalState.kind === "error" && modalState.stage === "signature") {
    return ["error", "pending", "pending"];
  }
  if (modalState.kind === "error" && modalState.stage === "submit") {
    return ["done", "error", "pending"];
  }
  if (modalState.kind === "error" && modalState.stage === "poll") {
    return ["done", "done", "error"];
  }
  return ["pending", "pending", "pending"];
}

function tokenPreview(
  tokens: { tokenAddress: string; symbol: string }[],
): string {
  if (tokens.length === 0) return "";
  const sample = tokens
    .slice(0, 4)
    .map((token) => token.symbol)
    .join(", ");
  if (tokens.length <= 4) return sample;
  return `${sample}, +${tokens.length - 4} more`;
}

function modalAction(state: TxModalState): ActionKind | null {
  if (
    state.kind === "awaiting_signature" ||
    state.kind === "submitting" ||
    state.kind === "polling" ||
    state.kind === "error"
  ) {
    return state.action;
  }
  if (state.kind === "claim_success") return "claim";
  if (state.kind === "migrate_success") return "migrate";
  return null;
}

function stepLabels(action: ActionKind | null): [string, string, string] {
  if (action === "migrate") {
    return ["1. Wallet", "2. Submit", "3. Mining"];
  }
  return ["1. Prepare", "2. Submit", "3. Mining"];
}

function errorStageLabel(
  action: ActionKind,
  stage: "signature" | "submit" | "poll",
): string {
  if (action === "migrate" && stage === "signature") {
    return "wallet stage";
  }
  return `${stage} stage`;
}

function modalCopy(state: TxModalState): {
  title: string;
  subtitle: string;
  message: string;
  details: string;
  spinnerText: string;
  showSpinner: boolean;
  ariaLive: string;
} {
  if (state.kind === "awaiting_signature") {
    const isMigration = state.action === "migrate";
    return {
      title: `${actionLabel(state.action)} ${state.tokens.length > 1 ? "batch" : "request"}`,
      subtitle: tokenPreview(state.tokens),
      message: isMigration
        ? "Confirm the transaction in your wallet."
        : "Preparing the claim request.",
      details: `items: ${state.tokens.length}`,
      spinnerText: isMigration
        ? "Waiting for wallet confirmation..."
        : "Preparing claim...",
      showSpinner: true,
      ariaLive: isMigration
        ? "Waiting for wallet confirmation"
        : "Preparing claim",
    };
  }

  if (state.kind === "submitting") {
    const isMigration = state.action === "migrate";
    return {
      title: `${actionLabel(state.action)} request`,
      subtitle: tokenPreview(state.tokens),
      message: isMigration
        ? "Transaction submitted. Recording it with the backend..."
        : "Submitting sponsored transaction...",
      details: `items: ${state.tokens.length}`,
      spinnerText: isMigration
        ? "Recording submitted transaction..."
        : "Submitting request to backend...",
      showSpinner: true,
      ariaLive: "Submitting request",
    };
  }

  if (state.kind === "polling") {
    return {
      title: `${actionLabel(state.action)} in progress`,
      subtitle: tokenPreview(state.tokens),
      message: "Waiting for transaction to be mined.",
      details: state.txHash ? `txHash: ${state.txHash}` : "",
      spinnerText: `${state.statusLabel}...`,
      showSpinner: true,
      ariaLive: state.statusLabel,
    };
  }

  if (state.kind === "claim_success") {
    return {
      title: "Claim complete",
      subtitle: tokenPreview(state.tokens),
      message: "Claim request was mined successfully.",
      details: [
        state.txHash ? `txHash: ${state.txHash}` : "",
        state.receipt ? `block: ${state.receipt.blockNumber}` : "",
      ]
        .filter(Boolean)
        .join("\n"),
      spinnerText: "",
      showSpinner: false,
      ariaLive: "Claim complete",
    };
  }

  if (state.kind === "migrate_success") {
    return {
      title: "Migration complete",
      subtitle: tokenPreview(state.tokens),
      message: "Migration request was mined successfully.",
      details: [
        state.txHash ? `txHash: ${state.txHash}` : "",
        state.receipt ? `block: ${state.receipt.blockNumber}` : "",
      ]
        .filter(Boolean)
        .join("\n"),
      spinnerText: "",
      showSpinner: false,
      ariaLive: "Migration complete",
    };
  }

  if (state.kind === "error") {
    return {
      title: `${actionLabel(state.action)} failed`,
      subtitle: errorStageLabel(state.action, state.stage),
      message: state.message,
      details: state.details ?? "",
      spinnerText: "",
      showSpinner: false,
      ariaLive: `${actionLabel(state.action)} failed`,
    };
  }

  return {
    title: "",
    subtitle: "",
    message: "",
    details: "",
    spinnerText: "",
    showSpinner: false,
    ariaLive: "",
  };
}

export function TxModal({ state, onRequestClose }: Props) {
  const isOpen = state.kind !== "closed";
  const busy = isModalBusy(state);
  const cardRef = useRef<HTMLElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);

  const copy = modalCopy(state);
  const [signatureStep, submitStep, miningStep] = stepState(state);
  const [signatureStepLabel, submitStepLabel, miningStepLabel] = stepLabels(
    modalAction(state),
  );

  useModalAccessibility({
    isOpen,
    busy,
    rootRef: cardRef,
    initialFocusRef: closeButtonRef,
    onRequestClose,
  });

  if (!isOpen || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="claim-modal-backdrop"
      aria-hidden={false}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        if (busy) return;
        onRequestClose();
      }}
    >
      <section
        className="claim-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="claim-modal-title"
        ref={cardRef}
      >
        <div className="claim-modal__header">
          <h3 id="claim-modal-title" className="claim-modal__title">
            {copy.title}
          </h3>
          <button
            ref={closeButtonRef}
            type="button"
            className="claim-modal__close"
            onClick={onRequestClose}
            disabled={busy}
          >
            Close
          </button>
        </div>

        <p className="claim-modal__subtitle">{copy.subtitle}</p>
        <p className="sr-only" aria-live="polite">
          {copy.ariaLive}
        </p>

        <ol className="claim-modal__steps">
          <li className="claim-modal__step" data-step-state={signatureStep}>
            {signatureStepLabel}
          </li>
          <li className="claim-modal__step" data-step-state={submitStep}>
            {submitStepLabel}
          </li>
          <li className="claim-modal__step" data-step-state={miningStep}>
            {miningStepLabel}
          </li>
        </ol>

        {copy.showSpinner ? (
          <div className="claim-modal__spinner-row">
            <span className="claim-modal__spinner" />
            <span className="claim-modal__spinner-text">
              {copy.spinnerText}
            </span>
          </div>
        ) : null}

        <p className="claim-modal__message">{copy.message}</p>
        <pre className="claim-modal__details">{copy.details}</pre>
      </section>
    </div>,
    document.body,
  );
}
