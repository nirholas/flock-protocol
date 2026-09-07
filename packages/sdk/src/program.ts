import { PublicKey } from "@solana/web3.js";

/**
 * Program id of the index engine.
 *
 * Flock has not deployed to mainnet yet, so this is read from the environment rather than
 * hardcoded to an address nobody can verify. `scripts/deploy.mjs` writes the deployed id into
 * `deployments/<cluster>.json`, and every app in this repo reads it from there.
 */
export function programId(id?: string | PublicKey): PublicKey {
  if (id instanceof PublicKey) return id;
  const raw = id ?? process.env.FLOCK_PROGRAM_ID;
  if (!raw) {
    throw new Error(
      "No program id. Pass one explicitly or set FLOCK_PROGRAM_ID (see deployments/<cluster>.json).",
    );
  }
  return new PublicKey(raw);
}

export const SEED_INDEX = Buffer.from("index");
export const SEED_VAULT = Buffer.from("vault");

/** The index account holding every parameter and the component table. */
export function indexPda(program: PublicKey, indexMint: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_INDEX, indexMint.toBuffer()], program);
}

/** The token account holding one component of one index. */
export function vaultPda(
  program: PublicKey,
  indexMint: PublicKey,
  componentMint: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SEED_VAULT, indexMint.toBuffer(), componentMint.toBuffer()],
    program,
  );
}

/** Index tokens always have 9 decimals, so one whole token is 1e9 base units. */
export const UNIT_SCALE = 1_000_000_000n;
export const BPS = 10_000n;
export const SECONDS_PER_YEAR = 31_536_000n;

export const MAX_COMPONENTS = 16;
export const MAX_STREAMING_FEE_BPS = 500;
export const MAX_MINT_REDEEM_FEE_BPS = 100;
export const MAX_PREMIUM_CEILING_BPS = 1000;
export const MIN_REBALANCE_DELAY = 300;
