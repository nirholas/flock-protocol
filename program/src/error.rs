//! Every failure mode this program can hit, with a distinct code.
//!
//! A bidder or a keeper that gets a bare `0x1` back from a simulation learns nothing and guesses.
//! Numbered variants are cheap; the SDK maps each one to a sentence and the keeper logs it.

use solana_program::program_error::ProgramError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FlockError {
    /// The account is not owned by this program, or holds the wrong discriminator.
    InvalidAccountOwner = 0,
    /// A PDA argument did not match the address derived from its seeds.
    SeedsMismatch = 1,
    /// A required signature was missing.
    MissingSignature = 2,
    /// The signer is not the governor.
    NotGovernor = 3,
    /// The signer is not the manager.
    NotManager = 4,
    /// The index has already been sealed and cannot take new components.
    AlreadySealed = 5,
    /// The action requires a sealed (live) index.
    NotSealed = 6,
    /// Issuance and rebalancing are paused. Redemption never is.
    Paused = 7,
    /// The component list is full.
    TooManyComponents = 8,
    /// The mint is already a component of this index.
    DuplicateComponent = 9,
    /// Component index out of range.
    UnknownComponent = 10,
    /// A token account's mint or owner is not the one this instruction requires.
    InvalidTokenAccount = 11,
    /// The index mint must have 9 decimals, no freeze authority, and the index PDA as mint authority.
    InvalidIndexMint = 12,
    /// A fee parameter exceeds the hard cap this program enforces.
    FeeTooHigh = 13,
    /// Arithmetic overflowed. Never expected; the guard exists so it cannot pass silently.
    MathOverflow = 14,
    /// Issuance or redemption of zero tokens.
    ZeroAmount = 15,
    /// The caller's slippage bound was crossed.
    SlippageExceeded = 16,
    /// A rebalance is already running.
    RebalanceActive = 17,
    /// No rebalance is running.
    NoRebalance = 18,
    /// The auction has not opened yet, or has already closed.
    AuctionClosed = 19,
    /// The component the bid wants to buy from the fund is already at or below its target.
    SellLegAtTarget = 20,
    /// The bid would push fund NAV below the floor the proposal committed to.
    NavFloorBreached = 21,
    /// Auction premium bounds are inverted, or exceed the index's cap.
    InvalidPremium = 22,
    /// A supplied array does not have one entry per component.
    ArityMismatch = 23,
    /// A reference price of zero cannot value anything.
    InvalidPrice = 24,
    /// The rebalance timelock has not elapsed, or the requested delay is below the floor.
    TimelockNotMet = 25,
    /// Redeeming this many tokens would leave the fund with a supply too small to price units against.
    SupplyTooSmall = 26,
    /// The index still holds a balance for a component that is being valued at zero.
    StaleProposal = 27,
    /// A bid must move two different components.
    SameComponent = 28,
    /// The bid would sell more than the fund holds above target for that component.
    SellAmountExceedsExcess = 29,
    /// The component the bid pays in is already at the cap the auction may raise it to.
    BuyLegAtCap = 30,
    /// The payment would push the bought component past that cap.
    BuyAmountExceedsCap = 31,
}

impl From<FlockError> for ProgramError {
    fn from(e: FlockError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
