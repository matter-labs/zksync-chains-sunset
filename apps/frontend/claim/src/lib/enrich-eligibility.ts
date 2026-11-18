import {
  fetchClaimedFlags,
  fetchMigratedFlags,
} from "@shared/migration/onChain";
import { hasPositiveTokenAmount } from "@shared/ui/token-amount";

import type { ApiEligibleToken, EligibleToken } from "./api";

// Probes the migration contract and claim distributor for each eligibility
// row. The backend doesn't track on-chain state, so this is the only signal
// for `migrated` / `claimed`. RPC failures resolve to `false` (with a warn)
// so a flaky node doesn't block the claim button entirely.
export async function enrichEligibilityTokens(
  tokens: readonly ApiEligibleToken[],
): Promise<EligibleToken[]> {
  const visibleTokens = tokens.filter((token) =>
    hasPositiveTokenAmount(token.rawBalance),
  );
  if (visibleTokens.length === 0) return [];

  const migratedAddresses = visibleTokens.map((t) => t.tokenAddress);
  const claimIndices = visibleTokens.map((t) => BigInt(t.claimIndex));

  const [migratedFlags, claimedFlags] = await Promise.all([
    fetchMigratedFlags(migratedAddresses).catch((err) => {
      console.warn(
        "On-chain isTokenMigrated probe failed; reporting all tokens as not migrated.",
        err,
      );
      return new Map<string, boolean>();
    }),
    fetchClaimedFlags(claimIndices).catch((err) => {
      console.warn(
        "On-chain isClaimed probe failed; reporting all tokens as not claimed.",
        err,
      );
      return new Map<string, boolean>();
    }),
  ]);

  return visibleTokens.map((token) => ({
    ...token,
    migrated: migratedFlags.get(token.tokenAddress.toLowerCase()) ?? false,
    claimed: claimedFlags.get(BigInt(token.claimIndex).toString()) ?? false,
  }));
}
