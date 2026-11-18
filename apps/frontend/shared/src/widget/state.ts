export type WidgetAddress = `0x${string}`;

export type WidgetEligibleToken = {
  tokenAddress: WidgetAddress;
  symbol: string;
  displayBalance: string;
  rawBalance: string;
  decimals: number;
  migrated: boolean;
  claimed: boolean;
};

export type LoadedViewWithTokens<
  TToken extends WidgetEligibleToken = WidgetEligibleToken,
> = {
  kind: "loaded";
  tokens: TToken[];
};

export type AsyncViewState<
  TLoaded extends LoadedViewWithTokens = LoadedViewWithTokens,
> =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | TLoaded;

export type EligibilityModalState =
  | { kind: "closed" }
  | { kind: "connecting_wallet" }
  | { kind: "checking_eligibility" }
  | { kind: "results" }
  | { kind: "error"; title: string; message: string };

export type TokenActionState =
  | { kind: "idle" }
  | { kind: "signing" }
  | { kind: "submitting" }
  | { kind: "polling" }
  | { kind: "error"; message: string };

export type WidgetReceipt = {
  blockNumber: number;
  blockHash: `0x${string}`;
};

export type WidgetActionKind = "claim" | "migrate";

export type TxModalState<
  TToken extends WidgetEligibleToken = WidgetEligibleToken,
> =
  | { kind: "closed" }
  | {
      kind: "awaiting_signature";
      action: WidgetActionKind;
      tokens: TToken[];
      variant?: string;
    }
  | {
      kind: "submitting";
      action: WidgetActionKind;
      tokens: TToken[];
      variant?: string;
    }
  | {
      kind: "polling";
      action: WidgetActionKind;
      tokens: TToken[];
      statusLabel: string;
      txHash?: `0x${string}`;
      variant?: string;
    }
  | {
      kind: "claim_success";
      tokens: TToken[];
      txHash?: `0x${string}`;
      chainId?: number;
      receipt?: WidgetReceipt;
    }
  | {
      kind: "migrate_success";
      tokens: TToken[];
      txHash?: `0x${string}`;
      chainId?: number;
      receipt?: WidgetReceipt;
    }
  | {
      kind: "error";
      action: WidgetActionKind;
      tokens: TToken[];
      stage: "signature" | "submit" | "poll";
      message: string;
      details?: string;
      variant?: string;
    };

export type WidgetState<
  TView extends AsyncViewState<LoadedViewWithTokens<TToken>>,
  TToken extends WidgetEligibleToken = WidgetEligibleToken,
> = {
  view: TView;
  actionStates: Record<string, TokenActionState>;
  modal: TxModalState<TToken>;
  connectWalletBusy: boolean;
  actionBusy: boolean;
};

export type WidgetAction<
  TView extends AsyncViewState<LoadedViewWithTokens<TToken>>,
  TAddress extends string = WidgetAddress,
  TToken extends WidgetEligibleToken = WidgetEligibleToken,
> =
  | { type: "set_view"; view: TView }
  | { type: "set_modal"; modal: TxModalState<TToken> }
  | { type: "set_connect_wallet_busy"; busy: boolean }
  | { type: "set_action_busy"; busy: boolean }
  | {
      type: "set_action_state";
      action: WidgetActionKind;
      tokenAddress: TAddress;
      variant?: string;
      next: TokenActionState;
    }
  | { type: "clear_action_states" }
  | { type: "mark_token_claimed"; tokenAddress: TAddress }
  | { type: "mark_token_migrated"; tokenAddress: TAddress };

export function createInitialWidgetState<
  TView extends AsyncViewState<LoadedViewWithTokens<TToken>>,
  TToken extends WidgetEligibleToken = WidgetEligibleToken,
>(): WidgetState<TView, TToken> {
  return {
    view: { kind: "idle" } as TView,
    actionStates: {},
    modal: { kind: "closed" },
    connectWalletBusy: false,
    actionBusy: false,
  };
}

export function actionKey<TAddress extends string>(
  action: WidgetActionKind,
  tokenAddress: TAddress,
  variant?: string,
): string {
  const suffix = variant ? `:${variant}` : "";
  return `${action}:${tokenAddress.toLowerCase()}${suffix}`;
}

export function actionStateFor<TAddress extends string>(
  actionStates: Record<string, TokenActionState>,
  action: WidgetActionKind,
  tokenAddress: TAddress,
  variant?: string,
): TokenActionState {
  return (
    actionStates[actionKey(action, tokenAddress, variant)] ?? { kind: "idle" }
  );
}

export function widgetReducer<
  TView extends AsyncViewState<LoadedViewWithTokens<TToken>>,
  TAddress extends string = WidgetAddress,
  TToken extends WidgetEligibleToken = WidgetEligibleToken,
>(
  state: WidgetState<TView, TToken>,
  action: WidgetAction<TView, TAddress, TToken>,
  sameAddress: (left: TAddress, right: TAddress) => boolean,
): WidgetState<TView, TToken> {
  if (action.type === "set_view") {
    return { ...state, view: action.view };
  }

  if (action.type === "set_modal") {
    return { ...state, modal: action.modal };
  }

  if (action.type === "set_connect_wallet_busy") {
    return { ...state, connectWalletBusy: action.busy };
  }

  if (action.type === "set_action_busy") {
    return { ...state, actionBusy: action.busy };
  }

  if (action.type === "set_action_state") {
    const key = actionKey(action.action, action.tokenAddress, action.variant);
    if (action.next.kind === "idle") {
      const next = { ...state.actionStates };
      delete next[key];
      return { ...state, actionStates: next };
    }

    return {
      ...state,
      actionStates: {
        ...state.actionStates,
        [key]: action.next,
      },
    };
  }

  if (action.type === "clear_action_states") {
    return { ...state, actionStates: {} };
  }

  if (action.type === "mark_token_claimed") {
    if (state.view.kind !== "loaded") return state;
    return {
      ...state,
      view: {
        ...state.view,
        tokens: state.view.tokens.map((token) =>
          sameAddress(token.tokenAddress as TAddress, action.tokenAddress)
            ? { ...token, claimed: true }
            : token,
        ),
      },
    };
  }

  if (action.type === "mark_token_migrated") {
    if (state.view.kind !== "loaded") return state;
    return {
      ...state,
      view: {
        ...state.view,
        tokens: state.view.tokens.map((token) =>
          sameAddress(token.tokenAddress as TAddress, action.tokenAddress)
            ? { ...token, migrated: true }
            : token,
        ),
      },
    };
  }

  return state;
}

export function isTerminalStatus(status: string): boolean {
  return status === "mined_success" || status === "mined_reverted";
}

export function isModalBusy(state: TxModalState<WidgetEligibleToken>): boolean {
  return (
    state.kind === "awaiting_signature" ||
    state.kind === "submitting" ||
    state.kind === "polling"
  );
}
