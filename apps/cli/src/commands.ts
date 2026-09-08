import {
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  MINT_SIZE,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createInitializeMint2Instruction,
  getAssociatedTokenAddressSync,
  getMinimumBalanceForRentExemptMint,
  getMint,
} from "@solana/spl-token";
import {
  UNIT_SCALE,
  explainError,
  fetchPrices,
  fetchTokenStats,
  indexPda,
  instructions as ix,
  premiumAt,
  unitsForWeights,
  bidCost,
} from "@flock/sdk";

import { formatAmount, formatUsdE9, type Context } from "./context.js";

async function send(ctx: Context, transaction: Transaction, signers: Keypair[]): Promise<string> {
  try {
    return await sendAndConfirmTransaction(ctx.connection, transaction, signers, {
      commitment: "confirmed",
    });
  } catch (error) {
    throw new Error(explainError(error));
  }
}

/** `MINT=BPS,MINT=BPS` into a weighting that must add up to 100%. */
export function parseWeights(spec: string): Array<{ mint: PublicKey; bps: number }> {
  const parts = spec
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
  const weights = parts.map((entry) => {
    const [mint, bps] = entry.split("=");
    if (!mint || !bps) throw new Error(`Bad weight "${entry}", expected MINT=BPS`);
    return { mint: new PublicKey(mint.trim()), bps: Number(bps) };
  });
  const total = weights.reduce((sum, w) => sum + w.bps, 0);
  if (total !== 10_000) {
    throw new Error(`Weights add up to ${total} bps, not 10000. An index must be fully allocated.`);
  }
  return weights;
}

/** `MINT:WHOLE_UNITS` into base units per index token, using each mint's own decimals. */
export function parseComponents(spec: string): Array<{ mint: PublicKey; whole: number }> {
  return spec
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [mint, whole] = entry.split(":");
      if (!mint || !whole) throw new Error(`Bad component "${entry}", expected MINT:WHOLE_UNITS`);
      return { mint: new PublicKey(mint.trim()), whole: Number(whole) };
    });
}

export interface CreateOptions {
  name: string;
  symbol: string;
  components?: string;
  weights?: string;
  navPerToken?: number;
  streamingFeeBps: number;
  issueFeeBps: number;
  redeemFeeBps: number;
  maxPremiumBps: number;
  rebalanceDelay: number;
  governor?: string;
  manager?: string;
  feeRecipient?: string;
}

/**
 * Create an index end to end: mint, account, components, seal.
 *
 * Units can be given directly, or derived from a target weighting and the NAV one index token
 * should start at, which is how a methodology actually specifies a launch: "35% JUP, 20% JTO, and
 * one token is worth $100 on day one".
 */
export async function create(ctx: Context, payer: Keypair, options: CreateOptions): Promise<void> {
  const governor = options.governor ? new PublicKey(options.governor) : payer.publicKey;
  const manager = options.manager ? new PublicKey(options.manager) : payer.publicKey;
  const feeRecipient = options.feeRecipient ? new PublicKey(options.feeRecipient) : payer.publicKey;

  let table: Array<{ mint: PublicKey; units: bigint }>;
  if (options.weights) {
    if (!options.navPerToken) {
      throw new Error("--weights needs --nav-per-token: a weighting alone does not size a token.");
    }
    const weights = parseWeights(options.weights);
    const mints = weights.map((w) => w.mint.toBase58());
    const prices = await fetchPrices(mints);
    const decimals: number[] = [];
    const pricesE9: bigint[] = [];
    for (const mint of mints) {
      const quote = prices.get(mint);
      if (!quote) throw new Error(`No price for ${mint}`);
      pricesE9.push(quote.priceE9);
      decimals.push(quote.decimals);
    }
    const navE9 = BigInt(Math.round(options.navPerToken * 1e9));
    const units = unitsForWeights({
      weightsBps: weights.map((w) => w.bps),
      pricesE9,
      decimals,
      navE9,
      supply: UNIT_SCALE,
    });
    table = weights.map((w, i) => ({ mint: w.mint, units: units[i] ?? 0n }));
  } else if (options.components) {
    const parsed = parseComponents(options.components);
    table = [];
    for (const component of parsed) {
      const mint = await getMint(ctx.connection, component.mint);
      table.push({
        mint: component.mint,
        units: BigInt(Math.round(component.whole * 10 ** mint.decimals)),
      });
    }
  } else {
    throw new Error("Pass --components MINT:WHOLE,... or --weights MINT=BPS,... --nav-per-token N");
  }
  if (table.some((entry) => entry.units <= 0n)) {
    throw new Error("Every component needs a positive unit amount");
  }

  const mintKeypair = Keypair.generate();
  const indexMint = mintKeypair.publicKey;
  const [index] = indexPda(ctx.programId, indexMint);

  const createMintTx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: indexMint,
      lamports: await getMinimumBalanceForRentExemptMint(ctx.connection),
      space: MINT_SIZE,
      programId: TOKEN_PROGRAM_ID,
    }),
    // The index PDA is the only mint authority, and there is no freeze authority at all.
    createInitializeMint2Instruction(indexMint, 9, index, null),
  );
  await send(ctx, createMintTx, [payer, mintKeypair]);
  console.log(`index mint  ${indexMint.toBase58()}`);

  await send(
    ctx,
    new Transaction().add(
      ix.initIndex({
        programId: ctx.programId,
        payer: payer.publicKey,
        indexMint,
        governor,
        manager,
        feeRecipient,
        name: options.name,
        symbol: options.symbol,
        streamingFeeBps: options.streamingFeeBps,
        issueFeeBps: options.issueFeeBps,
        redeemFeeBps: options.redeemFeeBps,
        maxPremiumBps: options.maxPremiumBps,
        rebalanceDelay: options.rebalanceDelay,
      }),
    ),
    [payer],
  );
  console.log(`index       ${index.toBase58()}`);

  for (const component of table) {
    await send(
      ctx,
      new Transaction().add(
        ix.addComponent({
          programId: ctx.programId,
          manager,
          payer: payer.publicKey,
          indexMint,
          componentMint: component.mint,
          units: component.units,
        }),
      ),
      [payer],
    );
    console.log(`component   ${component.mint.toBase58()}  ${component.units} units`);
  }

  await send(
    ctx,
    new Transaction().add(ix.sealIndex({ programId: ctx.programId, manager, indexMint })),
    [payer],
  );
  // The fee recipient needs somewhere to receive fees before the first accrual, and creating it
  // now means nobody has to discover that at the worst moment.
  await send(
    ctx,
    new Transaction().add(
      createAssociatedTokenAccountIdempotentInstruction(
        payer.publicKey,
        getAssociatedTokenAddressSync(indexMint, feeRecipient, true),
        feeRecipient,
        indexMint,
      ),
    ),
    [payer],
  );
  console.log(`sealed. ${options.symbol} is live with ${table.length} components.`);
}

export async function inspect(ctx: Context, indexMint: PublicKey, json: boolean): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  const mints = snapshot.account.components.map((c) => c.mint.toBase58());
  const stats = await fetchTokenStats(mints);
  const pricesE9 = snapshot.account.components.map(
    (c) => stats.get(c.mint.toBase58())?.priceE9 ?? 0n,
  );
  const { navE9, weights } = ctx.client.weights(snapshot, pricesE9);
  const navPerToken = ctx.client.navPerToken(snapshot, pricesE9);
  const auction = ctx.client.auction(snapshot);

  if (json) {
    console.log(
      JSON.stringify(
        {
          indexMint: indexMint.toBase58(),
          index: snapshot.address.toBase58(),
          name: snapshot.account.name,
          symbol: snapshot.account.symbol,
          state: snapshot.account.state,
          supply: snapshot.supply.toString(),
          navUsd: Number(navE9) / 1e9,
          navPerTokenUsd: Number(navPerToken) / 1e9,
          pendingFee: snapshot.pendingFee.toString(),
          streamingFeeBps: snapshot.account.streamingFeeBps,
          components: snapshot.account.components.map((component, i) => ({
            mint: component.mint.toBase58(),
            symbol: stats.get(component.mint.toBase58())?.symbol ?? null,
            decimals: component.decimals,
            units: (snapshot.units[i] ?? 0n).toString(),
            targetUnits: component.targetUnits.toString(),
            balance: (snapshot.balances[i] ?? 0n).toString(),
            weightBps: weights[i] ?? 0,
            priceUsd: stats.get(component.mint.toBase58())?.usdPrice ?? null,
          })),
          auction: {
            active: auction.active,
            opensAt: auction.opensAt,
            closesAt: auction.closesAt,
            premiumBps: auction.premiumBps,
          },
        },
        null,
        2,
      ),
    );
    return;
  }

  console.log(`${snapshot.account.name} (${snapshot.account.symbol})  ${snapshot.account.state}`);
  console.log(`index mint   ${indexMint.toBase58()}`);
  console.log(`supply       ${formatAmount(snapshot.supply, 9)} tokens`);
  console.log(`NAV          ${formatUsdE9(navE9)}   (${formatUsdE9(navPerToken)} per token)`);
  console.log(
    `fees         ${snapshot.account.streamingFeeBps} bps streaming, ` +
      `${snapshot.account.issueFeeBps}/${snapshot.account.redeemFeeBps} bps issue/redeem, ` +
      `${formatAmount(snapshot.pendingFee, 9)} accrued and unminted`,
  );
  console.log("");
  console.log("  weight   symbol   units per token        balance");
  snapshot.account.components.forEach((component, i) => {
    const symbol = (stats.get(component.mint.toBase58())?.symbol ?? "?").padEnd(8);
    const weight = `${((weights[i] ?? 0) / 100).toFixed(2)}%`.padStart(8);
    const units = formatAmount(snapshot.units[i] ?? 0n, component.decimals).padStart(20);
    const balance = formatAmount(snapshot.balances[i] ?? 0n, component.decimals).padStart(14);
    console.log(`${weight}   ${symbol} ${units} ${balance}`);
  });

  if (auction.active) {
    const now = Math.floor(Date.now() / 1000);
    console.log("");
    console.log(
      auction.premiumBps === null
        ? `auction opens in ${auction.opensAt - now}s, closes ${auction.closesAt - now}s later`
        : `auction open, premium ${auction.premiumBps} bps, ${auction.closesAt - now}s left`,
    );
    auction.legs.forEach((leg, i) => {
      const component = snapshot.account.components[i]!;
      if (leg.sellable > 0n) {
        console.log(
          `  selling up to ${formatAmount(leg.sellable, component.decimals)} of ${component.mint.toBase58()}`,
        );
      }
      if (leg.buyable > 0n) {
        console.log(
          `  buying up to ${formatAmount(leg.buyable, component.decimals)} of ${component.mint.toBase58()}`,
        );
      }
    });
  }
}

export async function issue(
  ctx: Context,
  payer: Keypair,
  indexMint: PublicKey,
  wholeTokens: number,
  slippageBps: number,
): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  const amount = BigInt(Math.round(wholeTokens * 1e9));
  const { required, feeTokens } = ctx.client.quoteIssue(snapshot, amount);
  snapshot.account.components.forEach((component, i) => {
    console.log(
      `pay ${formatAmount(required[i] ?? 0n, component.decimals)} of ${component.mint.toBase58()}`,
    );
  });
  if (feeTokens > 0n) console.log(`issue fee ${formatAmount(feeTokens, 9)} ${snapshot.account.symbol}`);

  const transaction = new Transaction().add(
    createAssociatedTokenAccountIdempotentInstruction(
      payer.publicKey,
      getAssociatedTokenAddressSync(indexMint, payer.publicKey),
      payer.publicKey,
      indexMint,
    ),
    ctx.client.issueInstruction({ snapshot, user: payer.publicKey, amount, slippageBps }),
  );
  console.log(await send(ctx, transaction, [payer]));
}

export async function redeem(
  ctx: Context,
  payer: Keypair,
  indexMint: PublicKey,
  wholeTokens: number,
  slippageBps: number,
): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  const amount = BigInt(Math.round(wholeTokens * 1e9));
  const { payout } = ctx.client.quoteRedeem(snapshot, amount);

  const transaction = new Transaction();
  for (const component of snapshot.account.components) {
    transaction.add(
      createAssociatedTokenAccountIdempotentInstruction(
        payer.publicKey,
        getAssociatedTokenAddressSync(component.mint, payer.publicKey),
        payer.publicKey,
        component.mint,
      ),
    );
  }
  transaction.add(
    ctx.client.redeemInstruction({
      snapshot,
      user: payer.publicKey,
      amount,
      slippageBps,
      feeAccount: getAssociatedTokenAddressSync(indexMint, snapshot.account.feeRecipient, true),
    }),
  );
  snapshot.account.components.forEach((component, i) => {
    console.log(
      `receive ${formatAmount(payout[i] ?? 0n, component.decimals)} of ${component.mint.toBase58()}`,
    );
  });
  console.log(await send(ctx, transaction, [payer]));
}

export async function accrue(ctx: Context, payer: Keypair, indexMint: PublicKey): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  console.log(`minting ${formatAmount(snapshot.pendingFee, 9)} ${snapshot.account.symbol} in fees`);
  const transaction = new Transaction().add(
    ix.accrueFees({
      programId: ctx.programId,
      indexMint,
      feeAccount: getAssociatedTokenAddressSync(indexMint, snapshot.account.feeRecipient, true),
      componentMints: snapshot.account.components.map((c) => c.mint),
    }),
  );
  console.log(await send(ctx, transaction, [payer]));
}

export interface ProposeOptions {
  weights: string;
  duration: number;
  startPremiumBps: number;
  endPremiumBps: number;
  maxNavLossBps: number;
  dryRun: boolean;
}

/**
 * Publish a target composition as a Dutch auction.
 *
 * Reference prices are read live and written into the proposal, which is what every bid is then
 * scored against. That is deliberate: the auction prices against the market as it was when the
 * committee decided, and the decaying premium is what pays a bidder for the risk that it has
 * moved since.
 */
export async function propose(
  ctx: Context,
  payer: Keypair,
  indexMint: PublicKey,
  options: ProposeOptions,
): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  const weights = parseWeights(options.weights);
  const order = snapshot.account.components.map((c) => c.mint.toBase58());
  const byMint = new Map(weights.map((w) => [w.mint.toBase58(), w.bps]));
  for (const mint of order) {
    if (!byMint.has(mint)) throw new Error(`Weighting is missing component ${mint}`);
  }
  if (byMint.size !== order.length) {
    throw new Error("Weighting names a mint this index does not hold; components are fixed at seal");
  }

  const pricesE9 = await (async () => {
    const quotes = await fetchPrices(order);
    return order.map((mint) => {
      const quote = quotes.get(mint);
      if (!quote) throw new Error(`No price for ${mint}`);
      return quote.priceE9;
    });
  })();

  const { navE9 } = ctx.client.weights(snapshot, pricesE9);
  const targetUnits = unitsForWeights({
    weightsBps: order.map((mint) => byMint.get(mint)!),
    pricesE9,
    decimals: snapshot.account.components.map((c) => c.decimals),
    navE9,
    supply: snapshot.supply,
  });

  console.log(`NAV ${formatUsdE9(navE9)}, supply ${formatAmount(snapshot.supply, 9)}`);
  snapshot.account.components.forEach((component, i) => {
    const from = snapshot.units[i] ?? 0n;
    const to = targetUnits[i] ?? 0n;
    const direction = to > from ? "buy " : to < from ? "sell" : "hold";
    console.log(
      `${direction} ${component.mint.toBase58()}  ${formatAmount(from, component.decimals)} -> ` +
        `${formatAmount(to, component.decimals)} per token`,
    );
  });
  if (options.dryRun) {
    console.log("dry run: nothing was submitted");
    return;
  }

  const transaction = new Transaction().add(
    ix.proposeRebalance({
      programId: ctx.programId,
      manager: payer.publicKey,
      indexMint,
      componentMints: snapshot.account.components.map((c) => c.mint),
      targetUnits,
      refPricesE9: pricesE9,
      duration: options.duration,
      startPremiumBps: options.startPremiumBps,
      endPremiumBps: options.endPremiumBps,
      maxNavLossBps: options.maxNavLossBps,
    }),
  );
  console.log(await send(ctx, transaction, [payer]));
  console.log(
    `auction opens in ${snapshot.account.rebalanceDelay}s and runs for ${options.duration}s`,
  );
}

export async function bid(
  ctx: Context,
  payer: Keypair,
  indexMint: PublicKey,
  sellComponent: number,
  buyComponent: number,
  sellWhole: number,
  dryRun: boolean,
): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  const sell = snapshot.account.components[sellComponent];
  const buy = snapshot.account.components[buyComponent];
  if (!sell || !buy) throw new Error("Component index out of range");
  const { rebalance } = snapshot.account;
  if (!rebalance.active) throw new Error("No auction is running");

  const now = Math.floor(Date.now() / 1000);
  const premiumBps = premiumAt(
    rebalance.startPremiumBps,
    rebalance.endPremiumBps,
    rebalance.startTs,
    rebalance.endTs,
    now,
  );
  const sellAmount = BigInt(Math.round(sellWhole * 10 ** sell.decimals));
  const quote = bidCost({
    sellAmount,
    sellDecimals: sell.decimals,
    sellPriceE9: sell.refPriceE9,
    buyDecimals: buy.decimals,
    buyPriceE9: buy.refPriceE9,
    premiumBps,
  });

  console.log(`premium ${premiumBps} bps`);
  console.log(`take ${formatAmount(sellAmount, sell.decimals)} of ${sell.mint.toBase58()}`);
  console.log(`pay  ${formatAmount(quote.buyAmount, buy.decimals)} of ${buy.mint.toBase58()}`);

  // Mark the same trade at the live market, which is the number that decides whether to bid.
  const prices = await fetchPrices([sell.mint.toBase58(), buy.mint.toBase58()]);
  const sellMarket = prices.get(sell.mint.toBase58());
  const buyMarket = prices.get(buy.mint.toBase58());
  if (sellMarket && buyMarket) {
    const outUsd = (Number(sellAmount) / 10 ** sell.decimals) * sellMarket.usdPrice;
    const inUsd = (Number(quote.buyAmount) / 10 ** buy.decimals) * buyMarket.usdPrice;
    console.log(
      `market value out ${outUsd.toFixed(2)} USD, in ${inUsd.toFixed(2)} USD, ` +
        `edge ${(outUsd - inUsd).toFixed(2)} USD (${(((outUsd - inUsd) / inUsd) * 10_000).toFixed(0)} bps)`,
    );
  }
  if (dryRun) {
    console.log("dry run: nothing was submitted");
    return;
  }

  const transaction = new Transaction().add(
    createAssociatedTokenAccountIdempotentInstruction(
      payer.publicKey,
      getAssociatedTokenAddressSync(sell.mint, payer.publicKey),
      payer.publicKey,
      sell.mint,
    ),
    ix.bid({
      programId: ctx.programId,
      bidder: payer.publicKey,
      indexMint,
      componentMints: snapshot.account.components.map((c) => c.mint),
      bidderReceiveAccount: getAssociatedTokenAddressSync(sell.mint, payer.publicKey),
      bidderPayAccount: getAssociatedTokenAddressSync(buy.mint, payer.publicKey),
      sellComponent,
      buyComponent,
      sellAmount,
      maxBuyAmount: quote.buyAmount,
    }),
  );
  console.log(await send(ctx, transaction, [payer]));
}

export async function end(ctx: Context, payer: Keypair, indexMint: PublicKey): Promise<void> {
  const snapshot = await ctx.client.snapshot(indexMint);
  const transaction = new Transaction().add(
    ix.endRebalance({
      programId: ctx.programId,
      caller: payer.publicKey,
      indexMint,
      componentMints: snapshot.account.components.map((c) => c.mint),
    }),
  );
  console.log(await send(ctx, transaction, [payer]));
}
