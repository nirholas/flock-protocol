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
pnpm program:test        # 24 tests: unit, lifecycle, adversarial, cross-language fixtures
pnpm -r build && pnpm -r test   # SDK, CLI and keeper
```

The program tests run against the real `flock_index.so` in a real SVM, with the real SPL token
program. Nothing in this repository is mocked.

Then, against any cluster:

```bash
pnpm deploy -- --cluster devnet
export FLOCK_PROGRAM_ID=$(jq -r .programId deployments/devnet.json)

flock create --name "My Index" --symbol MYI --weights <mint>=6000,<mint>=4000 --nav-per-token 100
flock issue <indexMint> 10
flock inspect <indexMint>
```

The deploy script builds the program itself, for the SBPF version a cluster will actually execute.
See [program/README.md](program/README.md) for why that is not the toolchain's default.

## Status

Unaudited, and not deployed to a public cluster. `deployments/` is empty for that reason and every
app reads the program id from an environment variable rather than a constant nobody can verify.
Read [docs/security.md](docs/security.md) before putting anything real in it.

It has been run end to end against a local validator, which is how the two bugs the unit tests
could not see were found: the first issuance quoted zero (units were derived from vaults that are
empty until somebody issues, rather than from the seed recipe), and the fee token account was
derived with its mint and owner the wrong way round. Both are fixed and both now have regression
tests. The lifecycle that run exercised, with real SPL mints and the deployed program:

```
create   ->  index mint, account, two components, sealed
issue    ->  pays 20.000000 of a 6-decimal component and 5.000000 of a 9-decimal one for 10 tokens
inspect  ->  supply 10.000000, units 2.000000 and 0.500000 per token
redeem   ->  returns 7.999996 and 1.999999 for 4 tokens
```

The last line is the streaming fee, visible in real numbers: 95 bps had been accruing while the
session ran, so four tokens redeemed for slightly less than four tokens' worth of the recipe.

## Reading order

1. [docs/architecture.md](docs/architecture.md) - the account, the invariant, the fees
2. [docs/auctions.md](docs/auctions.md) - how a rebalance actually clears, with a worked example
3. [docs/launching-an-index.md](docs/launching-an-index.md) - methodology to live token
4. [docs/integrating.md](docs/integrating.md) - the SDK
5. [docs/security.md](docs/security.md) - what a compromised key gets you

MIT.
