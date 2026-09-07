//! End-to-end behaviour of an index: create it, issue into it, redeem out of it, charge for it,
//! and rebalance it through an auction.

mod common;

use common::{Env, UNIT_SCALE};
use flock_index::{
    builders::{self, AuctionArgs, InitArgs},
    state::{STATE_LIVE, STATE_PAUSED},
};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

/// Two components with different decimals, because a bug that treats them as the same scale hides
/// in a fixture where they are.
const A_DECIMALS: u8 = 6; // a stable-like asset, 2 whole units per index token
const B_DECIMALS: u8 = 9; // a SOL-like asset, 0.5 whole units per index token
const UNITS_A: u64 = 2_000_000;
const UNITS_B: u64 = 500_000_000;
const PRICE_A_E9: u64 = 1_000_000_000; // $1
const PRICE_B_E9: u64 = 100_000_000_000; // $100

struct Fund {
    env: Env,
    index_mint: Pubkey,
    index: Pubkey,
    mints: Vec<Pubkey>,
    mint_authority: Keypair,
    governor: Keypair,
    manager: Keypair,
    fee_ata: Pubkey,
}

impl Fund {
    fn new(streaming_fee_bps: u16, issue_fee_bps: u16, redeem_fee_bps: u16) -> Self {
        let mut env = Env::new();
        let governor = env.fund(10_000_000_000);
        let manager = env.fund(10_000_000_000);
        let fee_recipient = env.fund(10_000_000_000);
        let mint_authority = env.fund(10_000_000_000);

        let (index_mint, index) = env.create_index_mint();
        let a = env.create_mint(&mint_authority.pubkey(), A_DECIMALS);
        let b = env.create_mint(&mint_authority.pubkey(), B_DECIMALS);
        let fee_ata = env.create_token_account(&index_mint, &fee_recipient.pubkey());

        let program_id = env.program_id;
        let payer = env.payer.pubkey();
        env.send(
            &[builders::init_index(
                &program_id,
                &payer,
                &index_mint,
                &governor.pubkey(),
                &manager.pubkey(),
                &fee_recipient.pubkey(),
                InitArgs {
                    name: builders::fixed::<32>("Flock Test Index"),
                    symbol: builders::fixed::<12>("FTI"),
                    streaming_fee_bps,
                    issue_fee_bps,
                    redeem_fee_bps,
                    max_premium_bps: 300,
                    rebalance_delay: 3_600,
                },
            )],
            &[],
        )
        .expect("init index");

        for (mint, units) in [(a, UNITS_A), (b, UNITS_B)] {
            env.send(
                &[builders::add_component(&program_id, &manager.pubkey(), &payer, &index_mint, &mint, units)],
                &[&manager],
            )
            .expect("add component");
        }
        env.send(&[builders::seal_index(&program_id, &manager.pubkey(), &index_mint)], &[&manager])
            .expect("seal");

        Self {
            env,
            index_mint,
            index,
            mints: vec![a, b],
            mint_authority,
            governor,
            manager,
            fee_ata,
        }
    }

    /// A holder with token accounts for every component, funded with `whole` of each.
    fn holder(&mut self, whole_a: u64, whole_b: u64) -> Holder {
        let owner = self.env.fund(10_000_000_000);
        let index_ata = self.env.create_token_account(&self.index_mint, &owner.pubkey());
        let mut component_atas = Vec::new();
        for (i, mint) in self.mints.clone().iter().enumerate() {
            let ata = self.env.create_token_account(mint, &owner.pubkey());
            let decimals = if i == 0 { A_DECIMALS } else { B_DECIMALS };
            let whole = if i == 0 { whole_a } else { whole_b };
            let amount = whole * 10u64.pow(decimals as u32);
            let authority = self.mint_authority.insecure_clone();
            self.env.mint_tokens(mint, &authority, &ata, amount);
            component_atas.push(ata);
        }
        Holder { owner, index_ata, component_atas }
    }

    fn pairs(&self, holder: &Holder) -> Vec<(Pubkey, Pubkey)> {
        self.mints
            .iter()
            .copied()
            .zip(holder.component_atas.iter().copied())
            .collect()
    }

    fn issue(&mut self, holder: &Holder, amount: u64) -> Result<(), String> {
        let ix = builders::issue(
            &self.env.program_id,
            &holder.owner.pubkey(),
            &self.index_mint,
            &holder.index_ata,
            &self.fee_ata,
            &self.pairs(holder),
            amount,
            vec![u64::MAX; self.mints.len()],
        );
        let signer = holder.owner.insecure_clone();
        self.env.send(&[ix], &[&signer])
    }

    fn redeem(&mut self, holder: &Holder, amount: u64) -> Result<(), String> {
        let ix = builders::redeem(
            &self.env.program_id,
            &holder.owner.pubkey(),
            &self.index_mint,
            &holder.index_ata,
            &self.fee_ata,
            &self.pairs(holder),
            amount,
            vec![0; self.mints.len()],
        );
        let signer = holder.owner.insecure_clone();
        self.env.send(&[ix], &[&signer])
    }

    fn vault(&self, i: usize) -> Pubkey {
        flock_index::state::vault_pda(&self.env.program_id, &self.index_mint, &self.mints[i]).0
    }

    /// The invariant the whole program exists to preserve.
    fn assert_backed(&self) {
        let idx = self.env.index(&self.index);
        let supply = self.env.mint_supply(&self.index_mint);
        for (i, component) in idx.components().iter().enumerate() {
            let held = self.env.token_balance(&self.vault(i));
            let owed = (component.units as u128) * (supply as u128) / (UNIT_SCALE as u128);
            assert!(
                held as u128 >= owed,
                "component {i} is unbacked: vault {held} < claims {owed}"
            );
        }
    }
}

struct Holder {
    owner: Keypair,
    index_ata: Pubkey,
    component_atas: Vec<Pubkey>,
}

#[test]
fn issuance_takes_exactly_the_recipe_and_redemption_returns_it() {
    let mut fund = Fund::new(0, 0, 0);
    let holder = fund.holder(1_000, 1_000);

    let ten = 10 * UNIT_SCALE;
    fund.issue(&holder, ten).expect("issue");

    assert_eq!(fund.env.token_balance(&holder.index_ata), ten);
    assert_eq!(fund.env.token_balance(&fund.vault(0)), 20_000_000, "20 whole A");
    assert_eq!(fund.env.token_balance(&fund.vault(1)), 5_000_000_000, "5 whole B");
    fund.assert_backed();

    fund.redeem(&holder, ten).expect("redeem");
    assert_eq!(fund.env.token_balance(&holder.index_ata), 0);
    assert_eq!(fund.env.token_balance(&fund.vault(0)), 0);
    assert_eq!(fund.env.token_balance(&fund.vault(1)), 0);
    assert_eq!(fund.env.token_balance(&holder.component_atas[0]), 1_000 * 10u64.pow(6));
}

#[test]
fn a_second_holder_cannot_dilute_the_first() {
    let mut fund = Fund::new(0, 0, 0);
    let first = fund.holder(1_000, 1_000);
    let second = fund.holder(1_000, 1_000);

    fund.issue(&first, 3 * UNIT_SCALE).expect("first issue");
    let units_before: Vec<u64> = fund.env.index(&fund.index).components().iter().map(|c| c.units).collect();

    fund.issue(&second, 7 * UNIT_SCALE + 12_345).expect("second issue");
    let units_after: Vec<u64> = fund.env.index(&fund.index).components().iter().map(|c| c.units).collect();

    for (before, after) in units_before.iter().zip(units_after.iter()) {
        assert!(after >= before, "backing per token fell from {before} to {after}");
    }
    fund.assert_backed();

    // And the first holder can still take their whole share out.
    fund.redeem(&first, 3 * UNIT_SCALE).expect("first redeems");
    fund.assert_backed();
}

#[test]
fn issue_and_redeem_fees_are_charged_in_index_tokens_and_stay_backed() {
    let mut fund = Fund::new(0, 50, 50); // 50 bps each way
    let holder = fund.holder(1_000, 1_000);

    let ten = 10 * UNIT_SCALE;
    fund.issue(&holder, ten).expect("issue");
    assert_eq!(fund.env.token_balance(&holder.index_ata), ten);
    assert_eq!(fund.env.token_balance(&fund.fee_ata), ten * 50 / 10_000, "issue fee minted");
    // The fee tokens are backed by components the issuer delivered, so the vault holds the gross.
    assert_eq!(fund.env.token_balance(&fund.vault(0)), 20_100_000);
    fund.assert_backed();

    fund.redeem(&holder, ten).expect("redeem");
    assert_eq!(fund.env.token_balance(&fund.fee_ata), (ten * 50 / 10_000) * 2);
    fund.assert_backed();
}

#[test]
fn the_streaming_fee_is_charged_for_elapsed_time_and_nothing_else() {
    let mut fund = Fund::new(100, 0, 0); // 1% per year
    let holder = fund.holder(1_000, 1_000);
    fund.issue(&holder, 100 * UNIT_SCALE).expect("issue");
    assert_eq!(fund.env.token_balance(&fund.fee_ata), 0);

    let program_id = fund.env.program_id;
    let mints = fund.mints.clone();
    let accrue = builders::accrue_fees(&program_id, &fund.index_mint, &fund.fee_ata, &mints);

    // No time has passed: accrual is free.
    fund.env.send(&[accrue.clone()], &[]).expect("accrue at t0");
    assert_eq!(fund.env.token_balance(&fund.fee_ata), 0);

    fund.env.warp(365 * 86_400);
    fund.env.send(&[accrue], &[]).expect("accrue after a year");
    let charged = fund.env.token_balance(&fund.fee_ata);
    let supply = fund.env.mint_supply(&fund.index_mint);
    let share_bps = (charged as u128) * 10_000 / supply as u128;
    // The mint floors, so the recipient lands a hair under its 1% rather than a hair over it.
    // Overcharging by even one base unit would be a bug; undercharging by one is the design.
    assert!((99..=100).contains(&share_bps), "a year of 1% left the recipient {share_bps} bps");
    assert!(charged > 0);

    // Dilution is real: the same index token now claims fewer components than it did.
    let idx = fund.env.index(&fund.index);
    assert!(idx.components()[0].units < UNITS_A);
    fund.assert_backed();
}

#[test]
fn a_pause_stops_issuance_and_never_stops_redemption() {
    let mut fund = Fund::new(0, 0, 0);
    let holder = fund.holder(1_000, 1_000);
    fund.issue(&holder, 10 * UNIT_SCALE).expect("issue while live");

    let program_id = fund.env.program_id;
    let manager = fund.manager.insecure_clone();
    let governor = fund.governor.insecure_clone();
    fund.env
        .send(
            &[builders::set_paused(&program_id, &manager.pubkey(), &fund.index_mint, true)],
            &[&manager],
        )
        .expect("manager pauses");
    assert_eq!(fund.env.index(&fund.index).state, STATE_PAUSED);

    assert!(fund.issue(&holder, UNIT_SCALE).is_err(), "issuance must stop");
    fund.redeem(&holder, 5 * UNIT_SCALE).expect("redemption must not");

    // A manager can stop the fund but cannot restart it.
    assert!(fund
        .env
        .send(
            &[builders::set_paused(&program_id, &manager.pubkey(), &fund.index_mint, false)],
            &[&manager]
        )
        .is_err());
    fund.env
        .send(
            &[builders::set_paused(&program_id, &governor.pubkey(), &fund.index_mint, false)],
            &[&governor],
        )
        .expect("governor resumes");
    assert_eq!(fund.env.index(&fund.index).state, STATE_LIVE);
    fund.issue(&holder, UNIT_SCALE).expect("issue after resume");
}

#[test]
fn authority_boundaries_hold() {
    let mut fund = Fund::new(0, 0, 0);
    let program_id = fund.env.program_id;
    let index_mint = fund.index_mint;
    let manager = fund.manager.insecure_clone();
    let governor = fund.governor.insecure_clone();
    let stranger = fund.env.fund(1_000_000_000);

    // The manager cannot set fees.
    assert!(fund
        .env
        .send(
            &[builders::set_params(&program_id, &manager.pubkey(), &index_mint, 500, 0, 0, 300, 3_600)],
            &[&manager]
        )
        .is_err());
    // Nor can a stranger propose a rebalance.
    let mints = fund.mints.clone();
    assert!(fund
        .env
        .send(
            &[builders::propose_rebalance(
                &program_id,
                &stranger.pubkey(),
                &index_mint,
                &mints,
                vec![UNITS_A, UNITS_B],
                vec![PRICE_A_E9, PRICE_B_E9],
                AuctionArgs { duration: 3_600, start_premium_bps: 0, end_premium_bps: 100, max_nav_loss_bps: 50 },
            )],
            &[&stranger]
        )
        .is_err());
    // Fees above the hard cap are refused even from the governor.
    assert!(fund
        .env
        .send(
            &[builders::set_params(&program_id, &governor.pubkey(), &index_mint, 501, 0, 0, 300, 3_600)],
            &[&governor]
        )
        .is_err());
    fund.env
        .send(
            &[builders::set_params(&program_id, &governor.pubkey(), &index_mint, 200, 10, 10, 300, 3_600)],
            &[&governor],
        )
        .expect("governor sets fees inside the cap");
    assert_eq!(fund.env.index(&fund.index).streaming_fee_bps, 200);
}

#[test]
fn slippage_bounds_are_enforced_on_both_sides() {
    let mut fund = Fund::new(0, 0, 0);
    let holder = fund.holder(1_000, 1_000);
    let program_id = fund.env.program_id;

    let ix = builders::issue(
        &program_id,
        &holder.owner.pubkey(),
        &fund.index_mint,
        &holder.index_ata,
        &fund.fee_ata,
        &fund.pairs(&holder),
        10 * UNIT_SCALE,
        vec![19_999_999, u64::MAX], // one whole unit of A short
    );
    let signer = holder.owner.insecure_clone();
    assert!(fund.env.send(&[ix], &[&signer]).is_err(), "max_in must bind");

    fund.issue(&holder, 10 * UNIT_SCALE).expect("issue");
    let ix = builders::redeem(
        &program_id,
        &holder.owner.pubkey(),
        &fund.index_mint,
        &holder.index_ata,
        &fund.fee_ata,
        &fund.pairs(&holder),
        UNIT_SCALE,
        vec![2_000_001, 0], // one more base unit of A than the recipe pays
    );
    assert!(fund.env.send(&[ix], &[&signer]).is_err(), "min_out must bind");
}

#[test]
fn an_auction_moves_the_fund_to_its_targets_and_only_after_the_timelock() {
    let mut fund = Fund::new(0, 0, 0);
    let holder = fund.holder(10_000, 10_000);
    fund.issue(&holder, 10 * UNIT_SCALE).expect("issue");

    let program_id = fund.env.program_id;
    let index_mint = fund.index_mint;
    let mints = fund.mints.clone();
    let manager = fund.manager.insecure_clone();

    // Shift $250 of value out of B and into A: B halves, A goes from 20 to 270 whole units.
    let target_a = 27_000_000u64;
    let target_b = 250_000_000u64;
    fund.env
        .send(
            &[builders::propose_rebalance(
                &program_id,
                &manager.pubkey(),
                &index_mint,
                &mints,
                vec![target_a, target_b],
                vec![PRICE_A_E9, PRICE_B_E9],
                AuctionArgs {
                    duration: 7_200,
                    start_premium_bps: -50,
                    end_premium_bps: 100,
                    max_nav_loss_bps: 100,
                },
            )],
            &[&manager],
        )
        .expect("propose");

    // A bidder holding plenty of A, wanting B.
    let bidder = fund.holder(100_000, 0);
    let bid = |sell_amount: u64, max_buy: u64| {
        builders::bid(
            &program_id,
            &bidder.owner.pubkey(),
            &index_mint,
            &mints,
            &bidder.component_atas[1], // receives B
            &bidder.component_atas[0], // pays A
            1,
            0,
            sell_amount,
            max_buy,
        )
    };
    let bidder_signer = bidder.owner.insecure_clone();

    // The timelock is real: nothing fills before it elapses.
    assert!(
        fund.env.send(&[bid(2_500_000_000, u64::MAX)], &[&bidder_signer]).is_err(),
        "bids before the auction opens must fail"
    );

    fund.env.warp(3_600);
    // At the open the premium is negative, so the bidder pays above reference value: 2.5 B at
    // $100 is $250 of value, marked up 50 bps to $251.25, which is 251_250_000 base units of A.
    assert!(
        fund.env.send(&[bid(2_500_000_000, 251_249_999)], &[&bidder_signer]).is_err(),
        "max_buy_amount must bind"
    );
    fund.env
        .send(&[bid(2_500_000_000, 251_250_000)], &[&bidder_signer])
        .expect("bid fills at the opening premium");

    assert_eq!(fund.env.token_balance(&fund.vault(1)), 2_500_000_000, "B reached its target");
    assert_eq!(fund.env.token_balance(&fund.vault(0)), 271_250_000, "A took the proceeds");
    assert_eq!(fund.env.token_balance(&bidder.component_atas[1]), 2_500_000_000, "bidder got the B");

    let idx = fund.env.index(&fund.index);
    assert_eq!(idx.components()[1].units, target_b);
    assert!(idx.components()[0].units >= target_a, "A overshot in the fund's favour");
    fund.assert_backed();

    // The leg that reached its target cannot be sold any further.
    assert!(
        fund.env.send(&[bid(1, u64::MAX)], &[&bidder_signer]).is_err(),
        "a component at target has nothing left to sell"
    );

    // The auction is live through its last second, so a permissionless close needs one past it.
    fund.env.warp(7_201);
    fund.env
        .send(&[builders::end_rebalance(&program_id, &bidder.owner.pubkey(), &index_mint, &mints)], &[&bidder_signer])
        .expect("anyone may close an expired auction");
    assert_eq!(fund.env.index(&fund.index).rebalance.active, 0);
}

#[test]
fn a_holder_who_never_bids_still_gets_the_rebalanced_basket() {
    let mut fund = Fund::new(0, 0, 0);
    let holder = fund.holder(10_000, 10_000);
    fund.issue(&holder, 10 * UNIT_SCALE).expect("issue");

    let program_id = fund.env.program_id;
    let index_mint = fund.index_mint;
    let mints = fund.mints.clone();
    let manager = fund.manager.insecure_clone();
    fund.env
        .send(
            &[builders::propose_rebalance(
                &program_id,
                &manager.pubkey(),
                &index_mint,
                &mints,
                vec![27_000_000, 250_000_000],
                vec![PRICE_A_E9, PRICE_B_E9],
                AuctionArgs { duration: 7_200, start_premium_bps: -50, end_premium_bps: 100, max_nav_loss_bps: 100 },
            )],
            &[&manager],
        )
        .expect("propose");
    fund.env.warp(3_600);

    let bidder = fund.holder(100_000, 0);
    let bidder_signer = bidder.owner.insecure_clone();
    fund.env
        .send(
            &[builders::bid(
                &program_id,
                &bidder.owner.pubkey(),
                &index_mint,
                &mints,
                &bidder.component_atas[1],
                &bidder.component_atas[0],
                1,
                0,
                2_500_000_000,
                u64::MAX,
            )],
            &[&bidder_signer],
        )
        .expect("bid");

    let before_a = fund.env.token_balance(&holder.component_atas[0]);
    let before_b = fund.env.token_balance(&holder.component_atas[1]);
    fund.redeem(&holder, 10 * UNIT_SCALE).expect("redeem the new composition");
    let got_a = fund.env.token_balance(&holder.component_atas[0]) - before_a;
    let got_b = fund.env.token_balance(&holder.component_atas[1]) - before_b;
    assert_eq!(got_a, 271_250_000, "holder receives the A the auction bought");
    assert_eq!(got_b, 2_500_000_000, "and the B that is left");
    assert_eq!(fund.env.token_balance(&fund.vault(0)), 0);
    assert_eq!(fund.env.token_balance(&fund.vault(1)), 0);
}
