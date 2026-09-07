# Flock

**An index cooperative on Solana.** One program that turns a published methodology into a token
anyone can hold, issue, redeem and rebalance, with no privileged party able to touch the assets.

The beachhead products are two indexes:

| Index | Repo | What it holds |
|---|---|---|
| Flock DeFi Index (FDI) | [flock-defi-index](https://github.com/nirholas/flock-defi-index) | The Solana DeFi bluechips, liquidity-adjusted and capped |
| Flock Meme Index (FMI) | [flock-meme-index](https://github.com/nirholas/flock-meme-index) | The Solana memecoins that survive a hard safety and organic-volume screen |

This repository is the engine both of them run on.

## Why this design

Three decisions do most of the work.

**Issuance and redemption are in kind.** You deliver every component to mint, and receive every
component to redeem. The fund never sells anything to let somebody in or out, so it eats no
slippage, needs no liquid market for its own token, and can keep redemption open even while it is
paused. It always is: a pause stops issuance and bidding, never the exit.

**Backing is derived, not tracked.** How much of each component backs one index token is recomputed
from the vault balances after every state change, and deliveries round up while payouts round down.
The invariant `vault >= units * supply / 1e9` is therefore a consequence of the arithmetic rather
than an assertion that could be missed on a new code path.

**The manager cannot trade the fund.** There is no instruction for it. A rebalance is published as a
Dutch auction: the manager sets target units and reference prices behind a timelock, and outside
bidders trade against the fund at a premium that decays from favouring the fund to favouring them,
bounded by a NAV floor the proposal commits to. Price discovery is done by whoever shows up with
capital, not by the manager and not by an oracle.

## Layout

```
program/          the Solana program (native Rust, no framework), its tests, and the parity fixtures
packages/sdk/     @flock/sdk: PDAs, instruction builders, account decoding, index math, prices
apps/cli/         flock: create, inspect, issue, redeem, propose, bid
apps/keeper/      flock-keeper: accrues fees, closes expired auctions, bids the profitable ones
docs/             architecture, auctions, launching an index, integrating, trust model
scripts/deploy.mjs deploys the program and records the id and binary hash
```

## Getting started

```bash
pnpm install
cd program && cargo build-sbf && cargo test      # 23 tests: unit, lifecycle, adversarial
cd .. && pnpm -r build && pnpm -r test           # SDK, CLI and keeper
```

The program tests run against the real `flock_index.so` in a real SVM, with the real SPL token
program. Nothing in this repository is mocked.

Then, against any cluster:

```bash
node scripts/deploy.mjs --cluster devnet
export FLOCK_PROGRAM_ID=$(jq -r .programId deployments/devnet.json)

flock create --name "My Index" --symbol MYI --weights <mint>=6000,<mint>=4000 --nav-per-token 100
flock issue <indexMint> 10
flock inspect <indexMint>
```

## Status

Unaudited, and deployed nowhere. `deployments/` is empty for that reason and every app reads the
program id from an environment variable rather than a constant nobody can verify. Read
[docs/security.md](docs/security.md) before putting anything real in it.

## Reading order

1. [docs/architecture.md](docs/architecture.md) - the account, the invariant, the fees
2. [docs/auctions.md](docs/auctions.md) - how a rebalance actually clears, with a worked example
3. [docs/launching-an-index.md](docs/launching-an-index.md) - methodology to live token
4. [docs/integrating.md](docs/integrating.md) - the SDK
5. [docs/security.md](docs/security.md) - what a compromised key gets you

MIT.
