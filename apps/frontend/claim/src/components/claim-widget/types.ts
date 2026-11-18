import {
  actionKey as sharedActionKey,
  actionStateFor as sharedActionStateFor,
  isModalBusy as sharedIsModalBusy,
  isTerminalStatus,
  type AsyncViewState,
  type EligibilityModalState as SharedEligibilityModalState,
  type TokenActionState as SharedTokenActionState,
  type TxModalState as SharedTxModalState,
  type WidgetAction as SharedWidgetAction,
  type WidgetState as SharedWidgetState,
} from "@shared/widget/state";

import type {
  ActionKind,
  Address,
  ClaimResponse,
  EligibleToken,
  MigrationResponse,
} from "../../lib/api";

type LoadedView = {
  kind: "loaded";
  address: Address;
  tokens: EligibleToken[];
};

export type ViewState = AsyncViewState<LoadedView>;
export type EligibilityModalState = SharedEligibilityModalState;
export type TokenActionState = SharedTokenActionState;
export type TxModalState = SharedTxModalState<EligibleToken>;
export type WidgetState = SharedWidgetState<ViewState, EligibleToken>;
export type WidgetAction = SharedWidgetAction<
  ViewState,
  Address,
  EligibleToken
>;

export function actionKey(
  action: ActionKind,
  tokenAddress: Address,
  variant?: string,
): string {
  return sharedActionKey(action, tokenAddress, variant);
}

export function actionStateFor(
  actionStates: Record<string, TokenActionState>,
  action: ActionKind,
  tokenAddress: Address,
  variant?: string,
): TokenActionState {
  return sharedActionStateFor(actionStates, action, tokenAddress, variant);
}

export function isClaimTerminal(status: ClaimResponse["status"]): boolean {
  return isTerminalStatus(status);
}

export function isMigrationTerminal(
  status: MigrationResponse["status"],
): boolean {
  return isTerminalStatus(status);
}

export function isModalBusy(state: TxModalState): boolean {
  return sharedIsModalBusy(state);
}
