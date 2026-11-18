import { useEffect, useEffectEvent, useRef, type RefObject } from "react";

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

type Options<TRoot extends HTMLElement, TInitial extends HTMLElement> = {
  isOpen: boolean;
  busy: boolean;
  rootRef: RefObject<TRoot | null>;
  initialFocusRef: RefObject<TInitial | null>;
  onRequestClose: () => void;
};

export function useModalAccessibility<
  TRoot extends HTMLElement,
  TInitial extends HTMLElement,
>({
  isOpen,
  busy,
  rootRef,
  initialFocusRef,
  onRequestClose,
}: Options<TRoot, TInitial>) {
  const lastFocusedRef = useRef<HTMLElement | null>(null);

  const requestClose = useEffectEvent(() => {
    if (!busy) {
      onRequestClose();
    }
  });

  useEffect(() => {
    if (!isOpen) return;

    lastFocusedRef.current = document.activeElement as HTMLElement | null;
    initialFocusRef.current?.focus();
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        requestClose();
        return;
      }

      if (event.key !== "Tab") return;
      const root = rootRef.current;
      if (!root) return;

      const focusable = root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement as HTMLElement | null;

      if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
      lastFocusedRef.current?.focus();
    };
  }, [isOpen]);
}
