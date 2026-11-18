import {
  createInitialWidgetState,
  widgetReducer as sharedWidgetReducer,
} from "@shared/widget/state";

import type { EligibleToken } from "../../lib/api";
import { sameAddress } from "./utils";
import type { ViewState, WidgetAction, WidgetState } from "./types";

export const initialWidgetState: WidgetState = createInitialWidgetState<
  ViewState,
  EligibleToken
>();

export function widgetReducer(
  state: WidgetState,
  action: WidgetAction,
): WidgetState {
  return sharedWidgetReducer(state, action, sameAddress);
}
