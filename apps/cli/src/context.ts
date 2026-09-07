import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { FlockClient, programId } from "@flock/sdk";

/**
 * Everything a command needs to talk to a cluster: an RPC connection, a program id, and (only for
 * commands that write) a signer.
 *
 * Resolution order is flag, then environment, then the Solana CLI's own config, so this behaves
 * the way anyone with `solana config set` already expects.
 */
export interface Context {
  connection: Connection;
  client: FlockClient;
  programId: PublicKey;
  cluster: string;
}

export const DEFAULT_RPC = "https://api.mainnet-beta.solana.com";

export function loadKeypair(path?: string): Keypair {
  const file = path ?? process.env.FLOCK_KEYPAIR ?? join(homedir(), ".config/solana/id.json");
  let raw: string;
  try {
    raw = readFileSync(file, "utf8");
  } catch (error) {
    throw new Error(
      `No keypair at ${file}. Pass --keypair, set FLOCK_KEYPAIR, or run \`solana-keygen new\`.`,
    );
  }
  const bytes = JSON.parse(raw);
  if (!Array.isArray(bytes)) throw new Error(`${file} is not a Solana keypair file`);
  return Keypair.fromSecretKey(Uint8Array.from(bytes));
}

/** Program ids of deployments this repo knows about, written by `scripts/deploy.mjs`. */
export function deployedProgramId(cluster: string): string | undefined {
  try {
    const file = new URL(`../../../deployments/${cluster}.json`, import.meta.url);
    return JSON.parse(readFileSync(file, "utf8")).programId;
  } catch {
    return undefined;
  }
}

export function makeContext(options: { url?: string; programId?: string; cluster?: string }): Context {
  const cluster = options.cluster ?? process.env.FLOCK_CLUSTER ?? "mainnet";
  const url = options.url ?? process.env.SOLANA_RPC_URL ?? DEFAULT_RPC;
  const connection = new Connection(url, "confirmed");
  const id = programId(options.programId ?? process.env.FLOCK_PROGRAM_ID ?? deployedProgramId(cluster));
  return { connection, client: new FlockClient(connection, id), programId: id, cluster };
}

export function requirePubkey(value: string | undefined, label: string): PublicKey {
  if (!value) throw new Error(`Missing ${label}`);
  return new PublicKey(value);
}

/** Whole tokens as a string, from base units and decimals. Display only; never fed back in. */
export function formatAmount(amount: bigint, decimals: number, places = 6): string {
  const scale = 10n ** BigInt(decimals);
  const whole = amount / scale;
  const fraction = (amount % scale).toString().padStart(decimals, "0").slice(0, places);
  return places > 0 ? `${whole}.${fraction}` : whole.toString();
}

export function formatUsdE9(value: bigint): string {
  const dollars = Number(value) / 1e9;
  return dollars.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });
}
