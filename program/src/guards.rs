//! Account checks shared by every instruction.
//!
//! These are separated out because an unchecked account is the entire attack surface of a Solana
//! program. Every account this program reads or writes passes through one of these functions, and
//! anything that does not is a bug.

use solana_program::{account_info::AccountInfo, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey};
use spl_token::state::{Account as TokenAccount, Mint};

use crate::error::FlockError;

pub fn require_signer(info: &AccountInfo) -> Result<(), ProgramError> {
    if !info.is_signer {
        return Err(FlockError::MissingSignature.into());
    }
    Ok(())
}

pub fn require_key(info: &AccountInfo, expected: &Pubkey) -> Result<(), ProgramError> {
    if info.key != expected {
        return Err(FlockError::SeedsMismatch.into());
    }
    Ok(())
}

pub fn require_token_program(info: &AccountInfo) -> Result<(), ProgramError> {
    if info.key != &spl_token::id() {
        return Err(FlockError::InvalidTokenAccount.into());
    }
    Ok(())
}

pub fn require_system_program(info: &AccountInfo) -> Result<(), ProgramError> {
    if info.key != &solana_program::system_program::id() {
        return Err(FlockError::InvalidTokenAccount.into());
    }
    Ok(())
}

pub fn read_mint(info: &AccountInfo) -> Result<Mint, ProgramError> {
    if info.owner != &spl_token::id() {
        return Err(FlockError::InvalidTokenAccount.into());
    }
    Mint::unpack(&info.try_borrow_data()?).map_err(|_| FlockError::InvalidTokenAccount.into())
}

pub fn read_token_account(info: &AccountInfo) -> Result<TokenAccount, ProgramError> {
    if info.owner != &spl_token::id() {
        return Err(FlockError::InvalidTokenAccount.into());
    }
    TokenAccount::unpack(&info.try_borrow_data()?).map_err(|_| FlockError::InvalidTokenAccount.into())
}

/// A token account that must belong to `owner` and hold `mint`.
pub fn require_token_account(info: &AccountInfo, mint: &Pubkey, owner: &Pubkey) -> Result<TokenAccount, ProgramError> {
    let acct = read_token_account(info)?;
    if &acct.mint != mint || &acct.owner != owner {
        return Err(FlockError::InvalidTokenAccount.into());
    }
    Ok(acct)
}

/// A token account that must hold `mint`, whoever owns it. Used for the accounts a caller
/// nominates for themselves, where the owner is the caller's business and not this program's.
pub fn require_token_account_of_mint(info: &AccountInfo, mint: &Pubkey) -> Result<TokenAccount, ProgramError> {
    let acct = read_token_account(info)?;
    if &acct.mint != mint {
        return Err(FlockError::InvalidTokenAccount.into());
    }
    Ok(acct)
}
