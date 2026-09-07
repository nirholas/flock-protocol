//! Flock, an index cooperative on Solana.
//!
//! One program, one account per index. An index holds a fixed table of SPL components in vaults it
//! owns, mints a 9-decimal token backed by them, and moves between compositions through a
//! permissionless Dutch auction rather than a privileged swap.
//!
//! Three properties are worth stating up front, because they are what the design is for.
//!
//! 1. **Issuance and redemption are in kind.** Nobody sells anything to enter or leave, so the
//!    fund never eats slippage on a subscription and never needs a liquid market to honor a
//!    redemption. Redemption stays open even while the index is paused.
//! 2. **The manager cannot move user funds.** The manager may propose a target composition. It
//!    cannot swap, withdraw, or trade against the fund. Every rebalance trade is executed by an
//!    outside bidder who pays for what they take, at a price the auction discovers.
//! 3. **Backing is derived, not tracked.** Component units are recomputed from vault balances on
//!    every state change, which is what makes the backing invariant a consequence of the code
//!    rather than an assertion in it.

pub mod builders;
pub mod error;
pub mod guards;
pub mod instruction;
pub mod math;
pub mod processor;
pub mod state;

#[cfg(not(feature = "no-entrypoint"))]
mod entry {
    use solana_program::{account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey};

    entrypoint!(process_instruction);
    fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
        crate::processor::process(program_id, accounts, data)
    }
}
