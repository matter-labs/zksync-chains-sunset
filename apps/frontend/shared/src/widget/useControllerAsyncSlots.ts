import { useEffect, useRef, type MutableRefObject } from "react";

type SlotState = {
  currentId: number | null;
  sequence: number;
  controller: AbortController | null;
};

type SlotTicket = {
  id: number;
  signal: AbortSignal;
};

type SlotApi = {
  begin: () => SlotTicket;
  cancel: () => void;
  clear: () => void;
  invalidate: () => void;
  isCurrent: (id: number) => boolean;
};

function createSlotState(): SlotState {
  return {
    currentId: null,
    sequence: 0,
    controller: null,
  };
}

function abortSlot(slot: SlotState) {
  slot.controller?.abort();
  slot.controller = null;
}

function slotApi(slotRef: MutableRefObject<SlotState>): SlotApi {
  return {
    begin() {
      const slot = slotRef.current;
      if (!slot) {
        throw new Error("Async controller slot is unavailable.");
      }
      slot.sequence += 1;
      const id = slot.sequence;
      slot.currentId = id;
      abortSlot(slot);
      const controller = new AbortController();
      slot.controller = controller;
      return { id, signal: controller.signal };
    },
    cancel() {
      const slot = slotRef.current;
      if (!slot) return;
      abortSlot(slot);
    },
    clear() {
      const slot = slotRef.current;
      if (!slot) return;
      slot.currentId = null;
      abortSlot(slot);
    },
    invalidate() {
      const slot = slotRef.current;
      if (!slot) return;
      slot.sequence += 1;
      slot.currentId = null;
    },
    isCurrent(id) {
      return slotRef.current?.currentId === id;
    },
  };
}

export function useControllerAsyncSlots() {
  const mountedRef = useRef(true);
  const checkSlotRef = useRef<SlotState>(createSlotState());
  const actionSlotRef = useRef<SlotState>(createSlotState());

  useEffect(() => {
    return () => {
      mountedRef.current = false;
      abortSlot(checkSlotRef.current);
      abortSlot(actionSlotRef.current);
      checkSlotRef.current.currentId = null;
      actionSlotRef.current.currentId = null;
    };
  }, []);

  return {
    mountedRef,
    check: slotApi(checkSlotRef),
    action: slotApi(actionSlotRef),
  };
}
