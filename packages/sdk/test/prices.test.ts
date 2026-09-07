import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

import {
  fetchPrices,
  fetchTokenStats,
  parsePriceResponse,
  parseTokenResponse,
  toPriceE9,
} from "../src/prices.js";

/**
 * The fixtures are real responses, recorded from the live endpoints on 2026-09-07, not invented
 * shapes. The live test below hits the API for real and is opt-in, so the suite stays runnable
 * offline without ever pretending an unreachable API answered.
 */
const priceFixture = JSON.parse(
  readFileSync(new URL("./fixtures/jup-price-v3.json", import.meta.url), "utf8"),
);
const tokenFixture = JSON.parse(
  readFileSync(new URL("./fixtures/jup-tokens-v2.json", import.meta.url), "utf8"),
);

const JUP = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";
const BONK = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";

describe("price conversion", () => {
  it("keeps nine decimals of a dollar price", () => {
    expect(toPriceE9(1)).toBe(1_000_000_000n);
    expect(toPriceE9(0.25232647249031087)).toBe(252_326_472n);
  });

  it("never rounds a real price down to zero, which would make a component worthless", () => {
    expect(toPriceE9(1e-12)).toBe(1n);
    expect(() => toPriceE9(0)).toThrow();
    expect(() => toPriceE9(Number.NaN)).toThrow();
  });
});

describe("parsing a real price response", () => {
  const parsed = parsePriceResponse(priceFixture);

  it("prices every mint it was asked about", () => {
    expect(parsed.size).toBe(3);
    expect(parsed.get(JUP)?.priceE9).toBeGreaterThan(0n);
    expect(parsed.get(BONK)?.decimals).toBe(5);
  });

  it("carries the liquidity a keeper needs to size a bid", () => {
    expect(parsed.get(BONK)!.liquidityUsd).toBeGreaterThan(0);
  });
});

describe("parsing a real token response", () => {
  const parsed = parseTokenResponse(tokenFixture);

  it("reads the screening fields a methodology depends on", () => {
    const jup = parsed.get(JUP)!;
    expect(jup.symbol).toBe("JUP");
    expect(jup.decimals).toBe(6);
    expect(jup.marketCapUsd).toBeGreaterThan(0);
    expect(jup.liquidityUsd).toBeGreaterThan(0);
    expect(jup.holderCount).toBeGreaterThan(0);
    expect(jup.organicScore).toBeGreaterThan(0);
    expect(jup.mintAuthorityDisabled).toBe(true);
    expect(jup.freezeAuthorityDisabled).toBe(true);
    expect(jup.tags).toContain("verified");
    expect(jup.firstPoolCreatedAt).toBeTruthy();
  });

  it("sums buy and sell volume into one 24h figure", () => {
    const bonk = parsed.get(BONK)!;
    expect(bonk.volume24hUsd).toBeGreaterThanOrEqual(bonk.organicVolume24hUsd);
  });
});

describe.runIf(process.env.FLOCK_LIVE === "1")("against the live API", () => {
  it("prices JUP and BONK right now", async () => {
    const quotes = await fetchPrices([JUP, BONK]);
    expect(quotes.get(JUP)?.priceE9).toBeGreaterThan(0n);
    expect(quotes.get(BONK)?.priceE9).toBeGreaterThan(0n);
  });

  it("returns screening data right now", async () => {
    const stats = await fetchTokenStats([JUP]);
    expect(stats.get(JUP)?.marketCapUsd).toBeGreaterThan(0);
  });
});
