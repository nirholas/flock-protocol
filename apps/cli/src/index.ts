#!/usr/bin/env node
import { parseArgs } from "node:util";
import { PublicKey } from "@solana/web3.js";

import * as commands from "./commands.js";
import { loadKeypair, makeContext } from "./context.js";

const USAGE = `flock - operate a Flock index on Solana

  flock create   --name "Flock DeFi Index" --symbol FDI \\
                 --weights MINT=3500,MINT=2500,... --nav-per-token 100
  flock create   --name ... --symbol ... --components MINT:12.5,MINT:0.8
  flock inspect  <indexMint> [--json]
  flock issue    <indexMint> <wholeTokens> [--slippage-bps 50]
  flock redeem   <indexMint> <wholeTokens> [--slippage-bps 50]
  flock accrue   <indexMint>
  flock propose  <indexMint> --weights MINT=3500,... [--duration 7200]
                 [--start-premium-bps -50] [--end-premium-bps 150] [--max-nav-loss-bps 100] [--dry-run]
  flock bid      <indexMint> --sell <i> --buy <j> --amount <wholeUnits> [--dry-run]
  flock end      <indexMint>

Common flags
  --url <rpc>            RPC endpoint (or SOLANA_RPC_URL)
  --program-id <pubkey>  index program (or FLOCK_PROGRAM_ID, or deployments/<cluster>.json)
  --cluster <name>       which deployments file to read (default mainnet)
  --keypair <path>       signer (or FLOCK_KEYPAIR, default ~/.config/solana/id.json)

Fees are in basis points. Auction premiums are bidder discounts: negative means the bidder pays
the fund above reference value, positive means the fund pays for the fill. An auction opens
negative and decays.
`;

const options = {
  url: { type: "string" },
  "program-id": { type: "string" },
  cluster: { type: "string" },
  keypair: { type: "string" },
  name: { type: "string" },
  symbol: { type: "string" },
  components: { type: "string" },
  weights: { type: "string" },
  "nav-per-token": { type: "string" },
  "streaming-fee-bps": { type: "string" },
  "issue-fee-bps": { type: "string" },
  "redeem-fee-bps": { type: "string" },
  "max-premium-bps": { type: "string" },
  "rebalance-delay": { type: "string" },
  governor: { type: "string" },
  manager: { type: "string" },
  "fee-recipient": { type: "string" },
  "slippage-bps": { type: "string" },
  duration: { type: "string" },
  "start-premium-bps": { type: "string" },
  "end-premium-bps": { type: "string" },
  "max-nav-loss-bps": { type: "string" },
  sell: { type: "string" },
  buy: { type: "string" },
  amount: { type: "string" },
  json: { type: "boolean" },
  "dry-run": { type: "boolean" },
  help: { type: "boolean" },
} as const;

function num(value: string | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) throw new Error(`"${value}" is not a number`);
  return parsed;
}

async function main(): Promise<void> {
  const { values, positionals } = parseArgs({ options, allowPositionals: true });
  const [command, ...rest] = positionals;
  if (!command || values.help) {
    console.log(USAGE);
    return;
  }

  const ctx = makeContext({
    ...(values.url ? { url: values.url } : {}),
    ...(values["program-id"] ? { programId: values["program-id"] } : {}),
    ...(values.cluster ? { cluster: values.cluster } : {}),
  });
  const signer = () => loadKeypair(values.keypair);
  const mintArg = (): PublicKey => {
    if (!rest[0]) throw new Error(`${command} needs an index mint`);
    return new PublicKey(rest[0]);
  };

  switch (command) {
    case "create": {
      if (!values.name || !values.symbol) throw new Error("create needs --name and --symbol");
      await commands.create(ctx, signer(), {
        name: values.name,
        symbol: values.symbol,
        ...(values.components ? { components: values.components } : {}),
        ...(values.weights ? { weights: values.weights } : {}),
        ...(values["nav-per-token"] ? { navPerToken: num(values["nav-per-token"], 0) } : {}),
        streamingFeeBps: num(values["streaming-fee-bps"], 95),
        issueFeeBps: num(values["issue-fee-bps"], 0),
        redeemFeeBps: num(values["redeem-fee-bps"], 0),
        maxPremiumBps: num(values["max-premium-bps"], 300),
        rebalanceDelay: num(values["rebalance-delay"], 86_400),
        ...(values.governor ? { governor: values.governor } : {}),
        ...(values.manager ? { manager: values.manager } : {}),
        ...(values["fee-recipient"] ? { feeRecipient: values["fee-recipient"] } : {}),
      });
      return;
    }
    case "inspect":
      await commands.inspect(ctx, mintArg(), values.json === true);
      return;
    case "issue":
      await commands.issue(ctx, signer(), mintArg(), num(rest[1], 0), num(values["slippage-bps"], 50));
      return;
    case "redeem":
      await commands.redeem(ctx, signer(), mintArg(), num(rest[1], 0), num(values["slippage-bps"], 50));
      return;
    case "accrue":
      await commands.accrue(ctx, signer(), mintArg());
      return;
    case "propose": {
      if (!values.weights) throw new Error("propose needs --weights MINT=BPS,...");
      await commands.propose(ctx, signer(), mintArg(), {
        weights: values.weights,
        duration: num(values.duration, 7_200),
        startPremiumBps: num(values["start-premium-bps"], -50),
        endPremiumBps: num(values["end-premium-bps"], 150),
        maxNavLossBps: num(values["max-nav-loss-bps"], 100),
        dryRun: values["dry-run"] === true,
      });
      return;
    }
    case "bid":
      await commands.bid(
        ctx,
        signer(),
        mintArg(),
        num(values.sell, -1),
        num(values.buy, -1),
        num(values.amount, 0),
        values["dry-run"] === true,
      );
      return;
    case "end":
      await commands.end(ctx, signer(), mintArg());
      return;
    default:
      console.log(USAGE);
      throw new Error(`Unknown command "${command}"`);
  }
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
