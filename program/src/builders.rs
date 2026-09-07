//! Instruction builders.
//!
//! These live in the program crate rather than only in the TypeScript SDK so that the account
//! order can never drift between the handler and its callers: both are compiled from this file.
//! The SDK mirrors it, and `packages/sdk/test/parity.test.ts` checks the two agree byte for byte.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    instruction::FlockInstruction,
    state::{index_pda, vault_pda},
};

fn pack(ix: &FlockInstruction) -> Vec<u8> {
    borsh::to_vec(ix).expect("instruction serializes")
}

fn ro(key: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(key, false)
}
fn rw(key: Pubkey) -> AccountMeta {
    AccountMeta::new(key, false)
}
fn signer_ro(key: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(key, true)
}
fn signer_rw(key: Pubkey) -> AccountMeta {
    AccountMeta::new(key, true)
}

pub struct InitArgs {
    pub name: [u8; 32],
    pub symbol: [u8; 12],
    pub streaming_fee_bps: u16,
    pub issue_fee_bps: u16,
    pub redeem_fee_bps: u16,
    pub max_premium_bps: u16,
    pub rebalance_delay: u32,
}

/// Pad a display string into the fixed field the account stores.
pub fn fixed<const N: usize>(text: &str) -> [u8; N] {
    let mut out = [0u8; N];
    let bytes = text.as_bytes();
    let n = bytes.len().min(N);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

pub fn init_index(
    program_id: &Pubkey,
    payer: &Pubkey,
    index_mint: &Pubkey,
    governor: &Pubkey,
    manager: &Pubkey,
    fee_recipient: &Pubkey,
    args: InitArgs,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            signer_rw(*payer),
            rw(index),
            ro(*index_mint),
            ro(*governor),
            ro(*manager),
            ro(*fee_recipient),
            ro(system_program::id()),
        ],
        data: pack(&FlockInstruction::InitIndex {
            name: args.name,
            symbol: args.symbol,
            streaming_fee_bps: args.streaming_fee_bps,
            issue_fee_bps: args.issue_fee_bps,
            redeem_fee_bps: args.redeem_fee_bps,
            max_premium_bps: args.max_premium_bps,
            rebalance_delay: args.rebalance_delay,
        }),
    }
}

pub fn add_component(
    program_id: &Pubkey,
    manager: &Pubkey,
    payer: &Pubkey,
    index_mint: &Pubkey,
    component_mint: &Pubkey,
    units: u64,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let (vault, _) = vault_pda(program_id, index_mint, component_mint);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            signer_ro(*manager),
            signer_rw(*payer),
            rw(index),
            ro(*index_mint),
            ro(*component_mint),
            rw(vault),
            ro(system_program::id()),
            ro(spl_token::id()),
        ],
        data: pack(&FlockInstruction::AddComponent { units }),
    }
}

pub fn seal_index(program_id: &Pubkey, manager: &Pubkey, index_mint: &Pubkey) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    Instruction {
        program_id: *program_id,
        accounts: vec![signer_ro(*manager), rw(index), ro(*index_mint)],
        data: pack(&FlockInstruction::SealIndex),
    }
}

/// `components` is `(component mint, the caller's token account for it)`, in table order.
pub fn issue(
    program_id: &Pubkey,
    user: &Pubkey,
    index_mint: &Pubkey,
    user_index_ata: &Pubkey,
    fee_ata: &Pubkey,
    components: &[(Pubkey, Pubkey)],
    amount: u64,
    max_in: Vec<u64>,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let mut accounts = vec![
        signer_ro(*user),
        rw(index),
        rw(*index_mint),
        rw(*user_index_ata),
        rw(*fee_ata),
        ro(spl_token::id()),
    ];
    for (mint, user_account) in components {
        accounts.push(rw(vault_pda(program_id, index_mint, mint).0));
        accounts.push(rw(*user_account));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: pack(&FlockInstruction::Issue { amount, max_in }),
    }
}

pub fn redeem(
    program_id: &Pubkey,
    user: &Pubkey,
    index_mint: &Pubkey,
    user_index_ata: &Pubkey,
    fee_ata: &Pubkey,
    components: &[(Pubkey, Pubkey)],
    amount: u64,
    min_out: Vec<u64>,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let mut accounts = vec![
        signer_ro(*user),
        rw(index),
        rw(*index_mint),
        rw(*user_index_ata),
        rw(*fee_ata),
        ro(spl_token::id()),
    ];
    for (mint, user_account) in components {
        accounts.push(rw(vault_pda(program_id, index_mint, mint).0));
        accounts.push(rw(*user_account));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: pack(&FlockInstruction::Redeem { amount, min_out }),
    }
}

pub fn accrue_fees(
    program_id: &Pubkey,
    index_mint: &Pubkey,
    fee_ata: &Pubkey,
    component_mints: &[Pubkey],
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let mut accounts = vec![rw(index), rw(*index_mint), rw(*fee_ata), ro(spl_token::id())];
    for mint in component_mints {
        accounts.push(ro(vault_pda(program_id, index_mint, mint).0));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: pack(&FlockInstruction::AccrueFees),
    }
}

pub struct AuctionArgs {
    pub duration: u32,
    pub start_premium_bps: i16,
    pub end_premium_bps: i16,
    pub max_nav_loss_bps: u16,
}

pub fn propose_rebalance(
    program_id: &Pubkey,
    manager: &Pubkey,
    index_mint: &Pubkey,
    component_mints: &[Pubkey],
    target_units: Vec<u64>,
    ref_prices_e9: Vec<u64>,
    args: AuctionArgs,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let mut accounts = vec![signer_ro(*manager), rw(index), ro(*index_mint)];
    for mint in component_mints {
        accounts.push(ro(vault_pda(program_id, index_mint, mint).0));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: pack(&FlockInstruction::ProposeRebalance {
            target_units,
            ref_prices_e9,
            duration: args.duration,
            start_premium_bps: args.start_premium_bps,
            end_premium_bps: args.end_premium_bps,
            max_nav_loss_bps: args.max_nav_loss_bps,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn bid(
    program_id: &Pubkey,
    bidder: &Pubkey,
    index_mint: &Pubkey,
    component_mints: &[Pubkey],
    bidder_receive: &Pubkey,
    bidder_pay: &Pubkey,
    sell_component: u8,
    buy_component: u8,
    sell_amount: u64,
    max_buy_amount: u64,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let mut accounts = vec![
        signer_ro(*bidder),
        rw(index),
        ro(*index_mint),
        rw(*bidder_receive),
        rw(*bidder_pay),
        ro(spl_token::id()),
    ];
    for mint in component_mints {
        accounts.push(rw(vault_pda(program_id, index_mint, mint).0));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: pack(&FlockInstruction::Bid {
            sell_component,
            buy_component,
            sell_amount,
            max_buy_amount,
        }),
    }
}

pub fn end_rebalance(
    program_id: &Pubkey,
    caller: &Pubkey,
    index_mint: &Pubkey,
    component_mints: &[Pubkey],
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    let mut accounts = vec![signer_ro(*caller), rw(index), ro(*index_mint)];
    for mint in component_mints {
        accounts.push(ro(vault_pda(program_id, index_mint, mint).0));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data: pack(&FlockInstruction::EndRebalance),
    }
}

pub fn set_params(
    program_id: &Pubkey,
    governor: &Pubkey,
    index_mint: &Pubkey,
    streaming_fee_bps: u16,
    issue_fee_bps: u16,
    redeem_fee_bps: u16,
    max_premium_bps: u16,
    rebalance_delay: u32,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    Instruction {
        program_id: *program_id,
        accounts: vec![signer_ro(*governor), rw(index), ro(*index_mint)],
        data: pack(&FlockInstruction::SetParams {
            streaming_fee_bps,
            issue_fee_bps,
            redeem_fee_bps,
            max_premium_bps,
            rebalance_delay,
        }),
    }
}

pub fn set_authority(
    program_id: &Pubkey,
    governor: &Pubkey,
    index_mint: &Pubkey,
    role: u8,
    new_authority: &Pubkey,
) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    Instruction {
        program_id: *program_id,
        accounts: vec![signer_ro(*governor), rw(index), ro(*index_mint)],
        data: pack(&FlockInstruction::SetAuthority {
            role,
            new_authority: *new_authority,
        }),
    }
}

pub fn set_paused(program_id: &Pubkey, caller: &Pubkey, index_mint: &Pubkey, paused: bool) -> Instruction {
    let (index, _) = index_pda(program_id, index_mint);
    Instruction {
        program_id: *program_id,
        accounts: vec![signer_ro(*caller), rw(index), ro(*index_mint)],
        data: pack(&FlockInstruction::SetPaused { paused }),
    }
}
