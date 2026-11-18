import { useReducer, useState } from "react";
import { useWalletSession } from "@shared/wallet/session";
import { useControllerAsyncSlots } from "@shared/widget/useControllerAsyncSlots";

import type {
  Address,
  MigrationPrepareResponse,
  MigrationResponse,
} from "../../lib/api";
import {
  ApiError,
  getMigrationTokens,
  prepareMigration,
  submitMigration,
} from "../../lib/api";
import { enrichMigrationTokens } from "../../lib/migration-state";
import type { IdentityState } from "../app-shell/types";
import { migrationStatusLabel, pollMigrationUntilTerminal } from "./api-flow";
import { initialWidgetState, widgetReducer } from "./reducer";
import type { EligibilityModalState } from "./types";
import { isModalBusy } from "./types";
import {
  asErrorMessage,
  isMigratable,
  sameAddress,
  screenAddress,
} from "./utils";

type Options = {
  identity: IdentityState;
  onRequiredChainChange: (chainId: number | null) => void;
};

function formatApiErrorMessage(err: ApiError): string {
  return err.message;
}

function formatApiErrorDetails(err: ApiError): string | undefined {
  return err.code ? `errorCode: ${err.code}` : undefined;
}

function formatMigrationFailureDetails(
  response: MigrationResponse,
): string | undefined {
  const details = [
    response.errorCode ? `errorCode: ${response.errorCode}` : "",
    response.failureStage ? `failureStage: ${response.failureStage}` : "",
    response.txHash ? `txHash: ${response.txHash}` : "",
    response.receipt ? `block: ${response.receipt.blockNumber}` : "",
    response.delivery?.path ? `deliveryPath: ${response.delivery.path}` : "",
    response.delivery?.claimDistributorAddress
      ? `claimDistributor: ${response.delivery.claimDistributorAddress}`
      : "",
  ].filter(Boolean);

  return details.length > 0 ? details.join("\n") : undefined;
}

export function useMigrationWidgetController({
  identity,
  onRequiredChainChange,
}: Options) {
  const wallet = useWalletSession();
  const asyncSlots = useControllerAsyncSlots();
  const [state, dispatch] = useReducer(widgetReducer, initialWidgetState);
  const [eligibilityModalState, setEligibilityModalState] =
    useState<EligibilityModalState>({ kind: "closed" });

  const actionBusy = state.actionBusy;
  const checkBusy = state.view.kind === "loading";
  const modalBusy = isModalBusy(state.modal);
  const connectWalletBusy =
    state.connectWalletBusy ||
    wallet.status === "connecting" ||
    wallet.status === "reconnecting";
  const controlsBusy =
    actionBusy ||
    checkBusy ||
    connectWalletBusy ||
    wallet.status === "reconnecting";
  const enabledActions: Array<"migrate"> =
    identity.source === "wallet" ? ["migrate"] : [];

  function clearActionOperation() {
    asyncSlots.action.clear();
    dispatch({ type: "set_action_busy", busy: false });
  }

  function resetWidget() {
    const keepLoadedView = state.view.kind === "loaded";
    onRequiredChainChange(null);
    asyncSlots.check.cancel();
    asyncSlots.check.invalidate();
    clearActionOperation();
    dispatch({ type: "clear_action_states" });
    dispatch({ type: "set_connect_wallet_busy", busy: false });
    dispatch({ type: "set_modal", modal: { kind: "closed" } });
    if (!keepLoadedView) {
      dispatch({ type: "set_view", view: { kind: "idle" } });
    }
    setEligibilityModalState(
      keepLoadedView ? { kind: "results" } : { kind: "closed" },
    );
  }

  function dismissModal() {
    dispatch({ type: "set_modal", modal: { kind: "closed" } });
  }

  async function runShowTokens() {
    if (actionBusy) return;

    const request = asyncSlots.check.begin();

    dispatch({ type: "clear_action_states" });
    dispatch({ type: "set_modal", modal: { kind: "closed" } });
    dispatch({ type: "set_view", view: { kind: "loading" } });
    setEligibilityModalState({ kind: "checking_eligibility" });

    try {
      const res = await getMigrationTokens(request.signal);
      if (
        !asyncSlots.mountedRef.current ||
        !asyncSlots.check.isCurrent(request.id)
      ) {
        return;
      }
      const enriched = await enrichMigrationTokens(res.tokens);
      if (
        !asyncSlots.mountedRef.current ||
        !asyncSlots.check.isCurrent(request.id)
      ) {
        return;
      }
      dispatch({
        type: "set_view",
        view: {
          kind: "loaded",
          tokens: enriched,
        },
      });
      setEligibilityModalState({ kind: "results" });
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (
        !asyncSlots.mountedRef.current ||
        !asyncSlots.check.isCurrent(request.id)
      ) {
        return;
      }
      const message =
        err instanceof ApiError
          ? formatApiErrorMessage(err)
          : asErrorMessage(err);
      dispatch({ type: "set_view", view: { kind: "error", message } });
      setEligibilityModalState({
        kind: "error",
        title: "Could not load tokens",
        message,
      });
    }
  }

  async function connectWallet(connectorId?: string) {
    if (actionBusy) return;
    setEligibilityModalState({ kind: "connecting_wallet" });
    dispatch({ type: "set_connect_wallet_busy", busy: true });
    try {
      await wallet.connect(connectorId);
      if (state.view.kind === "loaded") {
        setEligibilityModalState({ kind: "results" });
      } else {
        setEligibilityModalState({ kind: "closed" });
      }
    } catch (err) {
      if (!asyncSlots.mountedRef.current) return;
      const message = asErrorMessage(err);
      dispatch({ type: "set_view", view: { kind: "error", message } });
      setEligibilityModalState({
        kind: "error",
        title: "Wallet connection failed",
        message,
      });
    } finally {
      if (asyncSlots.mountedRef.current) {
        dispatch({ type: "set_connect_wallet_busy", busy: false });
      }
    }
  }

  function setTokenState(
    tokenAddress: Address,
    next:
      | { kind: "idle" }
      | { kind: "signing" }
      | { kind: "submitting" }
      | { kind: "polling" }
      | { kind: "error"; message: string },
  ) {
    dispatch({
      type: "set_action_state",
      action: "migrate",
      tokenAddress,
      next,
    });
  }

  async function runMigration(tokenAddress: Address) {
    if (actionBusy) return;
    const loadedView = state.view;
    if (loadedView.kind !== "loaded") return;
    if (identity.source !== "wallet" || identity.address === null) {
      dispatch({
        type: "set_view",
        view: {
          kind: "error",
          message: "Connect a wallet to pay gas and migrate this token.",
        },
      });
      setEligibilityModalState({
        kind: "error",
        title: "Wallet required",
        message: "Connect a wallet to pay gas and migrate this token.",
      });
      return;
    }

    const token = loadedView.tokens.find((item) =>
      sameAddress(item.tokenAddress, tokenAddress),
    );
    if (!token || !isMigratable(token)) return;

    const operation = asyncSlots.action.begin();
    const isCurrent = () => asyncSlots.action.isCurrent(operation.id);

    dispatch({ type: "set_action_busy", busy: true });
    setTokenState(tokenAddress, { kind: "signing" });
    dispatch({
      type: "set_modal",
      modal: {
        kind: "awaiting_signature",
        action: "migrate",
        tokens: [token],
      },
    });

    let prepared: MigrationPrepareResponse;
    try {
      await screenAddress(identity.address, operation.signal);
      prepared = await prepareMigration(
        {
          address: identity.address,
          tokenAddress,
        },
        operation.signal,
      );
      onRequiredChainChange(prepared.chainId);
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      const message =
        err instanceof ApiError
          ? formatApiErrorMessage(err)
          : asErrorMessage(err);
      setTokenState(tokenAddress, { kind: "error", message });
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "migrate",
          tokens: [token],
          stage: "submit",
          message,
          details:
            err instanceof ApiError ? formatApiErrorDetails(err) : undefined,
        },
      });
      clearActionOperation();
      return;
    }

    let txHash: `0x${string}`;
    try {
      txHash = await wallet.sendPreparedTransaction(
        identity.address,
        prepared,
        operation.signal,
      );
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      const message = asErrorMessage(err);
      setTokenState(tokenAddress, { kind: "error", message });
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "migrate",
          tokens: [token],
          stage: "signature",
          message,
        },
      });
      clearActionOperation();
      return;
    }

    setTokenState(tokenAddress, { kind: "submitting" });
    dispatch({
      type: "set_modal",
      modal: { kind: "submitting", action: "migrate", tokens: [token] },
    });

    let created: MigrationResponse;
    try {
      created = await submitMigration(
        { migrationId: prepared.migrationId, txHash },
        operation.signal,
      );
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      const message =
        err instanceof ApiError
          ? formatApiErrorMessage(err)
          : asErrorMessage(err);
      setTokenState(tokenAddress, { kind: "error", message });
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "migrate",
          tokens: [token],
          stage: "submit",
          message,
          details:
            err instanceof ApiError ? formatApiErrorDetails(err) : undefined,
        },
      });
      clearActionOperation();
      return;
    }

    setTokenState(tokenAddress, { kind: "polling" });

    try {
      const final = await pollMigrationUntilTerminal(
        created,
        (snapshot) => {
          if (!isCurrent() || !asyncSlots.mountedRef.current) return;
          dispatch({
            type: "set_modal",
            modal: {
              kind: "polling",
              action: "migrate",
              tokens: [token],
              txHash: snapshot.txHash,
              statusLabel: migrationStatusLabel(snapshot.status),
            },
          });
        },
        operation.signal,
      );
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;

      if (final.status === "mined_success") {
        dispatch({ type: "mark_token_migrated", tokenAddress });
        setTokenState(tokenAddress, { kind: "idle" });
        dispatch({
          type: "set_modal",
          modal: {
            kind: "migrate_success",
            tokens: [token],
            txHash: final.txHash,
            receipt: final.receipt,
          },
        });
        clearActionOperation();
        return;
      }

      const message = final.error ?? "Migration transaction failed.";
      const details = formatMigrationFailureDetails(final);
      setTokenState(tokenAddress, { kind: "error", message });
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "migrate",
          tokens: [token],
          stage: "poll",
          message,
          details,
        },
      });
      clearActionOperation();
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      const message =
        err instanceof ApiError
          ? formatApiErrorMessage(err)
          : asErrorMessage(err);
      setTokenState(tokenAddress, { kind: "error", message });
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "migrate",
          tokens: [token],
          stage: "poll",
          message,
          details:
            err instanceof ApiError ? formatApiErrorDetails(err) : undefined,
        },
      });
      clearActionOperation();
    }
  }

  return {
    ui: {
      eligibilityModalState,
      state,
    },
    derived: {
      actionBusy,
      connectWalletBusy,
      controlsBusy,
      enabledActions,
      showTokensBusy: checkBusy,
    },
    actions: {
      connectWallet: (connectorId?: string) => {
        void connectWallet(connectorId);
      },
      migrateToken: (tokenAddress: Address) => {
        void runMigration(tokenAddress);
      },
      requestModalClose: () => {
        if (modalBusy) return;
        dismissModal();
      },
      resetWidget,
      showTokens: () => {
        void runShowTokens();
      },
    },
  };
}
