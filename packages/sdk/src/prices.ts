/**
 * Market data for Solana components, from Jupiter's public API.
 *
 * Two endpoints, two jobs. `price/v3` is the pricing an index uses to value itself and to set the
 * reference prices an auction is scored against. `tokens/v2` is the screening data a methodology
 * uses to decide what belongs in the index at all: real market cap, on-chain liquidity, holder
 * count, whether mint and freeze authority are actually disabled, and Jupiter's organic-score,
 * which discounts wash and bot volume. No key, no account, generous public rate limits.
 *
 * Prices arrive as floats and are converted to 1e9 fixed point immediately, because everything
 * downstream of this file is integer arithmetic that has to agree with a program.
 */

const PRICE_ENDPOINT = "https://lite-api.jup.ag/price/v3";
const TOKENS_ENDPOINT = "https://lite-api.jup.ag/tokens/v2/search";
const PRICE_BATCH = 50;
const TOKEN_BATCH = 100;

export type FetchLike = (url: string) => Promise<{ ok: boolean; status: number; json: () => Promise<unknown> }>;

export interface PriceQuote {
  mint: string;
  usdPrice: number;
  priceE9: bigint;
  decimals: number;
  liquidityUsd: number;
  priceChange24h: number;
  blockId: number;
}

export interface TokenStats {
  mint: string;
  symbol: string;
  name: string;
  decimals: number;
  usdPrice: number;
  priceE9: bigint;
  marketCapUsd: number;
  fdvUsd: number;
  circulatingSupply: number;
  liquidityUsd: number;
  holderCount: number;
  organicScore: number;
  organicScoreLabel: string;
  isVerified: boolean;
  tags: string[];
  tokenProgram: string;
  firstPoolCreatedAt: string | null;
  mintAuthorityDisabled: boolean;
  freezeAuthorityDisabled: boolean;
  topHoldersPercentage: number | null;
  devMints: number | null;
  volume24hUsd: number;
  organicVolume24hUsd: number;
  priceChange24h: number;
}

/** USD per whole token as 1e9 fixed point. Rounds to nearest, and never returns zero for a real price. */
export function toPriceE9(usdPrice: number): bigint {
  if (!Number.isFinite(usdPrice) || usdPrice <= 0) {
    throw new Error(`cannot price a token at ${usdPrice}`);
  }
  const scaled = BigInt(Math.round(usdPrice * 1e9));
  return scaled === 0n ? 1n : scaled;
}

function chunk<T>(items: T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size));
  return out;
}

async function getJson(url: string, fetchImpl: FetchLike, attempts = 3): Promise<unknown> {
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const response = await fetchImpl(url);
      if (response.ok) return await response.json();
      // 429 and 5xx are worth another try; a 400 means the request itself is wrong.
      if (response.status < 500 && response.status !== 429) {
        throw new Error(`Jupiter returned ${response.status} for ${url}`);
      }
      lastError = new Error(`Jupiter returned ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 250 * 2 ** attempt));
  }
  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}

export function parsePriceResponse(payload: unknown): Map<string, PriceQuote> {
  const out = new Map<string, PriceQuote>();
  if (typeof payload !== "object" || payload === null) return out;
  for (const [mint, value] of Object.entries(payload as Record<string, Record<string, unknown>>)) {
    const usdPrice = Number(value.usdPrice);
    if (!Number.isFinite(usdPrice) || usdPrice <= 0) continue;
    out.set(mint, {
      mint,
      usdPrice,
      priceE9: toPriceE9(usdPrice),
      decimals: Number(value.decimals ?? 0),
      liquidityUsd: Number(value.liquidity ?? 0),
      priceChange24h: Number(value.priceChange24h ?? 0),
      blockId: Number(value.blockId ?? 0),
    });
  }
  return out;
}

export function parseTokenResponse(payload: unknown): Map<string, TokenStats> {
  const out = new Map<string, TokenStats>();
  if (!Array.isArray(payload)) return out;
  for (const raw of payload as Array<Record<string, any>>) {
    const mint = String(raw.id ?? "");
    const usdPrice = Number(raw.usdPrice ?? 0);
    if (!mint || !Number.isFinite(usdPrice) || usdPrice <= 0) continue;
    const stats24h = (raw.stats24h ?? {}) as Record<string, number>;
    const audit = (raw.audit ?? {}) as Record<string, unknown>;
    out.set(mint, {
      mint,
      symbol: String(raw.symbol ?? ""),
      name: String(raw.name ?? ""),
      decimals: Number(raw.decimals ?? 0),
      usdPrice,
      priceE9: toPriceE9(usdPrice),
      marketCapUsd: Number(raw.mcap ?? 0),
      fdvUsd: Number(raw.fdv ?? 0),
      circulatingSupply: Number(raw.circSupply ?? 0),
      liquidityUsd: Number(raw.liquidity ?? 0),
      holderCount: Number(raw.holderCount ?? 0),
      organicScore: Number(raw.organicScore ?? 0),
      organicScoreLabel: String(raw.organicScoreLabel ?? "unknown"),
      isVerified: Boolean(raw.isVerified),
      tags: Array.isArray(raw.tags) ? raw.tags.map(String) : [],
      tokenProgram: String(raw.tokenProgram ?? ""),
      firstPoolCreatedAt: raw.firstPool?.createdAt ? String(raw.firstPool.createdAt) : null,
      mintAuthorityDisabled: audit.mintAuthorityDisabled === true,
      freezeAuthorityDisabled: audit.freezeAuthorityDisabled === true,
      topHoldersPercentage:
        typeof audit.topHoldersPercentage === "number" ? audit.topHoldersPercentage : null,
      devMints: typeof audit.devMints === "number" ? audit.devMints : null,
      volume24hUsd: Number(stats24h.buyVolume ?? 0) + Number(stats24h.sellVolume ?? 0),
      organicVolume24hUsd:
        Number(stats24h.buyOrganicVolume ?? 0) + Number(stats24h.sellOrganicVolume ?? 0),
      priceChange24h: Number(stats24h.priceChange ?? 0),
    });
  }
  return out;
}

export async function fetchPrices(
  mints: string[],
  fetchImpl: FetchLike = globalThis.fetch as unknown as FetchLike,
): Promise<Map<string, PriceQuote>> {
  const out = new Map<string, PriceQuote>();
  for (const batch of chunk([...new Set(mints)], PRICE_BATCH)) {
    const payload = await getJson(`${PRICE_ENDPOINT}?ids=${batch.join(",")}`, fetchImpl);
    for (const [mint, quote] of parsePriceResponse(payload)) out.set(mint, quote);
  }
  return out;
}

/** Prices in table order, throwing if any component could not be priced. Auctions need all of them. */
export async function fetchPricesE9(
  mints: string[],
  fetchImpl: FetchLike = globalThis.fetch as unknown as FetchLike,
): Promise<bigint[]> {
  const quotes = await fetchPrices(mints, fetchImpl);
  return mints.map((mint) => {
    const quote = quotes.get(mint);
    if (!quote) throw new Error(`No price for ${mint}; refusing to value an index without one`);
    return quote.priceE9;
  });
}

export async function fetchTokenStats(
  mints: string[],
  fetchImpl: FetchLike = globalThis.fetch as unknown as FetchLike,
): Promise<Map<string, TokenStats>> {
  const out = new Map<string, TokenStats>();
  for (const batch of chunk([...new Set(mints)], TOKEN_BATCH)) {
    const payload = await getJson(`${TOKENS_ENDPOINT}?query=${batch.join(",")}`, fetchImpl);
    for (const [mint, stats] of parseTokenResponse(payload)) out.set(mint, stats);
  }
  return out;
}
