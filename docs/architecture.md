# How a Flock index works

An index is one account, one mint, and a set of vaults. Everything else follows from those three.

## The account

`Index` (PDA, seeds `["index", indexMint]`) holds the authorities, the fee schedule, a fixed table
of up to sixteen components, and whatever auction is running. It is 1304 bytes, allocated once, and
read in place: the program never deserializes it into a local, because a 1.3 KB struct copied a few
times overflows the 4 KB stack frame an SBF program gets.

## The mint

The index token has 9 decimals. Its mint authority is the index PDA and it has **no freeze
authority at all**, checked at creation. A freeze authority in anyone's hands is the ability to stop
a holder redeeming, which is the one exposure an index holder must never have.

## The vaults

One token account per component, `["vault", indexMint, componentMint]`, owned by the index PDA. The
program is the only thing that can move them, and it will only move them for issuance, redemption,
or a bid that pays for what it takes.

## Units, and why they are derived

A component's `units` is how many of its base units back one whole index token. The invariant is:

```text
for every component i:  vault_balance[i] >= units[i] * index_supply / 1e9
```

The program never asserts this. It holds because `units` is recomputed from the vault balance after
every state change, and because what a caller must deliver rounds up while what a caller receives
rounds down. Rounding dust therefore accumulates in the vault, in favour of holders, and per-token
backing is monotonically non-decreasing except where a fee is charged on purpose.

That is also why the stored `units` in the account is only a cache. Anything that needs a current
number derives it from the vault, which is what `FlockClient.snapshot()` returns.

## Issuance and redemption are in kind

To mint 10 index tokens you deliver 10 tokens' worth of every component; to redeem you receive them
back. Nothing is bought or sold, so:

- the fund never eats slippage to let somebody in or out,
- redemption does not need a liquid market for the index token,
- and redemption can stay open while the index is paused, which it does.

## Fees

- **Streaming fee**, up to 5% a year, charged as dilution. Over `dt` the recipient should end up
  holding `fee * dt / year` of the fund; minting exactly that fraction of the *old* supply would
  undercharge, because the mint grows the denominator, so it is grossed up. Same construction as Set
  Protocol's StreamingFeeModule.
- **Issue and redeem fees**, up to 1% each, charged in index tokens and backed by components the
  caller delivered, so charging one never dilutes anyone already holding.

Fees accrue at the top of every issue and redeem, so nobody enters or exits at a composition that
still owes the manager time. Raising the fee settles the clock first, so the new rate can never bill
the period before it.

## Authorities

| Role | Can | Cannot |
|---|---|---|
| Governor | set fees inside the hard caps, move any role, resume a paused index | move fund assets |
| Manager | add components before seal, propose a rebalance, pause | set fees, move fund assets, trade |
| Anyone | issue, redeem, accrue fees, bid, close an expired auction | everything above |

Either authority can pause. Only the governor can resume: stopping is urgent, restarting never is.

## Rebalancing

See [auctions.md](auctions.md). The short version: the manager publishes a target composition and
reference prices behind a timelock, and outside bidders trade against the fund at a price that
decays from favouring the fund to favouring them. The manager never touches the assets.
