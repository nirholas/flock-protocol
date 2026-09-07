import {
  Connection,
  PublicKey,
  type TransactionInstruction,
} from "@solana/web3.js";
import { getAssociatedTokenAddressSync, unpackAccount, unpackMint } from "@solana/spl-token";

import * as ix from "./instructions.js";
import { decodeIndex, type IndexAccount } from "./state.js";
import { indexPda, vaultPda, UNIT_SCALE, BPS } from "./program.js";
import {
  mulDivCeil,
  premiumAt,
  streamingFeeTokens,
  unitsFromBalance,
  unitsIn,
  unitsOut,
  valueE9,
  weightsBps,
} from "./math.js";

export interface IndexSnapshot {
  indexMint: PublicKey;
  address: PublicKey;
  account: IndexAccount;
  supply: bigint;
  /** Vault balances in table order. */
  balances: bigint[];
  /**
   * Units as they stand right now, derived from the vaults. The stored units in the account are a
   * cache the program refreshes on every write; these are always current.
   */
  units: bigint[];
  /** Fee owed but not yet minted, at `asOf`. */
  pendingFee: bigint;
  asOf: number;
}

export interface AuctionView {
  active: boolean;
  opensAt: number;
  closesAt: number;
  /** Null before the auction opens or after it closes. */
  premiumBps: number | null;
  /** Per component: how much the fund may still sell, and how much it may still take in. */
  legs: Array<{
    mint: PublicKey;
    balance: bigint;
    target: bigint;
    sellable: bigint;
    buyable: bigint;
  }>;
}

/**
 * Read side of the protocol, plus transaction assembly.
 *
 * Everything here derives from accounts on chain. Prices are the one input the chain cannot give
 * you, so any method that needs them takes them as an argument rather than reaching for an oracle
 * of its own choosing: the meme index and the DeFi index price their components differently, and
 * an SDK that picked for them would be wrong for one of them.
 */
export class FlockClient {
  constructor(
    readonly connection: Connection,
    readonly programId: PublicKey,
  ) {}

  indexAddress(indexMint: PublicKey): PublicKey {
    return indexPda(this.programId, indexMint)[0];
  }

  vaultAddress(indexMint: PublicKey, componentMint: PublicKey): PublicKey {
    return vaultPda(this.programId, indexMint, componentMint)[0];
  }

  async fetchIndex(indexMint: PublicKey): Promise<IndexAccount> {
    const address = this.indexAddress(indexMint);
    const info = await this.connection.getAccountInfo(address);
    if (!info) throw new Error(`No index at ${address.toBase58()}`);
    if (!info.owner.equals(this.programId)) {
      throw new Error(`${address.toBase58()} is not owned by the index program`);
    }
    return decodeIndex(info.data);
  }

  /** One round trip for the index, its mint and every vault. */
  async snapshot(indexMint: PublicKey, asOf = Math.floor(Date.now() / 1000)): Promise<IndexSnapshot> {
    const address = this.indexAddress(indexMint);
    const first = await this.connection.getAccountInfo(address);
    if (!first) throw new Error(`No index at ${address.toBase58()}`);
    const account = decodeIndex(first.data);

    const vaultKeys = account.components.map((c) => this.vaultAddress(indexMint, c.mint));
    const infos = await this.connection.getMultipleAccountsInfo([indexMint, ...vaultKeys]);
    const mintInfo = infos[0];
    if (!mintInfo) throw new Error("index mint is missing");
    const supply = unpackMint(indexMint, mintInfo).supply;

    const balances = vaultKeys.map((key, i) => {
      const info = infos[i + 1];
      if (!info) throw new Error(`vault ${key.toBase58()} is missing`);
      return unpackAccount(key, info).amount;
    });

    return {
      indexMint,
      address,
      account,
      supply,
      balances,
      units: balances.map((balance) => unitsFromBalance(balance, supply)),
      pendingFee: streamingFeeTokens(supply, account.streamingFeeBps, asOf - account.lastFeeAccrual),
      asOf,
    };
  }

  /**
   * What issuing `amount` index base units costs, per component.
   *
   * The pending streaming fee is applied first, exactly as the program does it, so the quote holds
   * even for a fund nobody has touched in months.
   */
  quoteIssue(snapshot: IndexSnapshot, amount: bigint): { required: bigint[]; feeTokens: bigint } {
    const supplyAfterFee = snapshot.supply + snapshot.pendingFee;
    const units = snapshot.balances.map((balance) => unitsFromBalance(balance, supplyAfterFee));
    const feeTokens = mulDivCeil(amount, BigInt(snapshot.account.issueFeeBps), BPS);
    const gross = amount + feeTokens;
    return { required: units.map((u) => unitsIn(u, gross)), feeTokens };
  }

  quoteRedeem(snapshot: IndexSnapshot, amount: bigint): { payout: bigint[]; feeTokens: bigint } {
    const supplyAfterFee = snapshot.supply + snapshot.pendingFee;
    const units = snapshot.balances.map((balance) => unitsFromBalance(balance, supplyAfterFee));
    const feeTokens = mulDivCeil(amount, BigInt(snapshot.account.redeemFeeBps), BPS);
    const net = amount - feeTokens;
    return { payout: units.map((u) => unitsOut(u, net)), feeTokens };
  }

  /** NAV per whole index token, in USD-e9, given a price per whole component token. */
  navPerToken(snapshot: IndexSnapshot, pricesE9: bigint[]): bigint {
    const decimals = snapshot.account.components.map((c) => c.decimals);
    const { navE9 } = weightsBps(snapshot.balances, decimals, pricesE9);
    if (snapshot.supply === 0n) return 0n;
    return (navE9 * UNIT_SCALE) / snapshot.supply;
  }

  weights(snapshot: IndexSnapshot, pricesE9: bigint[]): { navE9: bigint; weights: number[] } {
    return weightsBps(
      snapshot.balances,
      snapshot.account.components.map((c) => c.decimals),
      pricesE9,
    );
  }

  auction(snapshot: IndexSnapshot, now = Math.floor(Date.now() / 1000)): AuctionView {
    const { rebalance, components, maxPremiumBps } = snapshot.account;
    const open = rebalance.active && now >= rebalance.startTs && now <= rebalance.endTs;
    const legs = components.map((component, i) => {
      const balance = snapshot.balances[i] ?? 0n;
      const target = (component.targetUnits * snapshot.supply) / UNIT_SCALE;
      const cap = target + (target * BigInt(maxPremiumBps)) / BPS;
      return {
        mint: component.mint,
        balance,
        target,
        sellable: balance > target ? balance - target : 0n,
        buyable: cap > balance ? cap - balance : 0n,
      };
    });
    return {
      active: rebalance.active,
      opensAt: rebalance.startTs,
      closesAt: rebalance.endTs,
      premiumBps: open
        ? premiumAt(
            rebalance.startPremiumBps,
            rebalance.endPremiumBps,
            rebalance.startTs,
            rebalance.endTs,
            now,
          )
        : null,
      legs,
    };
  }

  /** Value of the fund at reference prices, in USD-e9. Used for NAV-floor headroom checks. */
  navAtReferencePrices(snapshot: IndexSnapshot): bigint {
    return snapshot.account.components.reduce(
      (total, component, i) =>
        total + valueE9(snapshot.balances[i] ?? 0n, component.decimals, component.refPriceE9),
      0n,
    );
  }

  issueInstruction(args: {
    snapshot: IndexSnapshot;
    user: PublicKey;
    amount: bigint;
    slippageBps?: number;
  }): TransactionInstruction {
    const { snapshot, user, amount } = args;
    const { required } = this.quoteIssue(snapshot, amount);
    const slippage = BigInt(args.slippageBps ?? 50);
    const maxIn = required.map((value) => value + (value * slippage) / BPS + 1n);
    return ix.issue({
      programId: this.programId,
      user,
      indexMint: snapshot.indexMint,
      userIndexAccount: getAssociatedTokenAddressSync(snapshot.indexMint, user),
      feeAccount: getAssociatedTokenAddressSync(snapshot.account.feeRecipient, user, true),
      components: snapshot.account.components.map((component) => ({
        mint: component.mint,
        tokenAccount: getAssociatedTokenAddressSync(component.mint, user),
      })),
      amount,
      maxIn,
    });
  }

  redeemInstruction(args: {
    snapshot: IndexSnapshot;
    user: PublicKey;
    amount: bigint;
    slippageBps?: number;
    feeAccount: PublicKey;
  }): TransactionInstruction {
    const { snapshot, user, amount } = args;
    const { payout } = this.quoteRedeem(snapshot, amount);
    const slippage = BigInt(args.slippageBps ?? 50);
    const minOut = payout.map((value) => value - (value * slippage) / BPS);
    return ix.redeem({
      programId: this.programId,
      user,
      indexMint: snapshot.indexMint,
      userIndexAccount: getAssociatedTokenAddressSync(snapshot.indexMint, user),
      feeAccount: args.feeAccount,
      components: snapshot.account.components.map((component) => ({
        mint: component.mint,
        tokenAccount: getAssociatedTokenAddressSync(component.mint, user),
      })),
      amount,
      minOut,
    });
  }
}
