/**
 * Borsh encoding for the program's instruction enum.
 *
 * Written out by hand rather than pulled from a schema library: the whole surface is twelve
 * variants of fixed scalars, and a hand-written encoder is checked byte for byte against the Rust
 * builders by `test/parity.test.ts`. A schema that only agrees with itself would not be.
 */

export class Writer {
  private chunks: Buffer[] = [];

  u8(value: number): this {
    const b = Buffer.alloc(1);
    b.writeUInt8(value);
    return this.push(b);
  }

  u16(value: number): this {
    const b = Buffer.alloc(2);
    b.writeUInt16LE(value);
    return this.push(b);
  }

  i16(value: number): this {
    const b = Buffer.alloc(2);
    b.writeInt16LE(value);
    return this.push(b);
  }

  u32(value: number): this {
    const b = Buffer.alloc(4);
    b.writeUInt32LE(value);
    return this.push(b);
  }

  u64(value: bigint | number): this {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(BigInt(value));
    return this.push(b);
  }

  bytes(value: Buffer | Uint8Array): this {
    return this.push(Buffer.from(value));
  }

  /** A fixed-width byte field, right-padded with zeroes, the way the account stores names. */
  fixed(text: string, length: number): this {
    const b = Buffer.alloc(length);
    b.write(text.slice(0, length), "utf8");
    return this.push(b);
  }

  vecU64(values: Array<bigint | number>): this {
    this.u32(values.length);
    for (const value of values) this.u64(value);
    return this;
  }

  private push(b: Buffer): this {
    this.chunks.push(b);
    return this;
  }

  toBuffer(): Buffer {
    return Buffer.concat(this.chunks);
  }
}

export const VARIANT = {
  InitIndex: 0,
  AddComponent: 1,
  SealIndex: 2,
  Issue: 3,
  Redeem: 4,
  AccrueFees: 5,
  ProposeRebalance: 6,
  Bid: 7,
  EndRebalance: 8,
  SetParams: 9,
  SetAuthority: 10,
  SetPaused: 11,
} as const;
