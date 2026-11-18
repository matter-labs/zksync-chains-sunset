import type { Address } from "./api";
import { getAddress, isAddress } from "viem";

export function validateAddress(
  value: string,
): { ok: true; address: Address } | { ok: false; message: string } {
  const trimmed = value.trim();
  if (!trimmed) return { ok: false, message: "Enter an address to continue." };

  // First, ensure the address is structurally valid.
  if (!isAddress(trimmed, { strict: false })) {
    return {
      ok: false,
      message:
        "Invalid address. Expected a 0x-prefixed, 40-hex-character Ethereum address.",
    };
  }

  // If the user supplied mixed-case, also validate the checksum and give a
  // higher-signal error message.
  const isMixedCase =
    trimmed !== trimmed.toLowerCase() && trimmed !== trimmed.toUpperCase();
  if (isMixedCase && !isAddress(trimmed, { strict: true })) {
    return {
      ok: false,
      message:
        "Address checksum looks incorrect. Please re-copy the address from a trusted source.",
    };
  }

  // Normalize to a checksummed address for display & transport.
  const normalized = getAddress(trimmed) as Address;
  return { ok: true, address: normalized };
}

export function shortenAddress(addr: string): string {
  if (addr.length < 10) return addr;
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
}
