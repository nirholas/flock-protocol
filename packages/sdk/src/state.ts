import { PublicKey } from "@solana/web3.js";

/**
 * Decoder for the on-chain index account.
 *
 * The program stores the record as a fixed C layout so it can read it in place, which means this
 * decoder is a set of byte offsets rather than a schema. The offsets are asserted against the Rust
 * struct by `test/layout.test.ts`, so a field added on one side without the other fails a test
 * instead of silently shifting every value after it.
 */

export const INDEX_ACCOUNT_LEN = 1304;
export const ACCOUNT_TAG_INDEX = 0xf1;
const COMPONENT_LEN = 64;
const COMPONENTS_OFFSET = 208;
const REBALANCE_OFFSET = COMPONENTS_OFFSET + COMPONENT_LEN * 16;

export type IndexStateName = "bootstrapping" | "live" | "paused";

export interface ComponentAccount {
  mint: PublicKey;
  /** Base units of this component backing one whole index token. */
  units: bigint;
  /** Where the running auction intends `units` to land. */
  targetUnits: bigint;
  /** USD per whole token, 1e9, captured when the auction was proposed. */
  refPriceE9: bigint;
  decimals: number;
  vaultBump: number;
}

export interface RebalanceAccount {
  active: boolean;
  proposer: PublicKey;
  proposedAt: number;
  startTs: number;
  endTs: number;
  navFloorE6: bigint;
  startPremiumBps: number;
  endPremiumBps: number;
  maxNavLossBps: number;
}

export interface IndexAccount {
  indexMint: PublicKey;
  governor: PublicKey;
  manager: PublicKey;
  feeRecipient: PublicKey;
  lastFeeAccrual: number;
  createdAt: number;
  rebalanceDelay: number;
  streamingFeeBps: number;
  issueFeeBps: number;
  redeemFeeBps: number;
  maxPremiumBps: number;
  version: number;
  bump: number;
  state: IndexStateName;
  decimals: number;
  name: string;
  symbol: string;
  components: ComponentAccount[];
  rebalance: RebalanceAccount;
}

function key(data: Buffer, offset: number): PublicKey {
  return new PublicKey(data.subarray(offset, offset + 32));
}

function text(data: Buffer, offset: number, length: number): string {
  const raw = data.subarray(offset, offset + length);
  const end = raw.indexOf(0);
  return raw.subarray(0, end === -1 ? raw.length : end).toString("utf8");
}

function stateName(value: number): IndexStateName {
  if (value === 0) return "bootstrapping";
  if (value === 1) return "live";
  if (value === 2) return "paused";
  throw new Error(`Unknown index state ${value}`);
}

export function decodeIndex(data: Buffer): IndexAccount {
  if (data.length < INDEX_ACCOUNT_LEN) {
    throw new Error(`Index account is ${data.length} bytes, expected at least ${INDEX_ACCOUNT_LEN}`);
  }
  if (data.readUInt8(156) !== ACCOUNT_TAG_INDEX) {
    throw new Error("Account is not a Flock index (wrong tag)");
  }
  const componentCount = data.readUInt8(161);
  const components: ComponentAccount[] = [];
  for (let i = 0; i < componentCount; i += 1) {
    const at = COMPONENTS_OFFSET + i * COMPONENT_LEN;
    components.push({
      mint: key(data, at),
      units: data.readBigUInt64LE(at + 32),
      targetUnits: data.readBigUInt64LE(at + 40),
      refPriceE9: data.readBigUInt64LE(at + 48),
      decimals: data.readUInt8(at + 56),
      vaultBump: data.readUInt8(at + 57),
    });
  }
  return {
    indexMint: key(data, 0),
    governor: key(data, 32),
    manager: key(data, 64),
    feeRecipient: key(data, 96),
    lastFeeAccrual: Number(data.readBigInt64LE(128)),
    createdAt: Number(data.readBigInt64LE(136)),
    rebalanceDelay: data.readUInt32LE(144),
    streamingFeeBps: data.readUInt16LE(148),
    issueFeeBps: data.readUInt16LE(150),
    redeemFeeBps: data.readUInt16LE(152),
    maxPremiumBps: data.readUInt16LE(154),
    version: data.readUInt8(157),
    bump: data.readUInt8(158),
    state: stateName(data.readUInt8(159)),
    decimals: data.readUInt8(160),
    name: text(data, 162, 32),
    symbol: text(data, 194, 12),
    components,
    rebalance: {
      proposer: key(data, REBALANCE_OFFSET),
      proposedAt: Number(data.readBigInt64LE(REBALANCE_OFFSET + 32)),
      startTs: Number(data.readBigInt64LE(REBALANCE_OFFSET + 40)),
      endTs: Number(data.readBigInt64LE(REBALANCE_OFFSET + 48)),
      navFloorE6: data.readBigUInt64LE(REBALANCE_OFFSET + 56),
      startPremiumBps: data.readInt16LE(REBALANCE_OFFSET + 64),
      endPremiumBps: data.readInt16LE(REBALANCE_OFFSET + 66),
      maxNavLossBps: data.readUInt16LE(REBALANCE_OFFSET + 68),
      active: data.readUInt8(REBALANCE_OFFSET + 70) === 1,
    },
  };
}
