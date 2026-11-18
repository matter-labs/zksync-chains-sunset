import { getBalance, multicall } from "wagmi/actions";
import { erc20Abi } from "viem";

import { getWalletConfig } from "@shared/wallet/config";
import {
  ETH_SENTINEL,
  fetchMigratedFlags,
  resolveMigrationChainId,
  resolveMigrationContract,
} from "@shared/migration/onChain";
import {
  formatTokenAmount,
  hasPositiveTokenAmount,
} from "@shared/ui/token-amount";

import type { Address, EligibleToken, MigrationToken } from "./api";

function sameAddress(a: Address, b: Address): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

export async function enrichMigrationTokens(
  tokens: MigrationToken[],
): Promise<EligibleToken[]> {
  if (tokens.length === 0) return [];

  const contract = resolveMigrationContract();
  const chainId = resolveMigrationChainId();
  const config = getWalletConfig();

  const ethIndices = tokens
    .map((token, index) => ({ token, index }))
    .filter(({ token }) => sameAddress(token.tokenAddress, ETH_SENTINEL));

  const erc20Tokens = tokens
    .map((token, index) => ({ token, index }))
    .filter(({ token }) => !sameAddress(token.tokenAddress, ETH_SENTINEL));

  const balanceCalls = erc20Tokens.map(({ token }) => ({
    address: token.tokenAddress,
    abi: erc20Abi,
    functionName: "balanceOf" as const,
    args: [contract] as const,
  }));
  const allAddresses = tokens.map((token) => token.tokenAddress);

  const [erc20Balances, migratedFlags, ethBalance] = await Promise.all([
    balanceCalls.length > 0
      ? multicall(config, {
          chainId,
          allowFailure: false,
          contracts: balanceCalls,
        })
      : Promise.resolve([] as bigint[]),
    fetchMigratedFlags(allAddresses),
    ethIndices.length > 0
      ? getBalance(config, { chainId, address: contract }).then((r) => r.value)
      : Promise.resolve(0n),
  ]);

  const rawByIndex = new Map<number, bigint>();
  erc20Tokens.forEach(({ index }, cursor) => {
    rawByIndex.set(index, erc20Balances[cursor] as bigint);
  });
  ethIndices.forEach(({ index }) => {
    rawByIndex.set(index, ethBalance);
  });

  return tokens.flatMap((token, index) => {
    const rawBalance = rawByIndex.get(index) ?? 0n;
    if (!hasPositiveTokenAmount(rawBalance)) return [];

    const migrated =
      migratedFlags.get(token.tokenAddress.toLowerCase()) ?? false;
    const { displayBalance } = formatTokenAmount(rawBalance, token.decimals);

    return [
      {
        tokenAddress: token.tokenAddress,
        symbol: token.symbol,
        decimals: token.decimals,
        rawBalance: rawBalance.toString(),
        displayBalance,
        migrated,
        claimed: false,
      },
    ];
  });
}
