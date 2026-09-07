import {
  PublicKey,
  SystemProgram,
  TransactionInstruction,
  type AccountMeta,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";

import { VARIANT, Writer } from "./encode.js";
import { indexPda, vaultPda } from "./program.js";

/**
 * Instruction builders. Account order mirrors `program/src/builders.rs` exactly; the two are held
 * together by a committed fixture that both sides are tested against.
 */

const ro = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: false, isWritable: false });
const rw = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: false, isWritable: true });
const signerRo = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: true, isWritable: false });
const signerRw = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: true, isWritable: true });

export interface InitIndexArgs {
  programId: PublicKey;
  payer: PublicKey;
  indexMint: PublicKey;
  governor: PublicKey;
  manager: PublicKey;
  feeRecipient: PublicKey;
  name: string;
  symbol: string;
  streamingFeeBps: number;
  issueFeeBps: number;
  redeemFeeBps: number;
  maxPremiumBps: number;
  rebalanceDelay: number;
}

export function initIndex(args: InitIndexArgs): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  const data = new Writer()
    .u8(VARIANT.InitIndex)
    .fixed(args.name, 32)
    .fixed(args.symbol, 12)
    .u16(args.streamingFeeBps)
    .u16(args.issueFeeBps)
    .u16(args.redeemFeeBps)
    .u16(args.maxPremiumBps)
    .u32(args.rebalanceDelay)
    .toBuffer();
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRw(args.payer),
      rw(index),
      ro(args.indexMint),
      ro(args.governor),
      ro(args.manager),
      ro(args.feeRecipient),
      ro(SystemProgram.programId),
    ],
    data,
  });
}

export function addComponent(args: {
  programId: PublicKey;
  manager: PublicKey;
  payer: PublicKey;
  indexMint: PublicKey;
  componentMint: PublicKey;
  units: bigint;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  const [vault] = vaultPda(args.programId, args.indexMint, args.componentMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRo(args.manager),
      signerRw(args.payer),
      rw(index),
      ro(args.indexMint),
      ro(args.componentMint),
      rw(vault),
      ro(SystemProgram.programId),
      ro(TOKEN_PROGRAM_ID),
    ],
    data: new Writer().u8(VARIANT.AddComponent).u64(args.units).toBuffer(),
  });
}

export function sealIndex(args: {
  programId: PublicKey;
  manager: PublicKey;
  indexMint: PublicKey;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [signerRo(args.manager), rw(index), ro(args.indexMint)],
    data: new Writer().u8(VARIANT.SealIndex).toBuffer(),
  });
}

/** `components` is `(component mint, the caller's token account for it)`, in table order. */
export type ComponentPair = { mint: PublicKey; tokenAccount: PublicKey };

function componentMetas(
  programId: PublicKey,
  indexMint: PublicKey,
  components: ComponentPair[],
): AccountMeta[] {
  return components.flatMap((component) => [
    rw(vaultPda(programId, indexMint, component.mint)[0]),
    rw(component.tokenAccount),
  ]);
}

export function issue(args: {
  programId: PublicKey;
  user: PublicKey;
  indexMint: PublicKey;
  userIndexAccount: PublicKey;
  feeAccount: PublicKey;
  components: ComponentPair[];
  amount: bigint;
  maxIn: bigint[];
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRo(args.user),
      rw(index),
      rw(args.indexMint),
      rw(args.userIndexAccount),
      rw(args.feeAccount),
      ro(TOKEN_PROGRAM_ID),
      ...componentMetas(args.programId, args.indexMint, args.components),
    ],
    data: new Writer().u8(VARIANT.Issue).u64(args.amount).vecU64(args.maxIn).toBuffer(),
  });
}

export function redeem(args: {
  programId: PublicKey;
  user: PublicKey;
  indexMint: PublicKey;
  userIndexAccount: PublicKey;
  feeAccount: PublicKey;
  components: ComponentPair[];
  amount: bigint;
  minOut: bigint[];
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRo(args.user),
      rw(index),
      rw(args.indexMint),
      rw(args.userIndexAccount),
      rw(args.feeAccount),
      ro(TOKEN_PROGRAM_ID),
      ...componentMetas(args.programId, args.indexMint, args.components),
    ],
    data: new Writer().u8(VARIANT.Redeem).u64(args.amount).vecU64(args.minOut).toBuffer(),
  });
}

export function accrueFees(args: {
  programId: PublicKey;
  indexMint: PublicKey;
  feeAccount: PublicKey;
  componentMints: PublicKey[];
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      rw(index),
      rw(args.indexMint),
      rw(args.feeAccount),
      ro(TOKEN_PROGRAM_ID),
      ...args.componentMints.map((mint) => ro(vaultPda(args.programId, args.indexMint, mint)[0])),
    ],
    data: new Writer().u8(VARIANT.AccrueFees).toBuffer(),
  });
}

export function proposeRebalance(args: {
  programId: PublicKey;
  manager: PublicKey;
  indexMint: PublicKey;
  componentMints: PublicKey[];
  targetUnits: bigint[];
  refPricesE9: bigint[];
  duration: number;
  startPremiumBps: number;
  endPremiumBps: number;
  maxNavLossBps: number;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  const data = new Writer()
    .u8(VARIANT.ProposeRebalance)
    .vecU64(args.targetUnits)
    .vecU64(args.refPricesE9)
    .u32(args.duration)
    .i16(args.startPremiumBps)
    .i16(args.endPremiumBps)
    .u16(args.maxNavLossBps)
    .toBuffer();
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRo(args.manager),
      rw(index),
      ro(args.indexMint),
      ...args.componentMints.map((mint) => ro(vaultPda(args.programId, args.indexMint, mint)[0])),
    ],
    data,
  });
}

export function bid(args: {
  programId: PublicKey;
  bidder: PublicKey;
  indexMint: PublicKey;
  componentMints: PublicKey[];
  bidderReceiveAccount: PublicKey;
  bidderPayAccount: PublicKey;
  sellComponent: number;
  buyComponent: number;
  sellAmount: bigint;
  maxBuyAmount: bigint;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  const data = new Writer()
    .u8(VARIANT.Bid)
    .u8(args.sellComponent)
    .u8(args.buyComponent)
    .u64(args.sellAmount)
    .u64(args.maxBuyAmount)
    .toBuffer();
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRo(args.bidder),
      rw(index),
      ro(args.indexMint),
      rw(args.bidderReceiveAccount),
      rw(args.bidderPayAccount),
      ro(TOKEN_PROGRAM_ID),
      ...args.componentMints.map((mint) => rw(vaultPda(args.programId, args.indexMint, mint)[0])),
    ],
    data,
  });
}

export function endRebalance(args: {
  programId: PublicKey;
  caller: PublicKey;
  indexMint: PublicKey;
  componentMints: PublicKey[];
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [
      signerRo(args.caller),
      rw(index),
      ro(args.indexMint),
      ...args.componentMints.map((mint) => ro(vaultPda(args.programId, args.indexMint, mint)[0])),
    ],
    data: new Writer().u8(VARIANT.EndRebalance).toBuffer(),
  });
}

export function setParams(args: {
  programId: PublicKey;
  governor: PublicKey;
  indexMint: PublicKey;
  streamingFeeBps: number;
  issueFeeBps: number;
  redeemFeeBps: number;
  maxPremiumBps: number;
  rebalanceDelay: number;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  const data = new Writer()
    .u8(VARIANT.SetParams)
    .u16(args.streamingFeeBps)
    .u16(args.issueFeeBps)
    .u16(args.redeemFeeBps)
    .u16(args.maxPremiumBps)
    .u32(args.rebalanceDelay)
    .toBuffer();
  return new TransactionInstruction({
    programId: args.programId,
    keys: [signerRo(args.governor), rw(index), ro(args.indexMint)],
    data,
  });
}

export const ROLE = { governor: 0, manager: 1, feeRecipient: 2 } as const;

export function setAuthority(args: {
  programId: PublicKey;
  governor: PublicKey;
  indexMint: PublicKey;
  role: (typeof ROLE)[keyof typeof ROLE];
  newAuthority: PublicKey;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  const data = new Writer()
    .u8(VARIANT.SetAuthority)
    .u8(args.role)
    .bytes(args.newAuthority.toBuffer())
    .toBuffer();
  return new TransactionInstruction({
    programId: args.programId,
    keys: [signerRo(args.governor), rw(index), ro(args.indexMint)],
    data,
  });
}

export function setPaused(args: {
  programId: PublicKey;
  caller: PublicKey;
  indexMint: PublicKey;
  paused: boolean;
}): TransactionInstruction {
  const [index] = indexPda(args.programId, args.indexMint);
  return new TransactionInstruction({
    programId: args.programId,
    keys: [signerRo(args.caller), rw(index), ro(args.indexMint)],
    data: new Writer().u8(VARIANT.SetPaused).u8(args.paused ? 1 : 0).toBuffer(),
  });
}
