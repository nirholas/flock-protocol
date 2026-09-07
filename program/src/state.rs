//! Account state, laid out for zero-copy access.
//!
//! An index account is 1304 bytes and every instruction touches it. Deserializing that into a
//! local would eat a third of the 4 KB SBF stack frame per copy, and the compiler makes several,
//! so the whole record is read and written in place through `bytemuck` instead. That is why every
//! field here is 8-byte aligned or smaller with explicit padding, and why nothing in this file is
//! a `Vec` or an `Option`.

use bytemuck::{Pod, Zeroable};
use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
use std::cell::{Ref, RefMut};

use crate::error::FlockError;

pub const MAX_COMPONENTS: usize = 16;
pub const INDEX_DECIMALS: u8 = 9;
pub const ACCOUNT_TAG_INDEX: u8 = 0xF1;
pub const STATE_VERSION: u8 = 1;

/// Hard caps. Governance sets values inside these; nothing can set them outside.
pub const MAX_STREAMING_FEE_BPS: u16 = 500;
pub const MAX_MINT_REDEEM_FEE_BPS: u16 = 100;
pub const MAX_PREMIUM_CEILING_BPS: u16 = 1_000;
/// The shortest notice a rebalance may give holders between proposal and first bid.
pub const MIN_REBALANCE_DELAY: u32 = 300;
pub const MAX_AUCTION_DURATION: u32 = 7 * 86_400;

pub const SEED_INDEX: &[u8] = b"index";
pub const SEED_VAULT: &[u8] = b"vault";

pub const STATE_BOOTSTRAPPING: u8 = 0;
pub const STATE_LIVE: u8 = 1;
pub const STATE_PAUSED: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Component {
    pub mint: Pubkey,
    /// Base units of this component backing one whole index token (1e9 index base units).
    pub units: u64,
    /// Where the running auction intends `units` to land. Equal to `units` when idle.
    pub target_units: u64,
    /// USD per whole token, 1e9, as of the proposal that set `target_units`. Prices auction bids
    /// and the NAV floor; never used to value a user's issuance or redemption.
    pub ref_price_e9: u64,
    pub decimals: u8,
    pub vault_bump: u8,
    pub _pad: [u8; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Rebalance {
    pub proposer: Pubkey,
    pub proposed_at: i64,
    pub start_ts: i64,
    pub end_ts: i64,
    /// Fund NAV in USD micro-dollars that no bid may take the fund below.
    pub nav_floor_e6: u64,
    /// Discount to the bidder in bps at auction open. Negative means the bidder pays a premium.
    pub start_premium_bps: i16,
    /// Discount to the bidder in bps at auction close.
    pub end_premium_bps: i16,
    pub max_nav_loss_bps: u16,
    pub active: u8,
    pub _pad: [u8; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Index {
    pub index_mint: Pubkey,
    /// Sets fees and authorities. In production this is the cooperative's multisig.
    pub governor: Pubkey,
    /// Proposes rebalances. Cannot touch fees, authorities, or user funds.
    pub manager: Pubkey,
    pub fee_recipient: Pubkey,
    pub last_fee_accrual: i64,
    pub created_at: i64,
    pub rebalance_delay: u32,
    pub streaming_fee_bps: u16,
    pub issue_fee_bps: u16,
    pub redeem_fee_bps: u16,
    /// Ceiling on the edge any auction may ever hand a bidder.
    pub max_premium_bps: u16,
    pub tag: u8,
    pub version: u8,
    pub bump: u8,
    pub state: u8,
    pub decimals: u8,
    pub component_count: u8,
    pub name: [u8; 32],
    pub symbol: [u8; 12],
    pub _pad: [u8; 2],
    pub components: [Component; MAX_COMPONENTS],
    pub rebalance: Rebalance,
}

impl Index {
    pub const LEN: usize = std::mem::size_of::<Index>();

    pub fn components(&self) -> &[Component] {
        &self.components[..self.component_count as usize]
    }

    pub fn component(&self, i: usize) -> Result<Component, ProgramError> {
        if i >= self.component_count as usize {
            return Err(FlockError::UnknownComponent.into());
        }
        Ok(self.components[i])
    }

    pub fn find_component(&self, mint: &Pubkey) -> Option<usize> {
        self.components().iter().position(|c| &c.mint == mint)
    }

    pub fn is_live(&self) -> Result<(), ProgramError> {
        match self.state {
            STATE_LIVE => Ok(()),
            STATE_PAUSED => Err(FlockError::Paused.into()),
            _ => Err(FlockError::NotSealed.into()),
        }
    }
}

/// Read a decoded copy out of an unaligned byte slice. For clients and tests; on-chain code uses
/// the borrow helpers below and never copies the record.
pub fn decode_index(data: &[u8]) -> Result<Index, ProgramError> {
    if data.len() < Index::LEN {
        return Err(ProgramError::AccountDataTooSmall);
    }
    let idx: Index = bytemuck::pod_read_unaligned(&data[..Index::LEN]);
    if idx.tag != ACCOUNT_TAG_INDEX || idx.version != STATE_VERSION {
        return Err(FlockError::InvalidAccountOwner.into());
    }
    Ok(idx)
}

fn check_account(info: &AccountInfo, program_id: &Pubkey) -> Result<(), ProgramError> {
    if info.owner != program_id {
        return Err(FlockError::InvalidAccountOwner.into());
    }
    if info.data_len() < Index::LEN {
        return Err(ProgramError::AccountDataTooSmall);
    }
    Ok(())
}

/// Borrow the index read-only. The borrow lives as long as the returned guard, so callers must
/// drop it before invoking a CPI that passes this account.
pub fn index_ref<'a>(info: &'a AccountInfo, program_id: &Pubkey) -> Result<Ref<'a, Index>, ProgramError> {
    check_account(info, program_id)?;
    let data = info.try_borrow_data()?;
    let guard = Ref::map(data, |d| bytemuck::from_bytes::<Index>(&d[..Index::LEN]));
    if guard.tag != ACCOUNT_TAG_INDEX || guard.version != STATE_VERSION {
        return Err(FlockError::InvalidAccountOwner.into());
    }
    Ok(guard)
}

/// Borrow the index for writing. Same rule: drop before any CPI that touches this account.
pub fn index_mut<'a>(info: &'a AccountInfo, program_id: &Pubkey) -> Result<RefMut<'a, Index>, ProgramError> {
    check_account(info, program_id)?;
    let data = info.try_borrow_mut_data()?;
    let guard = RefMut::map(data, |d| bytemuck::from_bytes_mut::<Index>(&mut d[..Index::LEN]));
    if guard.tag != ACCOUNT_TAG_INDEX || guard.version != STATE_VERSION {
        return Err(FlockError::InvalidAccountOwner.into());
    }
    Ok(guard)
}

/// Read-only borrow that skips the tag check, for the one moment the record does not have one yet.
pub fn index_mut_uninitialized<'a>(info: &'a AccountInfo) -> Result<RefMut<'a, Index>, ProgramError> {
    if info.data_len() < Index::LEN {
        return Err(ProgramError::AccountDataTooSmall);
    }
    let data = info.try_borrow_mut_data()?;
    Ok(RefMut::map(data, |d| bytemuck::from_bytes_mut::<Index>(&mut d[..Index::LEN])))
}

pub fn index_pda(program_id: &Pubkey, index_mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[SEED_INDEX, index_mint.as_ref()], program_id)
}

pub fn vault_pda(program_id: &Pubkey, index_mint: &Pubkey, component_mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[SEED_VAULT, index_mint.as_ref(), component_mint.as_ref()],
        program_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_stable_and_padding_free() {
        assert_eq!(std::mem::size_of::<Component>(), 64);
        assert_eq!(std::mem::size_of::<Rebalance>(), 72);
        assert_eq!(std::mem::align_of::<Index>(), 8);
        // 128 keys + 16 timestamps + 16 scalars + 46 name/symbol/flags + 2 pad + 1024 + 72.
        assert_eq!(Index::LEN, 1304);
    }

    #[test]
    fn decode_rejects_a_foreign_account() {
        let bytes = vec![0u8; Index::LEN];
        assert!(decode_index(&bytes).is_err());
    }

    #[test]
    fn decode_round_trips_through_bytes() {
        let mut idx = Index::zeroed();
        idx.tag = ACCOUNT_TAG_INDEX;
        idx.version = STATE_VERSION;
        idx.component_count = 2;
        idx.components[1].units = 4_321;
        let bytes = bytemuck::bytes_of(&idx).to_vec();
        let back = decode_index(&bytes).unwrap();
        assert_eq!(back.components().len(), 2);
        assert_eq!(back.components()[1].units, 4_321);
    }
}
