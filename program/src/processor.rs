//! Instruction handling.
//!
//! The invariant every handler in this file preserves:
//!
//! ```text
//! for every component i:  vault_balance[i] >= units[i] * index_supply / 1e9
//! ```
//!
//! It is never asserted with an if-statement. It holds because `units` is *derived* from the
//! vault balance after every state change, and because deliveries round up while payouts round
//! down. Dust from that rounding stays in the vault, so backing per token is monotonically
//! non-decreasing except where a fee is charged on purpose.
//!
//! One structural rule runs through every handler: a borrow of the index account is opened, used,
//! and dropped before any CPI that passes that account. The runtime hands a program one RefCell
//! per account, so a live borrow across `invoke_signed` is not a style problem, it is a panic.

use borsh::BorshDeserialize;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

use crate::{
    error::FlockError,
    guards::*,
    instruction::{FlockInstruction, ROLE_FEE_RECIPIENT, ROLE_GOVERNOR, ROLE_MANAGER},
    math::*,
    state::*,
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ix = FlockInstruction::try_from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)?;
    match ix {
        FlockInstruction::InitIndex {
            name,
            symbol,
            streaming_fee_bps,
            issue_fee_bps,
            redeem_fee_bps,
            max_premium_bps,
            rebalance_delay,
        } => init_index(
            program_id,
            accounts,
            InitParams {
                name,
                symbol,
                streaming_fee_bps,
                issue_fee_bps,
                redeem_fee_bps,
                max_premium_bps,
                rebalance_delay,
            },
        ),
        FlockInstruction::AddComponent { units } => add_component(program_id, accounts, units),
        FlockInstruction::SealIndex => seal_index(program_id, accounts),
        FlockInstruction::Issue { amount, max_in } => issue(program_id, accounts, amount, &max_in),
        FlockInstruction::Redeem { amount, min_out } => redeem(program_id, accounts, amount, &min_out),
        FlockInstruction::AccrueFees => accrue_fees_ix(program_id, accounts),
        FlockInstruction::ProposeRebalance {
            target_units,
            ref_prices_e9,
            duration,
            start_premium_bps,
            end_premium_bps,
            max_nav_loss_bps,
        } => propose_rebalance(
            program_id,
            accounts,
            &target_units,
            &ref_prices_e9,
            AuctionParams {
                duration,
                start_premium_bps,
                end_premium_bps,
                max_nav_loss_bps,
            },
        ),
        FlockInstruction::Bid {
            sell_component,
            buy_component,
            sell_amount,
            max_buy_amount,
        } => bid(program_id, accounts, sell_component, buy_component, sell_amount, max_buy_amount),
        FlockInstruction::EndRebalance => end_rebalance(program_id, accounts),
        FlockInstruction::SetParams {
            streaming_fee_bps,
            issue_fee_bps,
            redeem_fee_bps,
            max_premium_bps,
            rebalance_delay,
        } => set_params(
            program_id,
            accounts,
            streaming_fee_bps,
            issue_fee_bps,
            redeem_fee_bps,
            max_premium_bps,
            rebalance_delay,
        ),
        FlockInstruction::SetAuthority { role, new_authority } => {
            set_authority(program_id, accounts, role, new_authority)
        }
        FlockInstruction::SetPaused { paused } => set_paused(program_id, accounts, paused),
    }
}

pub struct InitParams {
    pub name: [u8; 32],
    pub symbol: [u8; 12],
    pub streaming_fee_bps: u16,
    pub issue_fee_bps: u16,
    pub redeem_fee_bps: u16,
    pub max_premium_bps: u16,
    pub rebalance_delay: u32,
}

pub struct AuctionParams {
    pub duration: u32,
    pub start_premium_bps: i16,
    pub end_premium_bps: i16,
    pub max_nav_loss_bps: u16,
}

// ---------------------------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------------------------

fn check_index_pda(program_id: &Pubkey, index_info: &AccountInfo, index_mint: &Pubkey) -> Result<u8, ProgramError> {
    let (expected, bump) = index_pda(program_id, index_mint);
    require_key(index_info, &expected)?;
    Ok(bump)
}

fn check_fees(streaming: u16, issue: u16, redeem: u16, premium: u16, delay: u32) -> Result<(), ProgramError> {
    if streaming > MAX_STREAMING_FEE_BPS
        || issue > MAX_MINT_REDEEM_FEE_BPS
        || redeem > MAX_MINT_REDEEM_FEE_BPS
        || premium > MAX_PREMIUM_CEILING_BPS
    {
        return Err(FlockError::FeeTooHigh.into());
    }
    if delay < MIN_REBALANCE_DELAY {
        return Err(FlockError::TimelockNotMet.into());
    }
    Ok(())
}

/// Check every vault account against its PDA, its mint and its owner, and return the balances in
/// table order. Nothing here invokes, so holding the index borrow for the walk is safe.
fn verify_vaults(
    program_id: &Pubkey,
    index_info: &AccountInfo,
    index_mint: &Pubkey,
    vaults: &[&AccountInfo],
) -> Result<[u64; MAX_COMPONENTS], ProgramError> {
    let idx = index_ref(index_info, program_id)?;
    let n = idx.component_count as usize;
    if vaults.len() != n {
        return Err(FlockError::ArityMismatch.into());
    }
    let mut balances = [0u64; MAX_COMPONENTS];
    for (i, vault) in vaults.iter().enumerate() {
        let mint = idx.components[i].mint;
        let (expected, _) = vault_pda(program_id, index_mint, &mint);
        require_key(vault, &expected)?;
        let acct = require_token_account(vault, &mint, index_info.key)?;
        balances[i] = acct.amount;
    }
    Ok(balances)
}

/// Refresh the cached units from what the vaults actually hold. The only place unit values are
/// ever written outside bootstrapping.
fn sync_units(
    program_id: &Pubkey,
    index_info: &AccountInfo,
    balances: &[u64; MAX_COMPONENTS],
    supply: u64,
) -> Result<(), ProgramError> {
    if supply == 0 {
        return Ok(());
    }
    let mut idx = index_mut(index_info, program_id)?;
    for i in 0..idx.component_count as usize {
        idx.components[i].units = units_from_balance(balances[i], supply)?;
    }
    Ok(())
}

fn nav_e6(
    program_id: &Pubkey,
    index_info: &AccountInfo,
    balances: &[u64; MAX_COMPONENTS],
) -> Result<u64, ProgramError> {
    let idx = index_ref(index_info, program_id)?;
    let mut total = 0u128;
    for i in 0..idx.component_count as usize {
        let c = idx.components[i];
        total = total
            .checked_add(value_e9(balances[i], c.decimals, c.ref_price_e9)?)
            .ok_or(FlockError::MathOverflow)?;
    }
    Ok(to_u64(total / 1_000)?)
}

fn component_at(program_id: &Pubkey, index_info: &AccountInfo, i: usize) -> Result<Component, ProgramError> {
    let idx = index_ref(index_info, program_id)?;
    idx.component(i)
}

#[allow(deprecated)]
fn transfer_from_vault<'a>(
    token_program: &AccountInfo<'a>,
    vault: &AccountInfo<'a>,
    to: &AccountInfo<'a>,
    index_authority: &AccountInfo<'a>,
    amount: u64,
    seeds: &[&[u8]],
) -> ProgramResult {
    if amount == 0 {
        return Ok(());
    }
    let ix = spl_token::instruction::transfer(token_program.key, vault.key, to.key, index_authority.key, &[], amount)?;
    invoke_signed(
        &ix,
        &[vault.clone(), to.clone(), index_authority.clone(), token_program.clone()],
        &[seeds],
    )
}

#[allow(deprecated)]
fn transfer_from_user<'a>(
    token_program: &AccountInfo<'a>,
    from: &AccountInfo<'a>,
    vault: &AccountInfo<'a>,
    user: &AccountInfo<'a>,
    amount: u64,
) -> ProgramResult {
    if amount == 0 {
        return Ok(());
    }
    let ix = spl_token::instruction::transfer(token_program.key, from.key, vault.key, user.key, &[], amount)?;
    invoke(&ix, &[from.clone(), vault.clone(), user.clone(), token_program.clone()])
}

fn mint_index_tokens<'a>(
    token_program: &AccountInfo<'a>,
    mint: &AccountInfo<'a>,
    to: &AccountInfo<'a>,
    index_authority: &AccountInfo<'a>,
    amount: u64,
    seeds: &[&[u8]],
) -> ProgramResult {
    if amount == 0 {
        return Ok(());
    }
    let ix = spl_token::instruction::mint_to(token_program.key, mint.key, to.key, index_authority.key, &[], amount)?;
    invoke_signed(
        &ix,
        &[mint.clone(), to.clone(), index_authority.clone(), token_program.clone()],
        &[seeds],
    )
}

fn burn_index_tokens<'a>(
    token_program: &AccountInfo<'a>,
    mint: &AccountInfo<'a>,
    from: &AccountInfo<'a>,
    user: &AccountInfo<'a>,
    amount: u64,
) -> ProgramResult {
    let ix = spl_token::instruction::burn(token_program.key, from.key, mint.key, user.key, &[], amount)?;
    invoke(&ix, &[from.clone(), mint.clone(), user.clone(), token_program.clone()])
}

/// Mint the streaming fee owed since the last accrual, and return what was minted.
///
/// Called at the top of issue and redeem as well as on its own, so a holder can never enter or
/// exit at a composition that still owes the manager time.
fn accrue<'a>(
    program_id: &Pubkey,
    index_info: &AccountInfo<'a>,
    index_mint_info: &AccountInfo<'a>,
    fee_ata: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    now: i64,
) -> Result<u64, ProgramError> {
    let (bump, fee_bps, last, fee_recipient) = {
        let idx = index_ref(index_info, program_id)?;
        (idx.bump, idx.streaming_fee_bps, idx.last_fee_accrual, idx.fee_recipient)
    };
    let supply = read_mint(index_mint_info)?.supply;
    let owed = streaming_fee_tokens(supply, fee_bps, now.saturating_sub(last))?;
    {
        let mut idx = index_mut(index_info, program_id)?;
        idx.last_fee_accrual = now;
    }
    if owed == 0 {
        return Ok(0);
    }
    require_token_account(fee_ata, index_mint_info.key, &fee_recipient)?;
    let bump_seed = [bump];
    let seeds: [&[u8]; 3] = [SEED_INDEX, index_mint_info.key.as_ref(), &bump_seed];
    mint_index_tokens(token_program, index_mint_info, fee_ata, index_info, owed, &seeds)?;
    Ok(owed)
}

// ---------------------------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------------------------

fn init_index(program_id: &Pubkey, accounts: &[AccountInfo], p: InitParams) -> ProgramResult {
    let iter = &mut accounts.iter();
    let payer = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let governor = next_account_info(iter)?;
    let manager = next_account_info(iter)?;
    let fee_recipient = next_account_info(iter)?;
    let system_program = next_account_info(iter)?;

    require_signer(payer)?;
    require_system_program(system_program)?;
    check_fees(
        p.streaming_fee_bps,
        p.issue_fee_bps,
        p.redeem_fee_bps,
        p.max_premium_bps,
        p.rebalance_delay,
    )?;

    let (index_key, bump) = index_pda(program_id, index_mint_info.key);
    require_key(index_info, &index_key)?;
    if !index_info.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    // The mint must already be under this PDA's control, with nothing minted and no freeze
    // authority. A freeze authority held by anyone would let that party stop redemptions, which is
    // the one exposure a holder of an index must never have.
    let mint = read_mint(index_mint_info)?;
    if mint.decimals != INDEX_DECIMALS
        || mint.supply != 0
        || mint.freeze_authority.is_some()
        || mint.mint_authority != solana_program::program_option::COption::Some(index_key)
    {
        return Err(FlockError::InvalidIndexMint.into());
    }

    let rent = Rent::get()?;
    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            &index_key,
            rent.minimum_balance(Index::LEN),
            Index::LEN as u64,
            program_id,
        ),
        &[payer.clone(), index_info.clone(), system_program.clone()],
        &[&[SEED_INDEX, index_mint_info.key.as_ref(), &[bump]]],
    )?;

    let now = Clock::get()?.unix_timestamp;
    let mut idx = index_mut_uninitialized(index_info)?;
    idx.tag = ACCOUNT_TAG_INDEX;
    idx.version = STATE_VERSION;
    idx.bump = bump;
    idx.state = STATE_BOOTSTRAPPING;
    idx.decimals = INDEX_DECIMALS;
    idx.index_mint = *index_mint_info.key;
    idx.governor = *governor.key;
    idx.manager = *manager.key;
    idx.fee_recipient = *fee_recipient.key;
    idx.streaming_fee_bps = p.streaming_fee_bps;
    idx.issue_fee_bps = p.issue_fee_bps;
    idx.redeem_fee_bps = p.redeem_fee_bps;
    idx.max_premium_bps = p.max_premium_bps;
    idx.rebalance_delay = p.rebalance_delay;
    idx.last_fee_accrual = now;
    idx.created_at = now;
    idx.name = p.name;
    idx.symbol = p.symbol;
    msg!("flock: index initialized");
    Ok(())
}

fn add_component(program_id: &Pubkey, accounts: &[AccountInfo], units: u64) -> ProgramResult {
    let iter = &mut accounts.iter();
    let manager = next_account_info(iter)?;
    let payer = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let component_mint_info = next_account_info(iter)?;
    let vault_info = next_account_info(iter)?;
    let system_program = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;

    require_signer(manager)?;
    require_signer(payer)?;
    require_system_program(system_program)?;
    require_token_program(token_program)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    if units == 0 {
        return Err(FlockError::ZeroAmount.into());
    }

    {
        let idx = index_ref(index_info, program_id)?;
        if &idx.manager != manager.key {
            return Err(FlockError::NotManager.into());
        }
        if idx.state != STATE_BOOTSTRAPPING {
            return Err(FlockError::AlreadySealed.into());
        }
        if idx.component_count as usize >= MAX_COMPONENTS {
            return Err(FlockError::TooManyComponents.into());
        }
        if idx.find_component(component_mint_info.key).is_some() {
            return Err(FlockError::DuplicateComponent.into());
        }
    }

    let component_decimals = read_mint(component_mint_info)?.decimals;
    let (vault_key, vault_bump) = vault_pda(program_id, index_mint_info.key, component_mint_info.key);
    require_key(vault_info, &vault_key)?;

    let rent = Rent::get()?;
    let space = spl_token::state::Account::LEN;
    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            &vault_key,
            rent.minimum_balance(space),
            space as u64,
            &spl_token::id(),
        ),
        &[payer.clone(), vault_info.clone(), system_program.clone()],
        &[&[
            SEED_VAULT,
            index_mint_info.key.as_ref(),
            component_mint_info.key.as_ref(),
            &[vault_bump],
        ]],
    )?;
    invoke(
        &spl_token::instruction::initialize_account3(
            token_program.key,
            &vault_key,
            component_mint_info.key,
            index_info.key,
        )?,
        &[vault_info.clone(), component_mint_info.clone(), token_program.clone()],
    )?;

    let mut idx = index_mut(index_info, program_id)?;
    let slot = idx.component_count as usize;
    idx.components[slot] = Component {
        mint: *component_mint_info.key,
        units,
        target_units: units,
        ref_price_e9: 0,
        decimals: component_decimals,
        vault_bump,
        _pad: [0u8; 6],
    };
    idx.component_count += 1;
    msg!("flock: component added");
    Ok(())
}

fn seal_index(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let manager = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;

    require_signer(manager)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    let now = Clock::get()?.unix_timestamp;
    let mut idx = index_mut(index_info, program_id)?;
    if &idx.manager != manager.key {
        return Err(FlockError::NotManager.into());
    }
    if idx.state != STATE_BOOTSTRAPPING {
        return Err(FlockError::AlreadySealed.into());
    }
    if idx.component_count == 0 {
        return Err(FlockError::UnknownComponent.into());
    }
    idx.state = STATE_LIVE;
    idx.last_fee_accrual = now;
    msg!("flock: index sealed");
    Ok(())
}

fn issue(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64, max_in: &[u64]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let user = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let user_index_ata = next_account_info(iter)?;
    let fee_ata = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let rest: Vec<&AccountInfo> = iter.collect();

    require_signer(user)?;
    require_token_program(token_program)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    if amount == 0 {
        return Err(FlockError::ZeroAmount.into());
    }

    let (bump, n, issue_fee_bps) = {
        let idx = index_ref(index_info, program_id)?;
        idx.is_live()?;
        (idx.bump, idx.component_count as usize, idx.issue_fee_bps)
    };
    if max_in.len() != n || rest.len() != n * 2 {
        return Err(FlockError::ArityMismatch.into());
    }
    require_token_account_of_mint(user_index_ata, index_mint_info.key)?;

    let now = Clock::get()?.unix_timestamp;
    accrue(program_id, index_info, index_mint_info, fee_ata, token_program, now)?;

    let vaults: Vec<&AccountInfo> = (0..n).map(|i| rest[i * 2]).collect();
    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    sync_units(program_id, index_info, &balances, supply)?;

    // The fee is minted alongside the user's tokens and is backed by components the user delivers,
    // so charging it never dilutes anybody already holding.
    let fee_tokens = to_u64(mul_div_ceil(amount as u128, issue_fee_bps as u128, BPS)?)?;
    let gross = amount.checked_add(fee_tokens).ok_or(FlockError::MathOverflow)?;

    for i in 0..n {
        let component = component_at(program_id, index_info, i)?;
        let required = units_in(component.units, gross)?;
        if required > max_in[i] {
            return Err(FlockError::SlippageExceeded.into());
        }
        require_token_account_of_mint(rest[i * 2 + 1], &component.mint)?;
        transfer_from_user(token_program, rest[i * 2 + 1], vaults[i], user, required)?;
    }

    let bump_seed = [bump];
    let seeds: [&[u8]; 3] = [SEED_INDEX, index_mint_info.key.as_ref(), &bump_seed];
    mint_index_tokens(token_program, index_mint_info, user_index_ata, index_info, amount, &seeds)?;
    if fee_tokens > 0 {
        let fee_recipient = index_ref(index_info, program_id)?.fee_recipient;
        require_token_account(fee_ata, index_mint_info.key, &fee_recipient)?;
        mint_index_tokens(token_program, index_mint_info, fee_ata, index_info, fee_tokens, &seeds)?;
    }

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    sync_units(program_id, index_info, &balances, supply)?;
    msg!("flock: issued {}", amount);
    Ok(())
}

fn redeem(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64, min_out: &[u64]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let user = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let user_index_ata = next_account_info(iter)?;
    let fee_ata = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let rest: Vec<&AccountInfo> = iter.collect();

    require_signer(user)?;
    require_token_program(token_program)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    if amount == 0 {
        return Err(FlockError::ZeroAmount.into());
    }

    // Redemption is deliberately available while paused. A pause protects the fund from a broken
    // component or a bad proposal; it must never trap a holder inside one.
    let (bump, n, redeem_fee_bps) = {
        let idx = index_ref(index_info, program_id)?;
        if idx.state == STATE_BOOTSTRAPPING {
            return Err(FlockError::NotSealed.into());
        }
        (idx.bump, idx.component_count as usize, idx.redeem_fee_bps)
    };
    if min_out.len() != n || rest.len() != n * 2 {
        return Err(FlockError::ArityMismatch.into());
    }
    require_token_account_of_mint(user_index_ata, index_mint_info.key)?;

    let now = Clock::get()?.unix_timestamp;
    accrue(program_id, index_info, index_mint_info, fee_ata, token_program, now)?;

    let vaults: Vec<&AccountInfo> = (0..n).map(|i| rest[i * 2]).collect();
    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    if supply < amount {
        return Err(FlockError::SupplyTooSmall.into());
    }
    sync_units(program_id, index_info, &balances, supply)?;

    let fee_tokens = to_u64(mul_div_ceil(amount as u128, redeem_fee_bps as u128, BPS)?)?;
    let net = amount.checked_sub(fee_tokens).ok_or(FlockError::MathOverflow)?;

    burn_index_tokens(token_program, index_mint_info, user_index_ata, user, amount)?;
    let bump_seed = [bump];
    let seeds: [&[u8]; 3] = [SEED_INDEX, index_mint_info.key.as_ref(), &bump_seed];
    if fee_tokens > 0 {
        let fee_recipient = index_ref(index_info, program_id)?.fee_recipient;
        require_token_account(fee_ata, index_mint_info.key, &fee_recipient)?;
        mint_index_tokens(token_program, index_mint_info, fee_ata, index_info, fee_tokens, &seeds)?;
    }

    for i in 0..n {
        let component = component_at(program_id, index_info, i)?;
        let payout = units_out(component.units, net)?;
        if payout < min_out[i] {
            return Err(FlockError::SlippageExceeded.into());
        }
        require_token_account_of_mint(rest[i * 2 + 1], &component.mint)?;
        transfer_from_vault(token_program, vaults[i], rest[i * 2 + 1], index_info, payout, &seeds)?;
    }

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    sync_units(program_id, index_info, &balances, supply)?;
    msg!("flock: redeemed {}", amount);
    Ok(())
}

fn accrue_fees_ix(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let fee_ata = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let vaults: Vec<&AccountInfo> = iter.collect();

    require_token_program(token_program)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    {
        let idx = index_ref(index_info, program_id)?;
        if idx.state == STATE_BOOTSTRAPPING {
            return Err(FlockError::NotSealed.into());
        }
    }
    let now = Clock::get()?.unix_timestamp;
    let minted = accrue(program_id, index_info, index_mint_info, fee_ata, token_program, now)?;

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    sync_units(program_id, index_info, &balances, supply)?;
    msg!("flock: accrued {}", minted);
    Ok(())
}

fn propose_rebalance(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    target_units: &[u64],
    ref_prices_e9: &[u64],
    p: AuctionParams,
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let manager = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let vaults: Vec<&AccountInfo> = iter.collect();

    require_signer(manager)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;

    let (n, max_premium_bps, delay) = {
        let idx = index_ref(index_info, program_id)?;
        if &idx.manager != manager.key {
            return Err(FlockError::NotManager.into());
        }
        idx.is_live()?;
        if idx.rebalance.active != 0 {
            return Err(FlockError::RebalanceActive.into());
        }
        (idx.component_count as usize, idx.max_premium_bps, idx.rebalance_delay)
    };
    if target_units.len() != n || ref_prices_e9.len() != n {
        return Err(FlockError::ArityMismatch.into());
    }
    if p.duration == 0 || p.duration > MAX_AUCTION_DURATION {
        return Err(FlockError::InvalidPremium.into());
    }
    // The auction must open in the fund's favor and decay toward the bidder, never the reverse,
    // and it can never hand away more than the index's governed ceiling.
    if p.start_premium_bps > p.end_premium_bps
        || p.end_premium_bps > max_premium_bps as i16
        || p.start_premium_bps < -(max_premium_bps as i16)
        || p.max_nav_loss_bps > max_premium_bps
    {
        return Err(FlockError::InvalidPremium.into());
    }
    if ref_prices_e9.iter().any(|price| *price == 0) {
        return Err(FlockError::InvalidPrice.into());
    }

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    {
        let mut idx = index_mut(index_info, program_id)?;
        for i in 0..n {
            idx.components[i].ref_price_e9 = ref_prices_e9[i];
            idx.components[i].target_units = target_units[i];
        }
    }
    let nav = nav_e6(program_id, index_info, &balances)?;
    let now = Clock::get()?.unix_timestamp;
    let start_ts = now.checked_add(delay as i64).ok_or(FlockError::MathOverflow)?;
    let end_ts = start_ts.checked_add(p.duration as i64).ok_or(FlockError::MathOverflow)?;

    let mut idx = index_mut(index_info, program_id)?;
    idx.rebalance = Rebalance {
        active: 1,
        proposed_at: now,
        start_ts,
        end_ts,
        start_premium_bps: p.start_premium_bps,
        end_premium_bps: p.end_premium_bps,
        max_nav_loss_bps: p.max_nav_loss_bps,
        nav_floor_e6: to_u64(mul_div_floor(
            nav as u128,
            BPS - p.max_nav_loss_bps as u128,
            BPS,
        )?)?,
        proposer: *manager.key,
        _pad: [0u8; 1],
    };
    msg!("flock: rebalance proposed, opens at {}", start_ts);
    Ok(())
}

fn bid(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    sell_component: u8,
    buy_component: u8,
    sell_amount: u64,
    max_buy_amount: u64,
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let bidder = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let bidder_receive = next_account_info(iter)?;
    let bidder_pay = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let vaults: Vec<&AccountInfo> = iter.collect();

    require_signer(bidder)?;
    require_token_program(token_program)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    if sell_component == buy_component {
        return Err(FlockError::SameComponent.into());
    }
    if sell_amount == 0 {
        return Err(FlockError::ZeroAmount.into());
    }

    let (s, b) = (sell_component as usize, buy_component as usize);
    let now = Clock::get()?.unix_timestamp;
    let (bump, premium, max_premium_bps, sell, buy) = {
        let idx = index_ref(index_info, program_id)?;
        idx.is_live()?;
        if idx.rebalance.active == 0 {
            return Err(FlockError::NoRebalance.into());
        }
        let premium = premium_at(
            idx.rebalance.start_premium_bps,
            idx.rebalance.end_premium_bps,
            idx.rebalance.start_ts,
            idx.rebalance.end_ts,
            now,
        )?;
        if premium > idx.max_premium_bps as i32 {
            return Err(FlockError::InvalidPremium.into());
        }
        (idx.bump, premium, idx.max_premium_bps, idx.component(s)?, idx.component(b)?)
    };

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    if supply == 0 {
        return Err(FlockError::SupplyTooSmall.into());
    }
    require_token_account_of_mint(bidder_receive, &sell.mint)?;
    require_token_account_of_mint(bidder_pay, &buy.mint)?;

    // The fund may only sell what it holds above target. That single cap is what makes the auction
    // convergent and what stops a bidder draining a leg past the published plan.
    //
    // The buy side is capped a little above target rather than exactly at it. Early in an auction
    // the premium is negative, meaning the bidder pays the fund *more* than the reference value,
    // so the leg being bought lands above its target by roughly the premium. Capping the buy side
    // at exactly target would reject precisely the bids that make the fund money, which is the
    // opposite of what the cap is for. The slack is the index's own premium ceiling, so it can
    // never exceed what governance already allows a single auction to move.
    let sell_target = to_u64(mul_div_floor(sell.target_units as u128, supply as u128, UNIT_SCALE)?)?;
    let buy_target = to_u64(mul_div_floor(buy.target_units as u128, supply as u128, UNIT_SCALE)?)?;
    let buy_cap = buy_target
        .checked_add(to_u64(mul_div_floor(
            buy_target as u128,
            max_premium_bps as u128,
            BPS,
        )?)?)
        .ok_or(FlockError::MathOverflow)?;
    if balances[s] <= sell_target {
        return Err(FlockError::SellLegAtTarget.into());
    }
    if sell_amount > balances[s] - sell_target {
        return Err(FlockError::SellAmountExceedsExcess.into());
    }
    if balances[b] >= buy_cap {
        return Err(FlockError::BuyLegAtCap.into());
    }

    // What the bidder takes out, valued at the proposal's reference prices, then marked by the
    // auction: a negative premium makes the bidder pay above fair value, a positive one below.
    let value_out = value_e9(sell_amount, sell.decimals, sell.ref_price_e9)?;
    let marked = u128::try_from(BPS as i128 - premium as i128).map_err(|_| FlockError::MathOverflow)?;
    let value_in = mul_div_ceil(value_out, marked, BPS)?;
    let buy_amount = amount_from_value_ceil(value_in, buy.decimals, buy.ref_price_e9)?;
    if buy_amount > max_buy_amount {
        return Err(FlockError::SlippageExceeded.into());
    }
    if buy_amount > buy_cap - balances[b] {
        return Err(FlockError::BuyAmountExceedsCap.into());
    }

    transfer_from_user(token_program, bidder_pay, vaults[b], bidder, buy_amount)?;
    let bump_seed = [bump];
    let seeds: [&[u8]; 3] = [SEED_INDEX, index_mint_info.key.as_ref(), &bump_seed];
    transfer_from_vault(token_program, vaults[s], bidder_receive, index_info, sell_amount, &seeds)?;

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let nav = nav_e6(program_id, index_info, &balances)?;
    {
        let idx = index_ref(index_info, program_id)?;
        if nav < idx.rebalance.nav_floor_e6 {
            return Err(FlockError::NavFloorBreached.into());
        }
    }
    sync_units(program_id, index_info, &balances, supply)?;
    msg!("flock: bid filled at {} bps, paid {}", premium, buy_amount);
    Ok(())
}

fn end_rebalance(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let caller = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;
    let vaults: Vec<&AccountInfo> = iter.collect();

    require_signer(caller)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    let now = Clock::get()?.unix_timestamp;
    {
        let idx = index_ref(index_info, program_id)?;
        if idx.rebalance.active == 0 {
            return Err(FlockError::NoRebalance.into());
        }
        // Anyone may close an expired auction; only the manager may close a running one.
        if now <= idx.rebalance.end_ts && &idx.manager != caller.key {
            return Err(FlockError::NotManager.into());
        }
    }

    let balances = verify_vaults(program_id, index_info, index_mint_info.key, &vaults)?;
    let supply = read_mint(index_mint_info)?.supply;
    sync_units(program_id, index_info, &balances, supply)?;

    let mut idx = index_mut(index_info, program_id)?;
    for i in 0..idx.component_count as usize {
        idx.components[i].target_units = idx.components[i].units;
    }
    let proposer = idx.rebalance.proposer;
    let proposed_at = idx.rebalance.proposed_at;
    idx.rebalance = Rebalance {
        proposer,
        proposed_at,
        ..Rebalance::default()
    };
    msg!("flock: rebalance closed");
    Ok(())
}

fn set_params(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    streaming_fee_bps: u16,
    issue_fee_bps: u16,
    redeem_fee_bps: u16,
    max_premium_bps: u16,
    rebalance_delay: u32,
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let governor = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;

    require_signer(governor)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    check_fees(streaming_fee_bps, issue_fee_bps, redeem_fee_bps, max_premium_bps, rebalance_delay)?;
    let now = Clock::get()?.unix_timestamp;

    let mut idx = index_mut(index_info, program_id)?;
    if &idx.governor != governor.key {
        return Err(FlockError::NotGovernor.into());
    }
    // Settling the clock here means the old rate applies to the time it was actually in force. A
    // fee raise that skipped this step would bill the entire period since the last accrual at the
    // new rate, which is a retroactive charge on holders who never agreed to it.
    if idx.state != STATE_BOOTSTRAPPING {
        idx.last_fee_accrual = now;
    }
    idx.streaming_fee_bps = streaming_fee_bps;
    idx.issue_fee_bps = issue_fee_bps;
    idx.redeem_fee_bps = redeem_fee_bps;
    idx.max_premium_bps = max_premium_bps;
    idx.rebalance_delay = rebalance_delay;
    msg!("flock: params updated");
    Ok(())
}

fn set_authority(program_id: &Pubkey, accounts: &[AccountInfo], role: u8, new_authority: Pubkey) -> ProgramResult {
    let iter = &mut accounts.iter();
    let governor = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;

    require_signer(governor)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    let mut idx = index_mut(index_info, program_id)?;
    if &idx.governor != governor.key {
        return Err(FlockError::NotGovernor.into());
    }
    match role {
        ROLE_GOVERNOR => idx.governor = new_authority,
        ROLE_MANAGER => idx.manager = new_authority,
        ROLE_FEE_RECIPIENT => idx.fee_recipient = new_authority,
        _ => return Err(ProgramError::InvalidInstructionData),
    }
    msg!("flock: authority {} updated", role);
    Ok(())
}

fn set_paused(program_id: &Pubkey, accounts: &[AccountInfo], paused: bool) -> ProgramResult {
    let iter = &mut accounts.iter();
    let caller = next_account_info(iter)?;
    let index_info = next_account_info(iter)?;
    let index_mint_info = next_account_info(iter)?;

    require_signer(caller)?;
    check_index_pda(program_id, index_info, index_mint_info.key)?;
    let mut idx = index_mut(index_info, program_id)?;
    // Either authority can hit the brake; only the governor can release it. Stopping is urgent,
    // restarting never is.
    if paused {
        if &idx.governor != caller.key && &idx.manager != caller.key {
            return Err(FlockError::NotManager.into());
        }
    } else if &idx.governor != caller.key {
        return Err(FlockError::NotGovernor.into());
    }
    if idx.state == STATE_BOOTSTRAPPING {
        return Err(FlockError::NotSealed.into());
    }
    idx.state = if paused { STATE_PAUSED } else { STATE_LIVE };
    msg!("flock: paused {}", paused);
    Ok(())
}
