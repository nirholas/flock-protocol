#!/usr/bin/env node
import { parseArgs } from "node:util";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  createAssociatedTokenAccountIdempotentInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { FlockClient, explainError, fetchPrices, instructions as ix, programId } from "@flock/sdk";

import { DEFAULT_POLICY, plan, type Action, type KeeperPolicy } from "./planner.js";

/**
 * The keeper.
 *
 * It does three jobs, in the order that matters: it mints the streaming fee the manager has
 * already earned, it closes auctions that have expired so the next one can be proposed, and it
 * bids into auctions that are paying more than they cost. The third is the one that makes an index
 * rebalance without anybody privileged touching the fund.
 *
 * It bids from inventory it already holds. Sourcing that inventory (a swap on any venue) is the
 * bidder's own business and is deliberately not wired in here: a keeper that swaps to chase every
 * auction is a strategy, and strategies belong to whoever runs them, not to the protocol's
 * reference operator. `flock bid --dry-run` prints the exact leg a bidder would need to hold.
 */

const USAGE = `flock-keeper - operate Flock indexes

  flock-keeper --indexes <mint>,<mint> [--once] [--execute]

  --indexes <mints>       comma separated index mints (or FLOCK_INDEXES)
  --interval <seconds>    poll interval, default 60
  --once                  run one pass and exit
  --execute               actually send transactions; without it, the keeper only reports
  --min-edge-bps <n>      do not bid under this market edge, default 25
  --min-accrual <tokens>  do not accrue less than this many index base units, default 1000000
  --max-leg-share-bps <n> most of a leg to take in one bid, default 10000 (all of it)
  --url <rpc>             RPC endpoint (or SOLANA_RPC_URL)
  --program-id <pubkey>   index program (or FLOCK_PROGRAM_ID)
  --keypair <path>        signer (or FLOCK_KEYPAIR)
`;

function log(event: string, fields: Record<string, unknown> = {}): void {
  const parts = Object.entries(fields).map(([k, v]) => `${k}=${typeof v === "bigint" ? v.toString() : v}`);
  console.log(`${new Date().toISOString()} ${event} ${parts.join(" ")}`.trimEnd());
}

async function inventoryOf(
  connection: Connection,
  owner: PublicKey,
  mints: PublicKey[],
): Promise<bigint[]> {
  const accounts = mints.map((mint) => getAssociatedTokenAddressSync(mint, owner));
  const infos = await connection.getMultipleAccountsInfo(accounts);
  return infos.map((info, i) => {
    if (!info) return 0n;
    try {
      return getAccountAmount(info.data);
    } catch {
      log("inventory.unreadable", { mint: mints[i]?.toBase58() });
      return 0n;
    }
  });
}

/** SPL token accounts store the amount as a little-endian u64 at offset 64. */
function getAccountAmount(data: Buffer): bigint {
  if (data.length < 72) throw new Error("not a token account");
  return data.readBigUInt64LE(64);
}

async function runOnce(args: {
  client: FlockClient;
  connection: Connection;
  programId: PublicKey;
  indexMint: PublicKey;
  signer: Keypair | null;
  execute: boolean;
  policy: KeeperPolicy;
}): Promise<void> {
  const { client, connection, indexMint, signer, execute, policy } = args;
  const snapshot = await client.snapshot(indexMint);
  const mints = snapshot.account.components.map((c) => c.mint);
  const quotes = await fetchPrices(mints.map((m) => m.toBase58()));
  const marketPricesE9 = mints.map((mint) => quotes.get(mint.toBase58())?.priceE9 ?? 0n);
  const inventory = signer ? await inventoryOf(connection, signer.publicKey, mints) : mints.map(() => 0n);

  const { actions, notes } = plan({
    snapshot,
    marketPricesE9,
    now: Math.floor(Date.now() / 1000),
    policy,
    inventory,
  });

  log("index", {
    symbol: snapshot.account.symbol,
    state: snapshot.account.state,
    supply: snapshot.supply,
    auction: snapshot.account.rebalance.active,
    actions: actions.length,
  });
  for (const note of notes) log("skip", { why: note });

  for (const action of actions) {
    log(`plan.${action.kind}`, describe(action));
    if (!execute || !signer) continue;
    try {
      const signature = await submit({ ...args, snapshot, signer, action });
      log(`done.${action.kind}`, { signature });
    } catch (error) {
      log(`failed.${action.kind}`, { error: explainError(error) });
    }
  }
}

function describe(action: Action): Record<string, unknown> {
  switch (action.kind) {
    case "accrue":
      return { pendingFee: action.pendingFee };
    case "end":
      return { reason: action.reason };
    case "bid":
      return {
        sell: action.sellComponent,
        buy: action.buyComponent,
        sellAmount: action.sellAmount,
        pay: action.maxBuyAmount,
        premiumBps: action.premiumBps,
        edgeBps: action.edgeBps,
        edgeUsd: action.edgeUsd.toFixed(2),
      };
  }
}

async function submit(args: {
  client: FlockClient;
  connection: Connection;
  programId: PublicKey;
  indexMint: PublicKey;
  signer: Keypair;
  snapshot: Awaited<ReturnType<FlockClient["snapshot"]>>;
  action: Action;
}): Promise<string> {
  const { connection, programId: program, indexMint, signer, snapshot, action } = args;
  const componentMints = snapshot.account.components.map((c) => c.mint);
  const transaction = new Transaction();

  if (action.kind === "accrue") {
    transaction.add(
      ix.accrueFees({
        programId: program,
        indexMint,
        feeAccount: getAssociatedTokenAddressSync(indexMint, snapshot.account.feeRecipient, true),
        componentMints,
      }),
    );
  } else if (action.kind === "end") {
    transaction.add(
      ix.endRebalance({ programId: program, caller: signer.publicKey, indexMint, componentMints }),
    );
  } else {
    const sell = snapshot.account.components[action.sellComponent]!;
    const buy = snapshot.account.components[action.buyComponent]!;
    const receive = getAssociatedTokenAddressSync(sell.mint, signer.publicKey);
    transaction.add(
      createAssociatedTokenAccountIdempotentInstruction(signer.publicKey, receive, signer.publicKey, sell.mint),
      ix.bid({
        programId: program,
        bidder: signer.publicKey,
        indexMint,
        componentMints,
        bidderReceiveAccount: receive,
        bidderPayAccount: getAssociatedTokenAddressSync(buy.mint, signer.publicKey),
        sellComponent: action.sellComponent,
        buyComponent: action.buyComponent,
        sellAmount: action.sellAmount,
        maxBuyAmount: action.maxBuyAmount,
      }),
    );
  }
  return sendAndConfirmTransaction(connection, transaction, [signer], { commitment: "confirmed" });
}

function loadSigner(path: string | undefined): Keypair | null {
  const file = path ?? process.env.FLOCK_KEYPAIR ?? join(homedir(), ".config/solana/id.json");
  try {
    return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(file, "utf8"))));
  } catch {
    return null;
  }
}

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      indexes: { type: "string" },
      interval: { type: "string" },
      once: { type: "boolean" },
      execute: { type: "boolean" },
      "min-edge-bps": { type: "string" },
      "min-accrual": { type: "string" },
      "max-leg-share-bps": { type: "string" },
      url: { type: "string" },
      "program-id": { type: "string" },
      keypair: { type: "string" },
      help: { type: "boolean" },
    },
    allowPositionals: false,
  });
  if (values.help) {
    console.log(USAGE);
    return;
  }

  const raw = values.indexes ?? process.env.FLOCK_INDEXES;
  if (!raw) throw new Error("No indexes to watch. Pass --indexes or set FLOCK_INDEXES.");
  const indexes = raw.split(",").map((entry) => new PublicKey(entry.trim()));

  const connection = new Connection(
    values.url ?? process.env.SOLANA_RPC_URL ?? "https://api.mainnet-beta.solana.com",
    "confirmed",
  );
  const program = programId(values["program-id"]);
  const client = new FlockClient(connection, program);
  const signer = loadSigner(values.keypair);
  const execute = values.execute === true;
  if (execute && !signer) throw new Error("--execute needs a keypair, and none could be loaded");

  const policy: KeeperPolicy = {
    minAccrualTokens: BigInt(values["min-accrual"] ?? DEFAULT_POLICY.minAccrualTokens.toString()),
    minEdgeBps: Number(values["min-edge-bps"] ?? DEFAULT_POLICY.minEdgeBps),
    maxLegShareBps: Number(values["max-leg-share-bps"] ?? DEFAULT_POLICY.maxLegShareBps),
  };
  const interval = Number(values.interval ?? 60) * 1_000;

  log("keeper.start", {
    indexes: indexes.length,
    execute,
    signer: signer?.publicKey.toBase58() ?? "none",
    program: program.toBase58(),
  });

  for (;;) {
    for (const indexMint of indexes) {
      try {
        await runOnce({ client, connection, programId: program, indexMint, signer, execute, policy });
      } catch (error) {
        log("pass.failed", { index: indexMint.toBase58(), error: explainError(error) });
      }
    }
    if (values.once) return;
    await new Promise((resolve) => setTimeout(resolve, interval));
  }
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
