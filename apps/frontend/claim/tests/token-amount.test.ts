import { describe, expect, it } from "bun:test";

import {
  formatExactTokenAmount,
  formatTokenAmount,
  hasPositiveTokenAmount,
} from "../../shared/src/ui/token-amount";

describe("token amount formatting", () => {
  it("uses two decimals for regular display values", () => {
    expect(formatTokenAmount("12345678900000000000", 18)).toMatchObject({
      displayBalance: "12.34",
      exactBalance: "12.3456789",
      isBelowDisplayFloor: false,
    });
    expect(formatTokenAmount("123450000000000000", 18)).toMatchObject({
      displayBalance: "0.12",
    });
    expect(formatTokenAmount("12345000000000000", 18)).toMatchObject({
      displayBalance: "0.01",
    });
    expect(formatTokenAmount("12", 0)).toMatchObject({
      displayBalance: "12.00",
      exactBalance: "12",
    });
    expect(formatTokenAmount("1", 1)).toMatchObject({
      displayBalance: "0.10",
      exactBalance: "0.1",
    });
  });

  it("expands small non-zero display values up to the display cap", () => {
    expect(formatTokenAmount("10000000000000", 18)).toMatchObject({
      displayBalance: "0.00001",
      exactBalance: "0.00001",
      isBelowDisplayFloor: false,
    });
    expect(formatTokenAmount("10000000000", 18)).toMatchObject({
      displayBalance: "0.00000001",
    });
    expect(formatTokenAmount("1", 8)).toMatchObject({
      displayBalance: "0.00000001",
      exactBalance: "0.00000001",
    });
    expect(formatTokenAmount("1", 6)).toMatchObject({
      displayBalance: "0.000001",
      exactBalance: "0.000001",
    });
  });

  it("uses a less-than floor for tiny values below the display cap", () => {
    expect(formatTokenAmount("1230000000", 18)).toMatchObject({
      displayBalance: "<0.00000001",
      exactBalance: "0.00000000123",
      isBelowDisplayFloor: true,
    });
  });

  it("formats exact balances without trailing zeroes", () => {
    expect(formatExactTokenAmount("0", 18)).toBe("0");
    expect(formatExactTokenAmount("1000000000000000000", 18)).toBe("1");
    expect(formatExactTokenAmount("10000000000000", 18)).toBe("0.00001");
  });

  it("detects zero and positive raw balances", () => {
    expect(hasPositiveTokenAmount("0")).toBe(false);
    expect(hasPositiveTokenAmount(0n)).toBe(false);
    expect(hasPositiveTokenAmount("1")).toBe(true);
    expect(hasPositiveTokenAmount(1n)).toBe(true);
  });
});
