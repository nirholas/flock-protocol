import { describe, expect, it } from "vitest";

import { parseComponents, parseWeights } from "../src/commands.js";

describe("weight parsing", () => {
  const a = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";
  const b = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";

  it("accepts a fully allocated weighting", () => {
    const weights = parseWeights(`${a}=6000,${b}=4000`);
    expect(weights.map((w) => w.bps)).toEqual([6000, 4000]);
    expect(weights[0]!.mint.toBase58()).toBe(a);
  });

  it("refuses one that does not add up, rather than silently normalising it", () => {
    expect(() => parseWeights(`${a}=6000,${b}=3000`)).toThrow(/9000 bps/);
  });

  it("reads whole-unit component specs", () => {
    expect(parseComponents(`${a}:12.5`)[0]!.whole).toBe(12.5);
    expect(() => parseComponents(`${a}`)).toThrow(/expected MINT:WHOLE_UNITS/);
  });
});
