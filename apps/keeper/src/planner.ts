import {
  BPS,
  UNIT_SCALE,
  bidCost,
  premiumAt,
  type IndexSnapshot,
} from "@flock/sdk";

/**
 * The keeper's decisions, as pure functions.
 *
 * Every judgement about what to do with an index is made here, from a snapshot and a set of market
 * prices, with no clock of its own and no network. That is what makes the keeper testable: the
 * interesting cases (an auction that is not yet worth taking, a fee too small to be worth the
 * transaction fee) are exactly the ones that are painful to reproduce against a live cluster.
 */

export interface KeeperPolicy {
  /** Do not pay a transaction fee to mint less than this many index base units. */
  minAccrualTokens: bigint;
  /** Do not bid unless the market edge covers this, after which the bidder still pays fees. */
  minEdgeBps: number;
  /** Never take more than this share of a leg's remaining size in one bid. */
  maxLegShareBps: number;
}

export const DEFAULT_POLICY: KeeperPolicy = {
  minAccrualTokens: 1_000_000n,
  minEdgeBps: 25,
  maxLegShareBps: 10_000,
};

export type Action =
  | { kind: "accrue"; pendingFee: bigint }
  | { kind: "end"; reason: "expired" }
  | {
      kind: "bid";
      sellComponent: number;
      buyComponent: number;
      sellAmount: bigint;
      maxBuyAmount: bigint;
      premiumBps: number;
      edgeBps: number;
      edgeUsd: number;
    };

export interface PlanInput {
  snapshot: IndexSnapshot;
  /** Live USD price per whole token, in table order. */
  marketPricesE9: bigint[];
  now: number;
  policy: KeeperPolicy;
  /** What the keeper actually holds, in table order. Bids are only planned against inventory. */
  inventory: bigint[];
}

export interface Plan {
  actions: Action[];
  /** Everything considered and rejected, so a quiet keeper can still explain itself. */
  notes: string[];
}

function usd(valueE9: bigint): number {
  return Number(valueE9) / 1e9;
}

export function plan(input: PlanInput): Plan {
  const { snapshot, now, policy } = input;
  const actions: Action[] = [];
  const notes: string[] = [];
  const { account } = snapshot;

  if (snapshot.pendingFee >= policy.minAccrualTokens) {
    actions.push({ kind: "accrue", pendingFee: snapshot.pendingFee });
  } else if (snapshot.pendingFee > 0n) {
    notes.push(`fee of ${snapshot.pendingFee} is below the ${policy.minAccrualTokens} floor`);
  }

  if (account.rebalance.active && now > account.rebalance.endTs) {
    actions.push({ kind: "end", reason: "expired" });
    return { actions, notes };
  }
  if (!account.rebalance.active) return { actions, notes };
  if (now < account.rebalance.startTs) {
    notes.push(`auction opens in ${account.rebalance.startTs - now}s`);
    return { actions, notes };
  }

  const premiumBps = premiumAt(
    account.rebalance.startPremiumBps,
    account.rebalance.endPremiumBps,
    account.rebalance.startTs,
    account.rebalance.endTs,
    now,
  );

  for (let sellIndex = 0; sellIndex < account.components.length; sellIndex += 1) {
    const sell = account.components[sellIndex]!;
    const sellBalance = snapshot.balances[sellIndex] ?? 0n;
    const sellTarget = (sell.targetUnits * snapshot.supply) / UNIT_SCALE;
    if (sellBalance <= sellTarget) continue;
    const sellable = sellBalance - sellTarget;

    for (let buyIndex = 0; buyIndex < account.components.length; buyIndex += 1) {
      if (buyIndex === sellIndex) continue;
      const buy = account.components[buyIndex]!;
      const buyBalance = snapshot.balances[buyIndex] ?? 0n;
      const buyTarget = (buy.targetUnits * snapshot.supply) / UNIT_SCALE;
      const buyCap = buyTarget + (buyTarget * BigInt(account.maxPremiumBps)) / BPS;
      if (buyBalance >= buyCap) continue;
      const buyable = buyCap - buyBalance;

      // Size the bid so that neither leg is pushed past what the program will accept, then trim
      // it to the inventory actually on hand and to the policy's per-bid share.
      let sellAmount = (sellable * BigInt(policy.maxLegShareBps)) / BPS;
      if (sellAmount === 0n) continue;

      let quote = bidCost({
        sellAmount,
        sellDecimals: sell.decimals,
        sellPriceE9: sell.refPriceE9,
        buyDecimals: buy.decimals,
        buyPriceE9: buy.refPriceE9,
        premiumBps,
      });
      const held = input.inventory[buyIndex] ?? 0n;
      const budget = held < buyable ? held : buyable;
      if (budget === 0n) {
        notes.push(`no inventory to pay for leg ${buyIndex}`);
        continue;
      }
      if (quote.buyAmount > budget) {
        // Scale the sell side down in the same ratio, which is exact enough because the pricing is
        // linear in size, then re-quote so the number sent on chain is the program's own.
        sellAmount = (sellAmount * budget) / quote.buyAmount;
        if (sellAmount === 0n) continue;
        quote = bidCost({
          sellAmount,
          sellDecimals: sell.decimals,
          sellPriceE9: sell.refPriceE9,
          buyDecimals: buy.decimals,
          buyPriceE9: buy.refPriceE9,
          premiumBps,
        });
        if (quote.buyAmount > budget) continue;
      }

      const sellMarket = input.marketPricesE9[sellIndex] ?? 0n;
      const buyMarket = input.marketPricesE9[buyIndex] ?? 0n;
      if (sellMarket === 0n || buyMarket === 0n) {
        notes.push(`no market price for the ${sellIndex}/${buyIndex} pair`);
        continue;
      }
      const outValue = (sellAmount * sellMarket) / 10n ** BigInt(sell.decimals);
      const inValue = (quote.buyAmount * buyMarket) / 10n ** BigInt(buy.decimals);
      if (inValue === 0n) continue;
      const edgeBps = Number(((outValue - inValue) * BPS) / inValue);
      if (edgeBps < policy.minEdgeBps) {
        notes.push(
          `pair ${sellIndex}->${buyIndex} pays ${edgeBps} bps at a ${premiumBps} bps premium, under the ${policy.minEdgeBps} bps floor`,
        );
        continue;
      }
      actions.push({
        kind: "bid",
        sellComponent: sellIndex,
        buyComponent: buyIndex,
        sellAmount,
        maxBuyAmount: quote.buyAmount,
        premiumBps,
        edgeBps,
        edgeUsd: usd(outValue - inValue),
      });
    }
  }

  return { actions, notes };
}
