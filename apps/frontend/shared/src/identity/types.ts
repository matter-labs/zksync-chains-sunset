export type Address = `0x${string}`;

export type IdentityState =
  | { source: "none"; address: null }
  | { source: "wallet"; address: Address }
  | { source: "selected_address"; address: Address };

export const emptyIdentityState: IdentityState = {
  source: "none",
  address: null,
};

export function shortenAddress(addr: string): string {
  if (addr.length < 10) return addr;
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
}
