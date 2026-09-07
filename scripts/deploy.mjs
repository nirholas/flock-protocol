#!/usr/bin/env node
// Deploy the index program and record what was deployed.
//
// The record matters as much as the deploy: every app in this repo resolves its program id from
// deployments/<cluster>.json, and the hash lets anyone check that the id they are trusting was
// built from the binary in this tree.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const binary = join(root, "program/target/deploy/flock_index.so");
const keypair = join(root, "program/target/deploy/flock_index-keypair.json");

const { values } = parseArgs({
  options: {
    cluster: { type: "string" },
    url: { type: "string" },
    keypair: { type: "string" },
    "dry-run": { type: "boolean" },
  },
});

const cluster = values.cluster ?? "devnet";
const url = values.url ?? (cluster === "mainnet" ? "https://api.mainnet-beta.solana.com" : `https://api.${cluster}.solana.com`);

if (!existsSync(binary)) {
  console.error(`${binary} is missing. Build it first:\n  cd program && cargo build-sbf`);
  process.exit(1);
}
if (!existsSync(keypair)) {
  console.error(`${keypair} is missing. cargo build-sbf writes it; do not commit it.`);
  process.exit(1);
}

const bytes = readFileSync(binary);
const sha256 = createHash("sha256").update(bytes).digest("hex");
const programId = execFileSync("solana-keygen", ["pubkey", keypair]).toString().trim();

console.log(`program   ${programId}`);
console.log(`binary    ${binary} (${bytes.length} bytes, sha256 ${sha256})`);
console.log(`cluster   ${cluster} via ${url}`);

if (values["dry-run"]) {
  console.log("dry run: nothing was deployed");
  process.exit(0);
}

const args = ["program", "deploy", binary, "--program-id", keypair, "--url", url];
if (values.keypair) args.push("--keypair", values.keypair);
execFileSync("solana", args, { stdio: "inherit" });

const record = {
  cluster,
  url,
  programId,
  binarySha256: sha256,
  binaryBytes: bytes.length,
  deployedAt: new Date().toISOString(),
};
mkdirSync(join(root, "deployments"), { recursive: true });
const file = join(root, "deployments", `${cluster}.json`);
writeFileSync(file, `${JSON.stringify(record, null, 2)}\n`);
console.log(`wrote ${file}`);
