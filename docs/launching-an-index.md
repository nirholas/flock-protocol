# Launching an index

This is the path from a methodology to a live token. It uses the CLI; the SDK does the same thing
from code.

## 1. Decide the composition

A methodology answers two questions: which tokens, and at what weights. Flock takes weights in basis
points that must add up to exactly 10000. It refuses a weighting that does not, rather than
normalising it quietly, because a weighting that does not add up is a mistake in the methodology and
silently fixing it hides the mistake.

Screening data for the decision (market cap, real on-chain liquidity, holder count, whether mint and
freeze authority are disabled, and Jupiter's organic-score, which discounts wash volume) is available
from `fetchTokenStats` in the SDK. The two beachhead indexes publish their screens:

- [flock-defi-index](https://github.com/nirholas/flock-defi-index)
- [flock-meme-index](https://github.com/nirholas/flock-meme-index)

## 2. Pick what one token is worth on day one

Weights alone do not size a token. `--nav-per-token 100` says the first index token should be worth
$100, and the CLI converts weights and live prices into the unit counts the program stores.

```bash
flock create \
  --name "Flock DeFi Index" --symbol FDI \
  --weights JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN=3500,<mint>=2500,<mint>=2000,<mint>=1200,<mint>=800 \
  --nav-per-token 100 \
  --streaming-fee-bps 95 \
  --rebalance-delay 86400 \
  --governor <multisig> --manager <committee> --fee-recipient <treasury>
```

That creates the mint with the index PDA as its only authority, initialises the account, adds every
component, seals it, and creates the fee recipient's token account.

## 3. Seed it

The first issuance sets the composition. Anyone can do it, including you:

```bash
flock issue <indexMint> 100          # mints 100 FDI, takes 100 tokens' worth of every component
flock inspect <indexMint>
```

Until the first issuance the stored units are only a recipe. From the first issuance on, they are
derived from what the vaults hold.

## 4. Run a keeper

```bash
flock-keeper --indexes <indexMint> --execute
```

It mints the streaming fee as it accrues, closes expired auctions, and bids when an auction pays
more than it costs. Run it without `--execute` first: it reports every decision it would make and
why it skipped the rest.

## 5. Rebalance on schedule

```bash
flock propose <indexMint> --weights <mint>=3000,... --duration 7200 --dry-run
```

The dry run prints the per-component move. Drop `--dry-run` to publish it. The auction opens after
the index's `rebalanceDelay` and anyone can fill it; see [auctions.md](auctions.md).

## What a launch cannot do

- It cannot add a component after sealing. The component set is fixed; only the weights move.
  A methodology that adds a name reconstitutes into a new index token, which is also how Index Coop
  handles it on Ethereum.
- It cannot set a streaming fee above 5%, an issue or redeem fee above 1%, or a rebalance delay
  below 300 seconds. Those are program constants, not governance parameters.
