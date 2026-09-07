import { describe, expect, it } from "vitest";

import {
  bidCost,
  premiumAt,
  streamingFeeTokens,
  unitsFromBalance,
  unitsForWeights,
  unitsIn,
  unitsOut,
  weightsBps,
} from "../src/math.js";
import { UNIT_SCALE } from "../src/program.js";

/**
 * These numbers are the ones `program/tests/lifecycle.rs` asserts on chain. Keeping the same cases
 * on both sides is what makes a quote from this SDK a promise the program will honour rather than
 * an estimate that fails at signature time.
 */
describe("issuance arithmetic", () => {
  it("rounds deliveries up and payouts down", () => {
    expect(unitsIn(1n, 333_333_333n)).toBe(1n);
    expect(unitsOut(1n, 333_333_333n)).toBe(0n);
  });

  it("prices the recipe the chain charged in the lifecycle test", () => {
    // 2 whole units of a 6-decimal component per index token, ten index tokens.
    expect(unitsIn(2_000_000n, 10n * UNIT_SCALE)).toBe(20_000_000n);
    expect(unitsIn(500_000_000n, 10n * UNIT_SCALE)).toBe(5_000_000_000n);
  });

  it("never lets derived units overclaim a vault", () => {
    for (const [balance, supply] of [
      [1_000_000n, 3_000_000n],
      [7n, 3n],
      [271_250_000n, 10n * UNIT_SCALE],
    ] as const) {
      const units = unitsFromBalance(balance, supply);
      expect((units * supply) / UNIT_SCALE).toBeLessThanOrEqual(balance);
    }
  });
});

describe("streaming fee", () => {
  it("charges a year of 1% as 1% of the grown supply", () => {
    const supply = 1_000_000_000_000n;
    const minted = streamingFeeTokens(supply, 100, 31_536_000);
    const shareBps = (minted * 10_000n) / (supply + minted);
    expect(shareBps).toBeGreaterThanOrEqual(99n);
    expect(shareBps).toBeLessThanOrEqual(100n);
  });

  it("is free when no time has passed", () => {
    expect(streamingFeeTokens(1_000n, 100, 0)).toBe(0n);
  });
});

describe("auction", () => {
  it("decays linearly and refuses to price a closed auction", () => {
    expect(premiumAt(-50, 150, 1_000, 2_000, 1_000)).toBe(-50);
    expect(premiumAt(-50, 150, 1_000, 2_000, 1_500)).toBe(50);
    expect(premiumAt(-50, 150, 1_000, 2_000, 2_000)).toBe(150);
    expect(() => premiumAt(-50, 150, 1_000, 2_000, 999)).toThrow();
  });

  it("quotes the fill the program accepted on chain", () => {
    // 2.5 whole units of a $100 9-decimal component, marked up 50 bps, paid in a $1 6-decimal one.
    const quote = bidCost({
      sellAmount: 2_500_000_000n,
      sellDecimals: 9,
      sellPriceE9: 100_000_000_000n,
      buyDecimals: 6,
      buyPriceE9: 1_000_000_000n,
      premiumBps: -50,
    });
    expect(quote.valueOutE9).toBe(250_000_000_000n);
    expect(quote.buyAmount).toBe(251_250_000n);
  });
});

describe("weights", () => {
  it("splits a fund by value, not by token count", () => {
    const { navE9, weights } = weightsBps(
      [20_000_000n, 5_000_000_000n],
      [6, 9],
      [1_000_000_000n, 100_000_000_000n],
    );
    expect(navE9).toBe(520_000_000_000n);
    expect(weights).toEqual([384, 9615]);
  });

  it("turns a target weighting back into units the program can take", () => {
    const units = unitsForWeights({
      weightsBps: [5_000, 5_000],
      pricesE9: [1_000_000_000n, 100_000_000_000n],
      decimals: [6, 9],
      navE9: 520_000_000_000n,
      supply: 10n * UNIT_SCALE,
    });
    // Half of $520 is $260: 260 whole units of the $1 token, 2.6 of the $100 one, per 10 tokens.
    expect(units).toEqual([26_000_000n, 260_000_000n]);
  });
});
