# Integrating

`@flock/sdk` is the only dependency you need to read an index, quote an issuance, or bid.

## Read

```ts
import { Connection, PublicKey } from "@solana/web3.js";
import { FlockClient, fetchPricesE9 } from "@flock/sdk";

const client = new FlockClient(new Connection(rpcUrl), new PublicKey(programId));
const snapshot = await client.snapshot(new PublicKey(indexMint));

const prices = await fetchPricesE9(snapshot.account.components.map((c) => c.mint.toBase58()));
const { navE9, weights } = client.weights(snapshot, prices);

console.log(snapshot.account.symbol, Number(client.navPerToken(snapshot, prices)) / 1e9);
weights.forEach((bps, i) => console.log(snapshot.account.components[i].mint.toBase58(), bps / 100));
```

`snapshot()` is one round trip for the index, its mint and every vault. `units` on the snapshot are
derived from the vaults and are always current; `pendingFee` is the streaming fee owed but not yet
minted, computed exactly the way the program computes it.

## Issue and redeem

```ts
const amount = 10n ** 9n * 5n; // 5 whole index tokens
const { required, feeTokens } = client.quoteIssue(snapshot, amount);
const instruction = client.issueInstruction({ snapshot, user: wallet.publicKey, amount, slippageBps: 50 });
```

`quoteIssue` applies the pending streaming fee first, exactly as the program does, so the quote holds
even for a fund nobody has touched in months. The caller needs a token account for every component
and enough of each; `required` is in each component's own base units.

Redemption is the mirror, and it works while the index is paused.

## Bid into an auction

```ts
import { bidCost, instructions, premiumAt } from "@flock/sdk";

const view = client.auction(snapshot);
if (view.premiumBps !== null) {
  const quote = bidCost({
    sellAmount,
    sellDecimals: sell.decimals,
    sellPriceE9: sell.refPriceE9,   // the auction's reference price, not the market's
    buyDecimals: buy.decimals,
    buyPriceE9: buy.refPriceE9,
    premiumBps: view.premiumBps,
  });
  // Compare quote.buyAmount at market prices with sellAmount at market prices. The difference is
  // the edge, before your own execution cost.
}
```

`view.legs[i].sellable` and `.buyable` are the exact sizes the program will accept right now.

## Errors

Every custom error the program can return has a sentence in `FLOCK_ERRORS`. Wrap sends in
`explainError` and a failed simulation reads as "That component is already at or below its target:
there is nothing to sell" rather than `custom program error: 0x14`.

## Prices

`fetchPrices`, `fetchPricesE9` and `fetchTokenStats` call Jupiter's public API: no key, no account.
Prices come back as 1e9 fixed point because everything downstream has to agree with a program that
does integer arithmetic. `fetchTokenStats` additionally returns the screening fields a methodology
needs, including `mintAuthorityDisabled`, `freezeAuthorityDisabled`, `topHoldersPercentage` and
`organicScore`.
