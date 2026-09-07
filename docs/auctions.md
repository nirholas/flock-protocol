# Rebalancing by auction

An index that rebalances by letting its manager swap the fund's assets is an index whose manager can
lose the fund's assets. Flock does not have that instruction. Instead a rebalance is a public Dutch
auction that anyone can fill.

## The proposal

The manager calls `ProposeRebalance` with, per component, a **target unit count** and a **reference
price**, plus an auction duration and a premium band. The program:

1. refuses it if an auction is already running,
2. refuses premiums that are inverted or above the index's governed ceiling,
3. values the fund at the reference prices and stores a **NAV floor** at
   `nav * (1 - maxNavLoss)`,
4. opens the auction at `now + rebalanceDelay`, never sooner.

The delay is the holders' notice period. It cannot be set below 300 seconds, and a real index sets
it in days.

## The price

At any moment the auction has a **premium**, in basis points, decaying linearly from
`startPremiumBps` to `endPremiumBps`:

```text
bidder pays = value taken out x (1 - premium)
```

A negative premium means the bidder pays the fund *more* than the reference value of what they take.
Auctions open negative and decay. The clearing point is wherever some bidder's own execution cost is
covered, which is how the fund discovers what its rebalance is actually worth instead of assuming an
oracle already knew.

## The guards

A bid must:

- take from a component the fund holds **above** its target, and no more than that excess,
- pay in a component the fund holds **below** its cap,
- leave fund NAV, at reference prices, at or above the floor the proposal committed to.

The buy-side cap is target plus the index's premium ceiling, not target exactly. Early in an auction
the bidder overpays, so the leg being bought lands slightly above target; capping at exactly target
would reject precisely the bids that make the fund money.

Together the sell-side cap and the NAV floor bound what any auction can cost holders, and the
sell-side cap makes the auction convergent: every fill moves both legs toward the published targets.

## A worked example

Ten index tokens, backed by 20 whole units of a $1 component and 5 of a $100 one. NAV $520. The
manager wants $250 moved out of the expensive leg:

| | before | target |
|---|---|---|
| A ($1, 6 decimals) | 20 | 270 |
| B ($100, 9 decimals) | 5 | 2.5 |

At the open the premium is -50 bps. A bidder takes 2.5 B, worth $250 at reference, and pays
`250 x 1.005 = $251.25` of A. The fund ends with 271.25 A and 2.5 B: NAV $521.25, up $1.25, and B is
exactly at target. This is the trade `program/tests/lifecycle.rs` runs on chain.

If nobody fills, the premium decays. At +150 bps the same 2.5 B costs the bidder $246.25, and the
fund has given up $3.75 of NAV to get its target composition. If the proposal promised
`maxNavLossBps: 0`, the program refuses that bid: the auction simply expires unfilled and the
manager proposes a better one.

## Closing

Anyone may close an auction that has expired. Only the manager may close one that is still running.
Closing sets every target back to the composition the fund actually has, so the next proposal starts
from the truth.
