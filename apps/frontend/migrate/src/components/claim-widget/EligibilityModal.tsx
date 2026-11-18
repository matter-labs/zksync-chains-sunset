import type { ActionKind, Address } from "../../lib/api";
import { shortenAddress } from "../../lib/validators";
import { EligibilityCards } from "./EligibilityCards";
import type {
  EligibilityModalState,
  TokenActionState,
  ViewState,
} from "./types";

type Props = {
  state: EligibilityModalState;
  view: ViewState;
  address: Address | null;
  busy: boolean;
  actionStates: Record<string, TokenActionState>;
  actionBusy: boolean;
  enabledActions: ActionKind[];
  allowBatchClaimAll?: boolean;
  walletConnected: boolean;
  onRunAction: (action: ActionKind, tokenAddress: Address) => void;
  onRunAllAction: (action: ActionKind) => void;
  onRequestClose: () => void;
};

function modalTitle(state: EligibilityModalState): string {
  if (state.kind === "connecting_wallet") return "Connecting wallet";
  if (state.kind === "checking_eligibility") return "Checking eligibility";
  if (state.kind === "error") return state.title;
  return "Eligibility results";
}

export function EligibilityModal({
  state,
  view,
  address,
  busy,
  actionStates,
  actionBusy,
  enabledActions,
  allowBatchClaimAll = false,
  walletConnected,
  onRunAction,
  onRunAllAction,
  onRequestClose,
}: Props) {
  if (state.kind === "closed") return null;

  return (
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
        className="claim-modal eligibility-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="eligibility-modal-title"
      >
        <div className="claim-modal__header">
          <h3 id="eligibility-modal-title" className="claim-modal__title">
            {modalTitle(state)}
          </h3>
          <button
            type="button"
            className="claim-modal__close"
            onClick={onRequestClose}
            disabled={busy}
          >
            Close
          </button>
        </div>

        {state.kind === "results" &&
        view.kind === "loaded" &&
        address !== null ? (
          <p className="claim-modal__subtitle">
            Address: {shortenAddress(address)}
          </p>
        ) : null}

        {state.kind === "connecting_wallet" ||
        state.kind === "checking_eligibility" ? (
          <>
            <div className="claim-modal__spinner-row">
              <span className="claim-modal__spinner" />
              <span className="claim-modal__spinner-text">
                {state.kind === "connecting_wallet"
                  ? "Waiting for wallet connection..."
                  : "Checking eligibility..."}
              </span>
            </div>
            <p className="claim-modal__message">
              {state.kind === "connecting_wallet"
                ? "Approve the connection request in your wallet."
                : "Fetching token balances and migration status."}
            </p>
          </>
        ) : null}

        {state.kind === "error" ? (
          <p className="claim-modal__message">{state.message}</p>
        ) : null}

        {state.kind === "results" && view.kind === "loaded" ? (
          <div className="widget__results" data-role="results">
            <EligibilityCards
              tokens={view.tokens}
              actionStates={actionStates}
              actionBusy={actionBusy}
              enabledActions={enabledActions}
              allowBatchClaimAll={allowBatchClaimAll}
              walletConnected={walletConnected}
              onRunAction={onRunAction}
              onRunAllAction={onRunAllAction}
            />
          </div>
        ) : null}
      </section>
    </div>
  );
}
