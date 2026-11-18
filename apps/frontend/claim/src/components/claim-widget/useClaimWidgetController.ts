import { useEffect, useReducer, useRef, useState } from "react";
import type { Hex } from "viem";
import { useWalletSession } from "@shared/wallet/session";
import { useControllerAsyncSlots } from "@shared/widget/useControllerAsyncSlots";

import type {
  ActionKind,
  Address,
  ClaimPrepareResponse,
  ClaimResponse,
  EligibleToken,
} from "../../lib/api";
import {
  ApiError,
  checkEligibility,
  prepareClaim,
  submitClaim,
  submitDirectClaim,
} from "../../lib/api";
import { enrichEligibilityTokens } from "../../lib/enrich-eligibility";
import { validateAddress } from "../../lib/validators";
import type { IdentityState } from "../app-shell/types";
import { claimStatusLabel, pollClaimUntilTerminal } from "./api-flow";
import { initialWidgetState, widgetReducer } from "./reducer";
import type { EligibilityModalState } from "./types";
import { isModalBusy } from "./types";
import {
  asErrorMessage,
  isClaimable,
  sameAddress,
  screenAddress,
} from "./utils";

type ActionScope = { kind: "single"; tokenAddress: Address } | { kind: "all" };
export type ClaimMode = "sponsored" | "paid";

type Options = {
  identity: IdentityState;
  onRequiredChainChange: (chainId: number | null) => void;
  onSelectedAddressChange: (address: Address | null) => void;
};

function formatApiErrorMessage(err: ApiError): string {
  if (err.code === "UNSUPPORTED_CHAIN_ID") {
    return "Wallet is on the wrong network for this action. Please switch network and retry.";
  }
  return err.message;
}

function selectedTokensForAction(
  scope: ActionScope,
  tokens: EligibleToken[],
): Address[] {
  if (scope.kind === "single") return [scope.tokenAddress];
  return tokens.filter(isClaimable).map((token) => token.tokenAddress);
}

function tokensForPreparedClaim(
  tokens: EligibleToken[],
  prepared: ClaimPrepareResponse,
): EligibleToken[] {
  return prepared.items
    .map((item) =>
      tokens.find((token) =>
        sameAddress(token.tokenAddress, item.tokenAddress),
      ),
    )
    .filter((token): token is EligibleToken => token !== undefined);
}

export function useClaimWidgetController({
  identity,
  onRequiredChainChange,
  onSelectedAddressChange,
}: Options) {
  const wallet = useWalletSession();
  const asyncSlots = useControllerAsyncSlots();
  const [state, dispatch] = useReducer(widgetReducer, initialWidgetState);
  const [addressInput, setAddressInput] = useState("");
  const [eligibilityModalState, setEligibilityModalState] =
    useState<EligibilityModalState>({ kind: "closed" });
  const addressInputRef = useRef<HTMLInputElement>(null);
  const autoCheckedWalletAddressRef = useRef<Address | null>(null);

  const actionBusy = state.actionBusy;
  const checkBusy = state.view.kind === "loading";
  const modalBusy = isModalBusy(state.modal);
  const showCheckButton =
    identity.source === "none" && state.view.kind === "idle";
  const showConnectButton =
    identity.source === "none" && wallet.status !== "reconnecting";
  const connectWalletBusy =
    state.connectWalletBusy ||
    wallet.status === "connecting" ||
    wallet.status === "reconnecting";

  useEffect(() => {
    if (identity.source !== "wallet" || identity.address === null) {
      autoCheckedWalletAddressRef.current = null;
      return;
    }

    onSelectedAddressChange(null);

    if (
      autoCheckedWalletAddressRef.current !== null &&
      sameAddress(autoCheckedWalletAddressRef.current, identity.address)
    ) {
      return;
    }

    if (actionBusy || checkBusy || state.connectWalletBusy) return;

    autoCheckedWalletAddressRef.current = identity.address;
    setAddressInput(identity.address);
    void runCheck(identity.address, "wallet");
  }, [
    identity.address,
    identity.source,
    actionBusy,
    checkBusy,
    onSelectedAddressChange,
    state.connectWalletBusy,
  ]);

  function clearActionOperation() {
    asyncSlots.action.clear();
    dispatch({ type: "set_action_busy", busy: false });
  }

  function currentAddressValue(): string {
    return addressInputRef.current?.value ?? addressInput;
  }

  function resetEligibilityUi() {
    asyncSlots.check.cancel();
    asyncSlots.check.invalidate();
    onRequiredChainChange(null);
    dispatch({ type: "clear_action_states" });
    dispatch({ type: "set_modal", modal: { kind: "closed" } });
    dispatch({ type: "set_view", view: { kind: "idle" } });
    setEligibilityModalState({ kind: "closed" });
  }

  function prepareWalletConnect(): boolean {
    if (actionBusy || checkBusy || state.connectWalletBusy) return false;
    onSelectedAddressChange(null);
    setAddressInput("");
    resetEligibilityUi();
    return true;
  }

  function clear() {
    if (actionBusy || checkBusy || state.connectWalletBusy) return;
    onSelectedAddressChange(null);
    setAddressInput("");
    resetEligibilityUi();
  }

  async function pasteAddress() {
    if (actionBusy || checkBusy || state.connectWalletBusy) return;
    const text = await navigator.clipboard.readText().catch(() => "");
    if (!text) return;
    setAddressInput(text.trim());
  }

  function dismissModal() {
    dispatch({ type: "set_modal", modal: { kind: "closed" } });
  }

  async function runCheck(
    rawAddress: string,
    nextIdentitySource: "wallet" | "selected_address" = "selected_address",
  ) {
    if (actionBusy) return;

    const validated = validateAddress(rawAddress);
    if (!validated.ok) {
      dispatch({
        type: "set_view",
        view: { kind: "error", message: validated.message },
      });
      setEligibilityModalState({
        kind: "error",
        title: "Could not check eligibility",
        message: validated.message,
      });
      return;
    }

    const request = asyncSlots.check.begin();
    const address = validated.address;

    dispatch({ type: "clear_action_states" });
    dispatch({ type: "set_modal", modal: { kind: "closed" } });
    dispatch({ type: "set_view", view: { kind: "loading" } });
    setEligibilityModalState({ kind: "checking_eligibility" });

    try {
      const res = await checkEligibility(address, request.signal);
      if (
        !asyncSlots.mountedRef.current ||
        !asyncSlots.check.isCurrent(request.id)
      ) {
        return;
      }
      // The backend never reads chain state — `migrated` and `claimed` are
      // derived here by probing the migration contract and claim distributor.
      const verifiedTokens = await enrichEligibilityTokens(res.tokens);
      if (
        !asyncSlots.mountedRef.current ||
        !asyncSlots.check.isCurrent(request.id)
      ) {
        return;
      }
      setAddressInput(res.address);
      dispatch({
        type: "set_view",
        view: {
          kind: "loaded",
          address: res.address,
          tokens: verifiedTokens,
        },
      });
      if (nextIdentitySource === "selected_address") {
        onSelectedAddressChange(res.address);
      } else {
        onSelectedAddressChange(null);
      }
      setEligibilityModalState({ kind: "results" });
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (
        !asyncSlots.mountedRef.current ||
        !asyncSlots.check.isCurrent(request.id)
      ) {
        return;
      }
      if (err instanceof ApiError) {
        const message = formatApiErrorMessage(err);
        dispatch({
          type: "set_view",
          view: { kind: "error", message },
        });
        setEligibilityModalState({
          kind: "error",
          title: "Could not check eligibility",
          message,
        });
        return;
      }
      const message = asErrorMessage(err);
      dispatch({
        type: "set_view",
        view: { kind: "error", message },
      });
      setEligibilityModalState({
        kind: "error",
        title: "Could not check eligibility",
        message,
      });
    }
  }

  async function connectWallet(connectorId?: string) {
    if (actionBusy) return;
    setEligibilityModalState({ kind: "connecting_wallet" });
    dispatch({ type: "set_connect_wallet_busy", busy: true });
    try {
      const connected = await wallet.connect(connectorId);
      setAddressInput(connected.address);
    } catch (err) {
      if (!asyncSlots.mountedRef.current) return;
      const message = asErrorMessage(err);
      dispatch({
        type: "set_view",
        view: { kind: "error", message },
      });
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

  function setTokenStates(
    action: ActionKind,
    tokenAddresses: Address[],
    next:
      | { kind: "idle" }
      | { kind: "signing" }
      | { kind: "submitting" }
      | { kind: "polling" }
      | { kind: "error"; message: string },
    variant?: string,
  ) {
    tokenAddresses.forEach((tokenAddress) => {
      dispatch({
        type: "set_action_state",
        action,
        tokenAddress,
        variant,
        next,
      });
    });
  }

  async function runAction(
    action: ActionKind,
    scope: ActionScope,
    mode: ClaimMode = "sponsored",
  ) {
    if (actionBusy) return;
    const loadedView = state.view;
    if (loadedView.kind !== "loaded") return;

    const tokenAddresses = selectedTokensForAction(scope, loadedView.tokens);
    const selectedTokens = tokenAddresses
      .map((tokenAddress) =>
        loadedView.tokens.find((token) =>
          sameAddress(token.tokenAddress, tokenAddress),
        ),
      )
      .filter(
        (token): token is (typeof loadedView.tokens)[number] =>
          token !== undefined,
      );

    if (selectedTokens.length === 0) {
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action,
          tokens: [],
          stage: "submit",
          message: "No eligible tokens found for this action.",
        },
      });
      return;
    }

    if (scope.kind === "single") {
      const token = selectedTokens[0];
      if (!token) return;
      if (!isClaimable(token)) return;
    }

    const operation = asyncSlots.action.begin();
    const addressSnapshot = loadedView.address;
    let actionTokens = selectedTokens;
    const isCurrent = () => asyncSlots.action.isCurrent(operation.id);
    const stateVariant = action === "claim" ? mode : undefined;

    dispatch({ type: "set_action_busy", busy: true });
    setTokenStates(
      action,
      actionTokens.map((token) => token.tokenAddress),
      { kind: "signing" },
      stateVariant,
    );
    dispatch({
      type: "set_modal",
      modal: {
        kind: "awaiting_signature",
        action,
        tokens: actionTokens,
        variant: stateVariant,
      },
    });

    let claimPrepared: ClaimPrepareResponse | null = null;

    try {
      await screenAddress(addressSnapshot, operation.signal);
      claimPrepared = await prepareClaim(
        scope.kind === "all"
          ? { address: addressSnapshot, allEligible: true }
          : {
              address: addressSnapshot,
              tokenAddress: actionTokens[0].tokenAddress,
            },
        operation.signal,
      );
      const preparedTokens = tokensForPreparedClaim(
        loadedView.tokens,
        claimPrepared,
      );
      if (preparedTokens.length > 0) {
        const removedTokens = actionTokens.filter(
          (token) =>
            !preparedTokens.some((candidate) =>
              sameAddress(candidate.tokenAddress, token.tokenAddress),
            ),
        );
        setTokenStates(
          action,
          removedTokens.map((token) => token.tokenAddress),
          { kind: "idle" },
          stateVariant,
        );
        actionTokens = preparedTokens;
        dispatch({
          type: "set_modal",
          modal: {
            kind: "awaiting_signature",
            action,
            tokens: actionTokens,
            variant: stateVariant,
          },
        });
      }
      onRequiredChainChange(claimPrepared.typedData.domain.chainId);
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      const message =
        err instanceof ApiError
          ? formatApiErrorMessage(err)
          : asErrorMessage(err);
      setTokenStates(
        action,
        actionTokens.map((token) => token.tokenAddress),
        { kind: "error", message },
        stateVariant,
      );
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action,
          tokens: actionTokens,
          stage: "submit",
          message,
          variant: stateVariant,
        },
      });
      clearActionOperation();
      return;
    }

    let created: ClaimResponse;
    if (mode === "paid") {
      let txHash: Hex;
      try {
        txHash = await wallet.sendPreparedTransaction(
          addressSnapshot,
          {
            chainId: claimPrepared.directTx.chainId,
            contractAddress: claimPrepared.directTx.contractAddress,
            callData: claimPrepared.directTx.callData,
          },
          operation.signal,
        );
        if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      } catch (err) {
        if (err instanceof DOMException && err.name === "AbortError") return;
        if (!isCurrent() || !asyncSlots.mountedRef.current) return;
        const message = asErrorMessage(err);
        setTokenStates(
          action,
          actionTokens.map((token) => token.tokenAddress),
          { kind: "error", message },
          stateVariant,
        );
        dispatch({
          type: "set_modal",
          modal: {
            kind: "error",
            action,
            tokens: actionTokens,
            stage: "signature",
            message,
            variant: stateVariant,
          },
        });
        clearActionOperation();
        return;
      }

      setTokenStates(
        action,
        actionTokens.map((token) => token.tokenAddress),
        { kind: "submitting" },
        stateVariant,
      );
      dispatch({
        type: "set_modal",
        modal: {
          kind: "submitting",
          action,
          tokens: actionTokens,
          variant: stateVariant,
        },
      });

      try {
        created = await submitDirectClaim(
          { claimId: claimPrepared.claimId, txHash },
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
        setTokenStates(
          action,
          actionTokens.map((token) => token.tokenAddress),
          { kind: "error", message },
          stateVariant,
        );
        dispatch({
          type: "set_modal",
          modal: {
            kind: "error",
            action,
            tokens: actionTokens,
            stage: "submit",
            message,
            variant: stateVariant,
          },
        });
        clearActionOperation();
        return;
      }
    } else {
      let batchSignature: Hex | null = null;
      try {
        batchSignature = await wallet.signClaimBatchTypedData(
          addressSnapshot,
          claimPrepared.typedData,
          operation.signal,
        );
        if (!isCurrent() || !asyncSlots.mountedRef.current) return;
      } catch (err) {
        if (err instanceof DOMException && err.name === "AbortError") return;
        if (!isCurrent() || !asyncSlots.mountedRef.current) return;
        const message = asErrorMessage(err);
        setTokenStates(
          action,
          actionTokens.map((token) => token.tokenAddress),
          { kind: "error", message },
          stateVariant,
        );
        dispatch({
          type: "set_modal",
          modal: {
            kind: "error",
            action,
            tokens: actionTokens,
            stage: "signature",
            message,
            variant: stateVariant,
          },
        });
        clearActionOperation();
        return;
      }

      setTokenStates(
        action,
        actionTokens.map((token) => token.tokenAddress),
        { kind: "submitting" },
        stateVariant,
      );
      if (batchSignature === null) {
        clearActionOperation();
        return;
      }
      dispatch({
        type: "set_modal",
        modal: {
          kind: "submitting",
          action,
          tokens: actionTokens,
          variant: stateVariant,
        },
      });

      try {
        created = await submitClaim(
          {
            address: addressSnapshot,
            items: claimPrepared.items,
            claimsHash: claimPrepared.claimsHash,
            nonce: claimPrepared.nonce,
            expiry: claimPrepared.expiry,
            signature: batchSignature,
          },
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
        setTokenStates(
          action,
          actionTokens.map((token) => token.tokenAddress),
          { kind: "error", message },
          stateVariant,
        );
        dispatch({
          type: "set_modal",
          modal: {
            kind: "error",
            action,
            tokens: actionTokens,
            stage: "submit",
            message,
            variant: stateVariant,
          },
        });
        clearActionOperation();
        return;
      }
    }

    setTokenStates(
      action,
      actionTokens.map((token) => token.tokenAddress),
      { kind: "polling" },
      stateVariant,
    );

    try {
      const final = await pollClaimUntilTerminal(
        created,
        (snapshot) => {
          if (!isCurrent() || !asyncSlots.mountedRef.current) return;
          dispatch({
            type: "set_modal",
            modal: {
              kind: "polling",
              action: "claim",
              tokens: actionTokens,
              txHash: snapshot.txHash,
              statusLabel: claimStatusLabel(snapshot.status),
              variant: stateVariant,
            },
          });
        },
        operation.signal,
      );
      if (!isCurrent() || !asyncSlots.mountedRef.current) return;

      if (final.status === "mined_success") {
        for (const token of actionTokens) {
          dispatch({
            type: "mark_token_claimed",
            tokenAddress: token.tokenAddress,
          });
          dispatch({
            type: "set_action_state",
            action,
            tokenAddress: token.tokenAddress,
            variant: stateVariant,
            next: { kind: "idle" },
          });
        }

        dispatch({
          type: "set_modal",
          modal: {
            kind: "claim_success",
            tokens: actionTokens,
            txHash: final.txHash,
            chainId: claimPrepared.chainId,
            receipt: final.receipt,
          },
        });
        clearActionOperation();
        return;
      }

      for (const token of actionTokens) {
        dispatch({
          type: "set_action_state",
          action,
          tokenAddress: token.tokenAddress,
          variant: stateVariant,
          next: {
            kind: "error",
            message: final.error ?? "Withdraw transaction failed.",
          },
        });
      }

      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "claim",
          tokens: actionTokens,
          stage: "poll",
          message: final.error ?? "Withdraw transaction failed.",
          variant: stateVariant,
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
      setTokenStates(
        action,
        actionTokens.map((token) => token.tokenAddress),
        { kind: "error", message },
        stateVariant,
      );
      dispatch({
        type: "set_modal",
        modal: {
          kind: "error",
          action: "claim",
          tokens: actionTokens,
          stage: "poll",
          message,
          variant: stateVariant,
        },
      });
      clearActionOperation();
    }
  }

  return {
    ui: {
      addressInput,
      addressInputRef,
      eligibilityModalState,
      state,
    },
    derived: {
      actionBusy,
      checkBusy,
      connectWalletBusy,
      showCheckButton,
      showConnectButton,
    },
    actions: {
      clear,
      connectWallet: (connectorId?: string) => {
        void connectWallet(connectorId);
      },
      prepareWalletConnect,
      checkCurrentAddress: () => {
        void runCheck(currentAddressValue());
      },
      pasteAddress: () => {
        void pasteAddress();
      },
      requestModalClose: () => {
        if (modalBusy) return;
        dismissModal();
      },
      runAllAction: (action: ActionKind, mode: ClaimMode = "sponsored") => {
        void runAction(action, { kind: "all" }, mode);
      },
      runSingleAction: (
        action: ActionKind,
        tokenAddress: Address,
        mode: ClaimMode = "sponsored",
      ) => {
        void runAction(action, { kind: "single", tokenAddress }, mode);
      },
      setAddressInput,
    },
  };
}
