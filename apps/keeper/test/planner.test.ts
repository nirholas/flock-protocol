import { describe, expect, it } from "vitest";
import { PublicKey } from "@solana/web3.js";
import type { IndexSnapshot } from "@flock/sdk";

import { DEFAULT_POLICY, plan } from "../src/planner.js";

/**
 * The fund in these cases is the one `program/tests/lifecycle.rs` rebalances on chain: ten index
 * tokens backed by 20 whole units of a $1 six-decimal component and 5 of a $100 nine-decimal one,
 * with an auction that wants to move $250 out of the second and into the first.
 */
const A = new PublicKey(new Uint8Array(32).fill(5));
const B = new PublicKey(new Uint8Array(32).fill(6));

function snapshot(overrides: Partial<IndexSnapshot["account"]["rebalance"]> = {}): IndexSnapshot {
  const rebalance = {
    active: true,
    proposer: A,
    proposedAt: 1_000,
    startTs: 2_000,
    endTs: 9_200,
    navFloorE6: 0n,
    startPremiumBps: -50,
    endPremiumBps: 150,
    maxNavLossBps: 100,
    ...overrides,
  };
  return {
    indexMint: A,
    address: B,
    supply: 10_000_000_000n,
    balances: [20_000_000n, 5_000_000_000n],
    units: [2_000_000n, 500_000_000n],
    pendingFee: 0n,
    asOf: 2_000,
    account: {
      indexMint: A,
      governor: A,
      manager: A,
      feeRecipient: A,
      lastFeeAccrual: 0,
      createdAt: 0,
      rebalanceDelay: 1_000,
      streamingFeeBps: 95,
      issueFeeBps: 0,
      redeemFeeBps: 0,
      maxPremiumBps: 300,
      version: 1,
      bump: 255,
      state: "live",
      decimals: 9,
      name: "Test",
      symbol: "T",
      components: [
        { mint: A, units: 2_000_000n, targetUnits: 27_000_000n, refPriceE9: 1_000_000_000n, decimals: 6, vaultBump: 255 },
        { mint: B, units: 500_000_000n, targetUnits: 250_000_000n, refPriceE9: 100_000_000_000n, decimals: 9, vaultBump: 255 },
      ],
      rebalance,
    },
  };
}

const marketPricesE9 = [1_000_000_000n, 100_000_000_000n];

describe("bidding", () => {
  it("does not bid at the open, where the auction is priced in the fund's favour", () => {
    const { actions, notes } = plan({
      snapshot: snapshot(),
      marketPricesE9,
      now: 2_000,
      policy: DEFAULT_POLICY,
      inventory: [1_000_000_000_000n, 0n],
    });
    expect(actions.filter((a) => a.kind === "bid")).toHaveLength(0);
    expect(notes.join(" ")).toMatch(/under the 25 bps floor/);
  });

  it("bids once the decaying premium clears the edge floor", () => {
    const { actions } = plan({
      snapshot: snapshot(),
      marketPricesE9,
      now: 9_200, // the close, where the premium is +150 bps
      policy: DEFAULT_POLICY,
      inventory: [1_000_000_000_000n, 0n],
    });
    const bid = actions.find((a) => a.kind === "bid");
    expect(bid).toBeDefined();
    if (bid?.kind !== "bid") throw new Error("no bid");
    expect(bid.sellComponent).toBe(1);
    expect(bid.buyComponent).toBe(0);
    expect(bid.premiumBps).toBe(150);
    expect(bid.edgeBps).toBeGreaterThanOrEqual(150);
    // The whole 2.5 units above target, paid for at 98.5% of reference value.
    expect(bid.sellAmount).toBe(2_500_000_000n);
    expect(bid.maxBuyAmount).toBe(246_250_000n);
  });

  it("shrinks the bid to the inventory it actually holds", () => {
    const { actions } = plan({
      snapshot: snapshot(),
      marketPricesE9,
      now: 9_200,
      policy: DEFAULT_POLICY,
      inventory: [100_000_000n, 0n], // only 100 whole units of the paying component
    });
    const bid = actions.find((a) => a.kind === "bid");
    if (bid?.kind !== "bid") throw new Error("no bid");
    expect(bid.maxBuyAmount).toBeLessThanOrEqual(100_000_000n);
    expect(bid.sellAmount).toBeLessThan(2_500_000_000n);
  });

  it("plans nothing when it holds nothing to pay with", () => {
    const { actions, notes } = plan({
      snapshot: snapshot(),
      marketPricesE9,
      now: 9_200,
      policy: DEFAULT_POLICY,
      inventory: [0n, 0n],
    });
    expect(actions).toHaveLength(0);
    expect(notes.join(" ")).toMatch(/no inventory/);
  });

  it("waits rather than bidding before the auction opens", () => {
    const { actions, notes } = plan({
      snapshot: snapshot(),
      marketPricesE9,
      now: 1_500,
      policy: DEFAULT_POLICY,
      inventory: [1_000_000_000_000n, 0n],
    });
    expect(actions).toHaveLength(0);
    expect(notes.join(" ")).toMatch(/auction opens in 500s/);
  });
});

describe("housekeeping", () => {
  it("closes an auction that has expired, and plans nothing else for it", () => {
    const { actions } = plan({
      snapshot: snapshot(),
      marketPricesE9,
      now: 9_201,
      policy: DEFAULT_POLICY,
      inventory: [1_000_000_000_000n, 0n],
    });
    expect(actions).toEqual([{ kind: "end", reason: "expired" }]);
  });

  it("accrues a fee worth the transaction, and leaves dust alone", () => {
    const big = { ...snapshot(), pendingFee: 5_000_000n };
    expect(
      plan({ snapshot: big, marketPricesE9, now: 1_500, policy: DEFAULT_POLICY, inventory: [0n, 0n] })
        .actions[0],
    ).toEqual({ kind: "accrue", pendingFee: 5_000_000n });

    const small = { ...snapshot(), pendingFee: 12n };
    const { actions, notes } = plan({
      snapshot: small,
      marketPricesE9,
      now: 1_500,
      policy: DEFAULT_POLICY,
      inventory: [0n, 0n],
    });
    expect(actions).toHaveLength(0);
    expect(notes.join(" ")).toMatch(/below the 1000000 floor/);
  });
});
