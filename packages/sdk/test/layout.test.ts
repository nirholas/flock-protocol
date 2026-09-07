import { describe, expect, it } from "vitest";

import { decodeIndex, INDEX_ACCOUNT_LEN } from "../src/state.js";
import { fixture } from "./fixture.js";

/**
 * The decoder reads the account by byte offset, which is fast and completely unforgiving: one
 * field added on the Rust side shifts everything after it. The fixture is an account the program
 * itself laid out, with every field set to a different value, so a shifted read is visible.
 */
describe("index account layout", () => {
  const data = Buffer.from(fixture.indexAccountHex, "hex");

  it("is the size the program allocates", () => {
    expect(data.length).toBe(INDEX_ACCOUNT_LEN);
  });

  it("decodes every field to the value the program wrote", () => {
    const index = decodeIndex(data);
    expect(index.name).toBe("Flock DeFi Index");
    expect(index.symbol).toBe("FDI");
    expect(index.state).toBe("live");
    expect(index.decimals).toBe(9);
    expect(index.bump).toBe(254);
    expect(index.streamingFeeBps).toBe(95);
    expect(index.issueFeeBps).toBe(10);
    expect(index.redeemFeeBps).toBe(15);
    expect(index.maxPremiumBps).toBe(300);
    expect(index.rebalanceDelay).toBe(86_400);
    expect(index.lastFeeAccrual).toBe(1_767_225_600);
    expect(index.createdAt).toBe(1_756_684_800);
    expect(index.indexMint.toBytes()[0]).toBe(1);
    expect(index.governor.toBytes()[0]).toBe(2);
    expect(index.manager.toBytes()[0]).toBe(3);
    expect(index.feeRecipient.toBytes()[0]).toBe(4);

    expect(index.components).toHaveLength(2);
    expect(index.components[0]!.mint.toBytes()[0]).toBe(5);
    expect(index.components[0]!.units).toBe(1_234_567n);
    expect(index.components[0]!.targetUnits).toBe(2_000_000n);
    expect(index.components[0]!.refPriceE9).toBe(1_050_000_000n);
    expect(index.components[0]!.decimals).toBe(6);
    expect(index.components[1]!.units).toBe(987_654_321n);
    expect(index.components[1]!.refPriceE9).toBe(103_930_000_000n);
    expect(index.components[1]!.decimals).toBe(9);

    expect(index.rebalance.active).toBe(true);
    expect(index.rebalance.startTs).toBe(1_767_086_400);
    expect(index.rebalance.endTs).toBe(1_767_093_600);
    expect(index.rebalance.navFloorE6).toBe(998_500_000n);
    expect(index.rebalance.startPremiumBps).toBe(-50);
    expect(index.rebalance.endPremiumBps).toBe(150);
    expect(index.rebalance.maxNavLossBps).toBe(100);
  });

  it("refuses an account that is not an index", () => {
    expect(() => decodeIndex(Buffer.alloc(INDEX_ACCOUNT_LEN))).toThrow(/wrong tag/);
  });
});
