import { readFileSync } from "node:fs";
import { PublicKey } from "@solana/web3.js";

/** The artifact `program/tests/fixtures.rs` generates, shared by every test that needs it. */
export interface Fixture {
  programId: string;
  indexMint: string;
  indexAccountHex: string;
  instructions: Array<{
    name: string;
    programId: string;
    accounts: Array<{ pubkey: string; isSigner: boolean; isWritable: boolean }>;
    dataHex: string;
  }>;
}

export const fixture: Fixture = JSON.parse(
  readFileSync(new URL("./fixtures/instructions.json", import.meta.url), "utf8"),
);

/** The fixtures use pubkeys of 32 identical bytes so both sides can name them the same way. */
export const key = (seed: number) => new PublicKey(new Uint8Array(32).fill(seed));
