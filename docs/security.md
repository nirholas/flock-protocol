# Trust model

## What the manager can do

Propose a target composition, pause the index, and add components before it is sealed. That is all.
There is no instruction that lets a manager, a governor, or the program's deployer move a component
out of a vault except through a bid that pays for it or a redemption that burns for it.

## What a compromised manager key gets you

A bad proposal. The bound on the damage is the index's own `maxPremiumBps` (never above 10%) and the
proposal's `maxNavLossBps`, both enforced on every fill, plus the `rebalanceDelay` before the first
bid can land. In the worst case an attacker who holds the manager key can propose a maximally
unfavourable composition and let it be filled, costing holders at most the NAV loss the proposal
declared. They cannot take custody of anything.

The mitigations are the ones the parameters exist for: keep `maxPremiumBps` tight, keep
`rebalanceDelay` long enough that a human sees the proposal, and pause on anything unexpected. Any
holder can redeem at any time, including while paused, so nobody is trapped inside a bad proposal.

## What a compromised governor key gets you

Fees up to the hard caps (5% a year streaming, 1% each way), and the ability to hand the roles to
someone else. Not custody. A governor cannot un-pause faster than a human can redeem, and cannot
touch a vault.

## What the program checks

Every account it reads or writes:

- PDAs are re-derived and compared, so a substituted vault fails.
- Every token account is checked for both mint and owner, so a vault the attacker owns is not a
  vault and a fee account that belongs to someone else is refused the moment a fee is payable.
- The index mint must have 9 decimals, zero supply, the index PDA as mint authority, and no freeze
  authority. An index whose mint anyone else can print is refused at creation.
- Arithmetic is checked, and rounds against the caller in both directions.

`program/tests/security.rs` runs each of these as the attack it prevents, not as a unit test of the
guard.

## What is not covered

- **Component risk.** An index holds what its methodology says it holds. If a component's issuer
  freezes accounts or rugs, the index holds that. Screening is the methodology's job, and both
  beachhead indexes publish theirs.
- **Price risk in an auction.** Reference prices come from the proposal. A proposal made with stale
  prices prices its auction wrongly, bounded by the NAV floor and the premium ceiling.
- **No formal audit yet.** The program is unaudited. It is deployed nowhere as of this writing;
  `deployments/` is empty for exactly that reason.
