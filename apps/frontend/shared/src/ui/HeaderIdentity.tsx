import "./header-identity.css";

import type { IdentityState } from "../identity/types";
import { shortenAddress } from "../identity/types";

type Props = {
  identity: IdentityState;
  onClearIdentity: () => void;
};

function identityLabel(identity: IdentityState): string {
  if (identity.source === "wallet") return "Connected wallet";
  if (identity.source === "selected_address") return "Selected address";
  return "";
}

function identityActionLabel(identity: IdentityState): string {
  if (identity.source === "wallet") return "Disconnect";
  if (identity.source === "selected_address") return "Clear address";
  return "";
}

export function HeaderIdentity({ identity, onClearIdentity }: Props) {
  if (identity.source === "none" || identity.address === null) return null;

  return (
    <div
      className="header-identity"
      data-identity-source={identity.source}
      data-role="header-identity"
    >
      <div className="header-identity__meta">
        <span className="header-identity__label">
          {identityLabel(identity)}
        </span>
        <span className="header-identity__address">
          {shortenAddress(identity.address)}
        </span>
      </div>
      <button
        className="header-identity__action"
        type="button"
        data-action="clear-identity"
        onClick={onClearIdentity}
      >
        {identityActionLabel(identity)}
      </button>
    </div>
  );
}
