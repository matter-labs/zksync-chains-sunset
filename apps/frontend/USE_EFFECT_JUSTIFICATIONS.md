# Remaining `useEffect` Justifications

These are the `useEffect` calls left after removing prop-sync, derived-state, and event-driven uses in the frontend. Each remaining effect exists to synchronize React state with an external system or to clean up external resources.

## `shared/src/wallet/session.tsx`

- `WalletSessionProvider`: initializes the shared wagmi connector config, watches connector/connection state, restores remembered wallet state from `localStorage`, and unsubscribes from external wallet-store listeners on cleanup. This is direct synchronization with browser wallet connectors and the external wagmi store.

## `claim/src/components/claim-widget/useClaimWidgetController.ts`

- Wallet auto-check effect: when the connected wallet address changes or silently reconnects, it triggers a fresh eligibility request for that externally-controlled wallet identity. The wallet provider is outside React, and the follow-up API request must happen after that external change.

## `claim/src/components/ClaimApp.tsx`

- Claim config fetch: after the external `/readyz` probe reports the backend is available, fetches `/api/config` and aborts the in-flight request if availability changes or the component unmounts. The result resolves the user-facing withdraw mode.

## `shared/src/widget/useControllerAsyncSlots.ts`

- Shared async-slot cleanup effect: aborts in-flight eligibility/action requests and marks the controller as unmounted so stale async continuations cannot update dead widgets. This is centralized cleanup for external network work that both widgets share.

## `shared/src/ui/useModalAccessibility.ts`

- Shared modal DOM-sync effect: focuses the modal, traps keyboard navigation, closes on `Escape`, locks body scroll, and restores focus when the modal closes. This is direct synchronization with the document and focus system, and it is centralized in a custom hook so the imperative behavior lives at the UI boundary instead of being duplicated in both modal components.

## `claim/src/lib/useApiAvailability.ts`

- API reachability probe: on mount, sends a probe request to the backend and aborts the in-flight fetch on unmount. The result drives whether the claim widget renders or a "claiming unavailable" banner shows. The network probe is an external system; the effect synchronizes its outcome into React state.
