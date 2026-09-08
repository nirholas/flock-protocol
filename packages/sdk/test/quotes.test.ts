import { describe, expect, it } from "vitest";
import { PublicKey } from "@solana/web3.js";

import { FlockClient, type IndexSnapshot } from "../src/client.js";
import { UNIT_SCALE } from "../src/program.js";
import { key } from "./fixture.js";

/**
 * Quoting a first issuance.
 *
 * Before anyone has issued, the vaults are empty and there is no supply to divide by, so units
 * cannot be derived from balances: the stored recipe is the answer. Getting this wrong quotes zero
 * for every component, and the chain then rejects the transaction on the caller's own slippage
 * bound, which is what happened the first time this ran against a real cluster.
 */
function emptySnapshot(): IndexSnapshot {
  const indexMint = key(1);
  return {
    indexMint,
    address: key(2),
    supply: 0n,
    balances: [0n, 0n],
    units: [2_000_000n, 500_000_000n],
    pendingFee: 0n,
    asOf: 1_767_225_600,
    account: {
      indexMint,
      governor: key(3),
      manager: key(3),
      feeRecipient: key(4),
      lastFeeAccrual: 1_767_225_600,
      createdAt: 1_767_225_600,
      rebalanceDelay: 300,
      streamingFeeBps: 95,
      issueFeeBps: 0,
      redeemFeeBps: 0,
      maxPremiumBps: 300,
      version: 1,
      bump: 255,
      state: "live",
      decimals: 9,
      name: "Local Test Index",
      symbol: "LTI",
      components: [
        { mint: key(5), units: 2_000_000n, targetUnits: 2_000_000n, refPriceE9: 0n, decimals: 6, vaultBump: 255 },
        { mint: key(6), units: 500_000_000n, targetUnits: 500_000_000n, refPriceE9: 0n, decimals: 9, vaultBump: 255 },
      ],
      rebalance: {
        active: false,
        proposer: PublicKey.default,
        proposedAt: 0,
        startTs: 0,
        endTs: 0,
        navFloorE6: 0n,
        startPremiumBps: 0,
        endPremiumBps: 0,
        maxNavLossBps: 0,
      },
    },
  };
}

describe("first issuance", () => {
  const client = new FlockClient(undefined as never, key(9));

  it("quotes the seed recipe rather than zero", () => {
    const { required } = client.quoteIssue(emptySnapshot(), 10n * UNIT_SCALE);
    // Ten index tokens at 2 whole units of a 6-decimal component and 0.5 of a 9-decimal one.
    expect(required).toEqual([20_000_000n, 5_000_000_000n]);
  });

  it("still derives from the vaults once there is a supply", () => {
    const snapshot = emptySnapshot();
    snapshot.supply = 10n * UNIT_SCALE;
    snapshot.balances = [20_000_000n, 5_000_000_000n];
    const { required } = client.quoteIssue(snapshot, 10n * UNIT_SCALE);
    expect(required).toEqual([20_000_000n, 5_000_000_000n]);
  });
});
