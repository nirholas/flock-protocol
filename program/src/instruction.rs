//! The instruction set, and the account order each one expects.
//!
//! Account lists are written out here rather than only in the SDK, because the SDK is a
//! convenience and this file is the contract. A client in any language can be written from it.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum FlockInstruction {
    /// Create an index around an existing mint whose authority is already the index PDA.
    ///
    /// 0. `[signer, writable]` payer
    /// 1. `[writable]` index PDA, seeds `["index", index_mint]`
    /// 2. `[]` index mint: 9 decimals, zero supply, no freeze authority, mint authority = index PDA
    /// 3. `[]` governor
    /// 4. `[]` manager
    /// 5. `[]` fee recipient
    /// 6. `[]` system program
    InitIndex {
        name: [u8; 32],
        symbol: [u8; 12],
        streaming_fee_bps: u16,
        issue_fee_bps: u16,
        redeem_fee_bps: u16,
        max_premium_bps: u16,
        rebalance_delay: u32,
    },

    /// Add a component and create its vault. Only while bootstrapping.
    ///
    /// 0. `[signer]` manager
    /// 1. `[signer, writable]` payer
    /// 2. `[writable]` index PDA
    /// 3. `[]` index mint
    /// 4. `[]` component mint
    /// 5. `[writable]` vault PDA, seeds `["vault", index_mint, component_mint]`
    /// 6. `[]` system program
    /// 7. `[]` token program
    /// 8. `[]` rent sysvar
    AddComponent { units: u64 },

    /// Close the component list and open the index for issuance.
    ///
    /// 0. `[signer]` manager
    /// 1. `[writable]` index PDA
    SealIndex,

    /// Deposit every component in proportion and receive `amount` index tokens.
    ///
    /// 0. `[signer]` user
    /// 1. `[writable]` index PDA
    /// 2. `[writable]` index mint
    /// 3. `[writable]` user's index token account
    /// 4. `[writable]` fee recipient's index token account
    /// 5. `[]` token program
    /// 6+. per component, in table order: `[writable]` vault, `[writable]` user source account
    Issue { amount: u64, max_in: Vec<u64> },

    /// Burn `amount` index tokens and withdraw every component in proportion.
    ///
    /// Same account layout as `Issue`, with the user's accounts receiving rather than sending.
    Redeem { amount: u64, min_out: Vec<u64> },

    /// Mint the streaming fee accrued since the last call to the fee recipient.
    ///
    /// 0. `[writable]` index PDA
    /// 1. `[writable]` index mint
    /// 2. `[writable]` fee recipient's index token account
    /// 3. `[]` token program
    AccrueFees,

    /// Open a Dutch auction that moves the fund from its current units to `target_units`.
    ///
    /// 0. `[signer]` manager
    /// 1. `[writable]` index PDA
    /// 2. `[]` index mint
    /// 3+. per component, in table order: `[]` vault
    ProposeRebalance {
        target_units: Vec<u64>,
        ref_prices_e9: Vec<u64>,
        duration: u32,
        start_premium_bps: i16,
        end_premium_bps: i16,
        max_nav_loss_bps: u16,
    },

    /// Buy `sell_amount` of one component out of the fund, paying in another, at the auction's
    /// current price. Permissionless.
    ///
    /// 0. `[signer]` bidder
    /// 1. `[writable]` index PDA
    /// 2. `[]` index mint
    /// 3. `[writable]` bidder's account receiving the sold component
    /// 4. `[writable]` bidder's account paying the bought component
    /// 5. `[]` token program
    /// 6+. per component, in table order: `[writable]` vault
    Bid {
        sell_component: u8,
        buy_component: u8,
        sell_amount: u64,
        max_buy_amount: u64,
    },

    /// Close the auction. Permissionless once it has expired; the manager may close it early.
    ///
    /// 0. `[signer]` caller
    /// 1. `[writable]` index PDA
    /// 2. `[]` index mint
    /// 3+. per component, in table order: `[]` vault
    EndRebalance,

    /// Governor: update the fee schedule and auction bounds.
    ///
    /// 0. `[signer]` governor
    /// 1. `[writable]` index PDA
    SetParams {
        streaming_fee_bps: u16,
        issue_fee_bps: u16,
        redeem_fee_bps: u16,
        max_premium_bps: u16,
        rebalance_delay: u32,
    },

    /// Governor: hand a role to a new key. Role 0 governor, 1 manager, 2 fee recipient.
    ///
    /// 0. `[signer]` governor
    /// 1. `[writable]` index PDA
    SetAuthority { role: u8, new_authority: Pubkey },

    /// Governor or manager: stop issuance and bidding. Redemption stays open, always.
    ///
    /// 0. `[signer]` governor or manager
    /// 1. `[writable]` index PDA
    SetPaused { paused: bool },
}

pub const ROLE_GOVERNOR: u8 = 0;
pub const ROLE_MANAGER: u8 = 1;
pub const ROLE_FEE_RECIPIENT: u8 = 2;
