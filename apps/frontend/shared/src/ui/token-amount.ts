const STANDARD_DISPLAY_DECIMALS = 2;
const MAX_ADAPTIVE_DISPLAY_DECIMALS = 8;

export type FormattedTokenAmount = {
  displayBalance: string;
  exactBalance: string;
  isBelowDisplayFloor: boolean;
};

export function formatTokenAmount(
  rawBalance: bigint | string,
  decimals: number,
): FormattedTokenAmount {
  const raw = parseRawBalance(rawBalance);
  const tokenDecimals = normalizeDecimals(decimals);
  const exactBalance = formatExactTokenAmount(raw, tokenDecimals);

  if (raw === 0n) {
    return {
      displayBalance: "0.00",
      exactBalance,
      isBelowDisplayFloor: false,
    };
  }

  if (tokenDecimals === 0) {
    return {
      displayBalance: `${raw.toString()}.00`,
      exactBalance,
      isBelowDisplayFloor: false,
    };
  }

  if (isAtLeastOneHundredth(raw, tokenDecimals)) {
    return {
      displayBalance: formatFixedDecimal(raw, tokenDecimals),
      exactBalance,
      isBelowDisplayFloor: false,
    };
  }

  const displayDecimals = Math.min(
    tokenDecimals,
    MAX_ADAPTIVE_DISPLAY_DECIMALS,
  );

  if (isBelowDisplayFloor(raw, tokenDecimals, displayDecimals)) {
    return {
      displayBalance: `<${formatDisplayFloor(displayDecimals)}`,
      exactBalance,
      isBelowDisplayFloor: true,
    };
  }

  return {
    displayBalance: formatTrimmedDecimal(raw, tokenDecimals, displayDecimals),
    exactBalance,
    isBelowDisplayFloor: false,
  };
}

export function formatExactTokenAmount(
  rawBalance: bigint | string,
  decimals: number,
): string {
  const raw = parseRawBalance(rawBalance);
  const tokenDecimals = normalizeDecimals(decimals);

  if (raw === 0n) return "0";
  if (tokenDecimals === 0) return raw.toString();

  return formatTrimmedDecimal(raw, tokenDecimals, tokenDecimals);
}

export function hasPositiveTokenAmount(rawBalance: bigint | string): boolean {
  return parseRawBalance(rawBalance) > 0n;
}

function parseRawBalance(rawBalance: bigint | string): bigint {
  if (typeof rawBalance === "bigint") return rawBalance;
  return BigInt(rawBalance);
}

function normalizeDecimals(decimals: number): number {
  if (!Number.isFinite(decimals) || decimals < 0) {
    throw new Error(`Invalid token decimals: ${decimals}`);
  }

  return Math.trunc(decimals);
}

function isAtLeastOneHundredth(raw: bigint, decimals: number): boolean {
  if (decimals < STANDARD_DISPLAY_DECIMALS) return true;
  return raw >= 10n ** BigInt(decimals - STANDARD_DISPLAY_DECIMALS);
}

function isBelowDisplayFloor(
  raw: bigint,
  decimals: number,
  displayDecimals: number,
): boolean {
  return raw < 10n ** BigInt(decimals - displayDecimals);
}

function decimalParts(
  raw: bigint,
  decimals: number,
): { whole: string; fractional: string } {
  const rawText = raw.toString();
  const padded = rawText.padStart(decimals + 1, "0");
  const splitAt = padded.length - decimals;

  return {
    whole: padded.slice(0, splitAt),
    fractional: padded.slice(splitAt),
  };
}

function formatFixedDecimal(raw: bigint, decimals: number): string {
  const { whole, fractional } = decimalParts(raw, decimals);
  const visibleFractional = fractional
    .slice(0, STANDARD_DISPLAY_DECIMALS)
    .padEnd(STANDARD_DISPLAY_DECIMALS, "0");

  return `${whole}.${visibleFractional}`;
}

function formatTrimmedDecimal(
  raw: bigint,
  decimals: number,
  displayDecimals: number,
): string {
  const { whole, fractional } = decimalParts(raw, decimals);
  const visibleFractional = fractional
    .slice(0, displayDecimals)
    .replace(/0+$/, "");

  return visibleFractional.length === 0
    ? whole
    : `${whole}.${visibleFractional}`;
}

function formatDisplayFloor(displayDecimals: number): string {
  if (displayDecimals === 0) return "1";
  return `0.${"0".repeat(displayDecimals - 1)}1`;
}
