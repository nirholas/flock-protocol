import { BPS, SECONDS_PER_YEAR, UNIT_SCALE } from "./program.js";

/**
 * The same arithmetic the program runs, in the same rounding direction.
 *
 * A client that rounds the other way quotes a user a number the chain then rejects, so these are
 * deliberate mirrors of `program/src/math.rs` rather than convenient approximations. Every one of
 * them is checked against the on-chain result in the SDK tests.
 */

export function mulDivFloor(a: bigint, b: bigint, d: bigint): bigint {
  if (d === 0n) throw new Error("division by zero");
  return (a * b) / d;
}

export function mulDivCeil(a: bigint, b: bigint, d: bigint): bigint {
  if (d === 0n) throw new Error("division by zero");
  const n = a * b;
  return n === 0n ? 0n : (n - 1n) / d + 1n;
}

/** Component base units a caller must deliver to mint `amount` index base units. */
export function unitsIn(units: bigint, amount: bigint): bigint {
  return mulDivCeil(units, amount, UNIT_SCALE);
}

/** Component base units a caller receives for burning `amount` index base units. */
export function unitsOut(units: bigint, amount: bigint): bigint {
  return mulDivFloor(units, amount, UNIT_SCALE);
}

/** What a component's units actually are, given what its vault holds right now. */
export function unitsFromBalance(balance: bigint, supply: bigint): bigint {
  if (supply === 0n) return 0n;
  return mulDivFloor(balance, UNIT_SCALE, supply);
}

/** USD value in 1e9 of `balance` base units of a token priced at `priceE9` per whole token. */
export function valueE9(balance: bigint, decimals: number, priceE9: bigint): bigint {
  return mulDivFloor(balance, priceE9, 10n ** BigInt(decimals));
}

export function amountFromValueCeil(value: bigint, decimals: number, priceE9: bigint): bigint {
  if (priceE9 === 0n) throw new Error("a price of zero cannot value anything");
  return mulDivCeil(value, 10n ** BigInt(decimals), priceE9);
}

/** Index tokens minted as a streaming fee over `dtSeconds`, charged as dilution. */
export function streamingFeeTokens(supply: bigint, feeBps: number, dtSeconds: number): bigint {
  if (supply === 0n || feeBps === 0 || dtSeconds <= 0) return 0n;
  const num = BigInt(feeBps) * BigInt(dtSeconds);
  const den = BPS * SECONDS_PER_YEAR;
  if (num >= den) throw new Error("fee period overflows");
  return mulDivFloor(supply, num, den - num);
}

/**
 * The auction discount in force at `now`, in bps. Negative means the bidder pays the fund above
 * reference value; positive means the bidder is paid to take the trade.
 */
export function premiumAt(
  startBps: number,
  endBps: number,
  startTs: number,
  endTs: number,
  now: number,
): number {
  if (now < startTs || now > endTs) throw new Error("the auction is not open");
  if (endTs <= startTs) throw new Error("the auction window is inverted");
  const span = endTs - startTs;
  const elapsed = now - startTs;
  return startBps + Math.trunc(((endBps - startBps) * elapsed) / span);
}

/** What a bidder must pay to take `sellAmount` of one component out of the fund. */
export function bidCost(args: {
  sellAmount: bigint;
  sellDecimals: number;
  sellPriceE9: bigint;
  buyDecimals: number;
  buyPriceE9: bigint;
  premiumBps: number;
}): { valueOutE9: bigint; valueInE9: bigint; buyAmount: bigint } {
  const valueOutE9 = valueE9(args.sellAmount, args.sellDecimals, args.sellPriceE9);
  const marked = BPS - BigInt(args.premiumBps);
  const valueInE9 = mulDivCeil(valueOutE9, marked, BPS);
  return {
    valueOutE9,
    valueInE9,
    buyAmount: amountFromValueCeil(valueInE9, args.buyDecimals, args.buyPriceE9),
  };
}

/** Portfolio weights in bps, given balances and prices. Sums to 10000 up to rounding. */
export function weightsBps(
  balances: bigint[],
  decimals: number[],
  pricesE9: bigint[],
): { navE9: bigint; weights: number[] } {
  const values = balances.map((balance, i) =>
    valueE9(balance, decimals[i] ?? 0, pricesE9[i] ?? 0n),
  );
  const navE9 = values.reduce((a, b) => a + b, 0n);
  if (navE9 === 0n) return { navE9, weights: values.map(() => 0) };
  return { navE9, weights: values.map((v) => Number((v * BPS) / navE9)) };
}

/**
 * Target units that put `weights` (in bps) on a fund of `navE9` with `supply` outstanding.
 *
 * This is the whole translation from a methodology's answer ("35% of the fund in JUP") into the
 * number the program takes ("this many base units of JUP behind each index token").
 */
export function unitsForWeights(args: {
  weightsBps: number[];
  pricesE9: bigint[];
  decimals: number[];
  navE9: bigint;
  supply: bigint;
}): bigint[] {
  if (args.supply === 0n) throw new Error("cannot size units against an empty fund");
  return args.weightsBps.map((weight, i) => {
    const price = args.pricesE9[i] ?? 0n;
    if (price === 0n) throw new Error(`component ${i} has no price`);
    const targetValueE9 = mulDivFloor(args.navE9, BigInt(weight), BPS);
    const targetAmount = mulDivFloor(targetValueE9, 10n ** BigInt(args.decimals[i] ?? 0), price);
    return mulDivFloor(targetAmount, UNIT_SCALE, args.supply);
  });
}
