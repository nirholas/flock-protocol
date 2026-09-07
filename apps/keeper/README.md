# flock-keeper

Keeps an index honest without being trusted by it. It does three things:

1. **Accrues the streaming fee** once it is worth a transaction fee.
2. **Closes expired auctions**, so the next proposal can be made.
3. **Bids into auctions** that are paying more than they cost.

```bash
pnpm start -- --indexes <indexMint>,<indexMint>            # report only
pnpm start -- --indexes <indexMint> --execute              # send transactions
pnpm start -- --indexes <indexMint> --once                 # one pass and exit
```

Without `--execute` it prints every action it would take and, just as usefully, why it skipped the
rest: an auction that opens in 500 seconds, a pair paying 40 bps against a 25 bps floor, a fee too
small to be worth minting.

## Policy

| Flag | Default | Meaning |
|---|---|---|
| `--min-edge-bps` | 25 | Do not bid under this market edge, before your own execution cost |
| `--min-accrual` | 1000000 | Do not pay a transaction fee to mint less than this many base units |
| `--max-leg-share-bps` | 10000 | Most of a leg to take in a single bid |
| `--interval` | 60 | Seconds between passes |

## It bids from inventory

The keeper only plans bids it can pay for out of tokens it already holds, and sizes them down to
that balance. Sourcing inventory (a swap on any venue) is the bidder's own business and is
deliberately not wired in: a keeper that swaps to chase every auction is a strategy, and strategies
belong to whoever runs them rather than to the protocol's reference operator. `flock bid --dry-run`
prints exactly which leg and how much a bidder would need.

## Why the decisions are pure functions

`src/planner.ts` decides everything from a snapshot, a set of prices, a clock and an inventory, with
no network of its own. The cases that matter (an auction not yet worth taking, a fee below the
floor, a bid that must shrink to fit the balance on hand) are the painful ones to reproduce against
a live cluster, and `test/planner.test.ts` runs all of them in milliseconds against the same numbers
the on-chain lifecycle test asserts.
