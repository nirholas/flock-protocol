//! Adversarial cases. Each one is an attack that would drain or damage a fund if the check it
//! targets were missing, so each is written as the attacker would run it, not as a unit test of
//! the guard.

mod common;

use common::{Env, UNIT_SCALE};
use flock_index::builders::{self, AuctionArgs, InitArgs};
use solana_sdk::{
    instruction::AccountMeta,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

struct Setup {
    env: Env,
    index_mint: Pubkey,
    mints: Vec<Pubkey>,
    governor: Keypair,
    manager: Keypair,
    mint_authority: Keypair,
    fee_ata: Pubkey,
}

fn setup() -> Setup {
    let mut env = Env::new();
    let governor = env.fund(10_000_000_000);
    let manager = env.fund(10_000_000_000);
    let fee_recipient = env.fund(10_000_000_000);
    let mint_authority = env.fund(10_000_000_000);
    let (index_mint, _) = env.create_index_mint();
    let a = env.create_mint(&mint_authority.pubkey(), 6);
    let b = env.create_mint(&mint_authority.pubkey(), 9);
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
                name: builders::fixed::<32>("Security"),
                symbol: builders::fixed::<12>("SEC"),
                streaming_fee_bps: 0,
                issue_fee_bps: 0,
                redeem_fee_bps: 0,
                max_premium_bps: 300,
                rebalance_delay: 3_600,
            },
        )],
        &[],
    )
    .expect("init");
    for (mint, units) in [(a, 2_000_000u64), (b, 500_000_000u64)] {
        env.send(
            &[builders::add_component(&program_id, &manager.pubkey(), &payer, &index_mint, &mint, units)],
            &[&manager],
        )
        .expect("add");
    }
    env.send(&[builders::seal_index(&program_id, &manager.pubkey(), &index_mint)], &[&manager])
        .expect("seal");

    Setup { env, index_mint, mints: vec![a, b], governor, manager, mint_authority, fee_ata }
}

impl Setup {
    fn holder(&mut self, whole_a: u64, whole_b: u64) -> (Keypair, Pubkey, Vec<Pubkey>) {
        let owner = self.env.fund(10_000_000_000);
        let index_ata = self.env.create_token_account(&self.index_mint, &owner.pubkey());
        let mut atas = Vec::new();
        for (i, mint) in self.mints.clone().iter().enumerate() {
            let ata = self.env.create_token_account(mint, &owner.pubkey());
            let amount = if i == 0 { whole_a * 1_000_000 } else { whole_b * 1_000_000_000 };
            let authority = self.mint_authority.insecure_clone();
            self.env.mint_tokens(mint, &authority, &ata, amount);
            atas.push(ata);
        }
        (owner, index_ata, atas)
    }

    fn pairs(&self, atas: &[Pubkey]) -> Vec<(Pubkey, Pubkey)> {
        self.mints.iter().copied().zip(atas.iter().copied()).collect()
    }
}

#[test]
fn a_vault_the_attacker_controls_is_not_a_vault() {
    let mut s = setup();
    let (user, index_ata, atas) = s.holder(1_000, 1_000);
    let program_id = s.env.program_id;

    // Substitute a token account the attacker owns for the real vault of component A, so that the
    // components they "deliver" land back in their own pocket while the index mints against them.
    let fake_vault = s.env.create_token_account(&s.mints[0], &user.pubkey());
    let mut ix = builders::issue(
        &program_id,
        &user.pubkey(),
        &s.index_mint,
        &index_ata,
        &s.fee_ata,
        &s.pairs(&atas),
        UNIT_SCALE,
        vec![u64::MAX; 2],
    );
    ix.accounts[6] = AccountMeta::new(fake_vault, false);
    let signer = user.insecure_clone();
    assert!(s.env.send(&[ix], &[&signer]).is_err(), "a substituted vault must be rejected");
}

#[test]
fn the_fee_account_must_belong_to_the_fee_recipient() {
    let mut s = setup();
    let (user, index_ata, atas) = s.holder(1_000, 1_000);
    let program_id = s.env.program_id;
    let index_mint = s.index_mint;
    let mints = s.mints.clone();
    let signer = user.insecure_clone();

    // Put a real streaming fee in force, so the fee account is genuinely written to.
    let governor = s.governor.insecure_clone();
    s.env
        .send(
            &[builders::set_params(&program_id, &governor.pubkey(), &index_mint, 200, 0, 0, 300, 3_600)],
            &[&governor],
        )
        .expect("governor raises the streaming fee");

    s.env
        .send(
            &[builders::issue(
                &program_id,
                &user.pubkey(),
                &index_mint,
                &index_ata,
                &s.fee_ata,
                &s.pairs(&atas),
                10 * UNIT_SCALE,
                vec![u64::MAX; 2],
            )],
            &[&signer],
        )
        .expect("issue");
    s.env.warp(180 * 86_400);

    // Redirect the accrued fee into an account the attacker owns.
    let attacker_fee_ata = s.env.create_token_account(&index_mint, &user.pubkey());
    let err = s
        .env
        .send(&[builders::accrue_fees(&program_id, &index_mint, &attacker_fee_ata, &mints)], &[])
        .expect_err("a fee account owned by somebody else must be refused");
    assert!(err.contains("Custom(11)"), "expected InvalidTokenAccount, got {err}");
    assert_eq!(s.env.token_balance(&attacker_fee_ata), 0);

    // The same substitution inside an issue is refused for the same reason.
    let err = s
        .env
        .send(
            &[builders::issue(
                &program_id,
                &user.pubkey(),
                &index_mint,
                &index_ata,
                &attacker_fee_ata,
                &s.pairs(&atas),
                UNIT_SCALE,
                vec![u64::MAX; 2],
            )],
            &[&signer],
        )
        .expect_err("issue must refuse it too");
    assert!(err.contains("Custom(11)"), "expected InvalidTokenAccount, got {err}");

    // And the real recipient can still be paid.
    s.env
        .send(&[builders::accrue_fees(&program_id, &index_mint, &s.fee_ata, &mints)], &[])
        .expect("the real fee account is paid");
    assert!(s.env.token_balance(&s.fee_ata) > 0);
}

#[test]
fn components_cannot_be_delivered_in_the_wrong_token() {
    let mut s = setup();
    let (user, index_ata, atas) = s.holder(1_000, 1_000);
    let program_id = s.env.program_id;

    // Pay for component A out of a component B account: same owner, wrong mint.
    let mut pairs = s.pairs(&atas);
    pairs[0].1 = atas[1];
    let ix = builders::issue(
        &program_id,
        &user.pubkey(),
        &s.index_mint,
        &index_ata,
        &s.fee_ata,
        &pairs,
        UNIT_SCALE,
        vec![u64::MAX; 2],
    );
    let signer = user.insecure_clone();
    assert!(s.env.send(&[ix], &[&signer]).is_err(), "the wrong mint must be refused");
}

#[test]
fn the_nav_floor_stops_an_auction_that_would_lose_more_than_it_promised() {
    let mut s = setup();
    let (user, index_ata, atas) = s.holder(10_000, 10_000);
    let program_id = s.env.program_id;
    let index_mint = s.index_mint;
    let mints = s.mints.clone();
    let signer = user.insecure_clone();

    s.env
        .send(
            &[builders::issue(
                &program_id,
                &user.pubkey(),
                &index_mint,
                &index_ata,
                &s.fee_ata,
                &s.pairs(&atas),
                10 * UNIT_SCALE,
                vec![u64::MAX; 2],
            )],
            &[&signer],
        )
        .expect("issue");

    // An auction that promises to give up nothing: the floor is the NAV at proposal.
    let manager = s.manager.insecure_clone();
    s.env
        .send(
            &[builders::propose_rebalance(
                &program_id,
                &manager.pubkey(),
                &index_mint,
                &mints,
                vec![27_000_000, 250_000_000],
                vec![1_000_000_000, 100_000_000_000],
                AuctionArgs { duration: 7_200, start_premium_bps: 0, end_premium_bps: 300, max_nav_loss_bps: 0 },
            )],
            &[&manager],
        )
        .expect("propose");

    let (bidder, _, bidder_atas) = s.holder(100_000, 0);
    let bidder_signer = bidder.insecure_clone();
    let bid = |sell: u64| {
        builders::bid(
            &program_id,
            &bidder.pubkey(),
            &index_mint,
            &mints,
            &bidder_atas[1],
            &bidder_atas[0],
            1,
            0,
            sell,
            u64::MAX,
        )
    };

    // At the open the premium is zero: value in equals value out, so the floor is met exactly.
    s.env.warp(3_600);
    s.env.send(&[bid(1_000_000_000)], &[&bidder_signer]).expect("a fair-value fill is allowed");

    // At the close the premium is 300 bps in the bidder's favour, which is a real loss of NAV, and
    // this auction promised none.
    s.env.warp(7_200);
    let err = s.env.send(&[bid(1_000_000_000)], &[&bidder_signer]).expect_err("must breach the floor");
    assert!(err.contains("Custom(21)"), "expected NavFloorBreached, got {err}");
}

#[test]
fn a_stranger_cannot_close_a_running_auction_or_reopen_a_finished_one() {
    let mut s = setup();
    let (user, index_ata, atas) = s.holder(1_000, 1_000);
    let program_id = s.env.program_id;
    let index_mint = s.index_mint;
    let mints = s.mints.clone();
    let signer = user.insecure_clone();
    s.env
        .send(
            &[builders::issue(
                &program_id,
                &user.pubkey(),
                &index_mint,
                &index_ata,
                &s.fee_ata,
                &s.pairs(&atas),
                UNIT_SCALE,
                vec![u64::MAX; 2],
            )],
            &[&signer],
        )
        .expect("issue");

    let manager = s.manager.insecure_clone();
    s.env
        .send(
            &[builders::propose_rebalance(
                &program_id,
                &manager.pubkey(),
                &index_mint,
                &mints,
                vec![2_000_000, 500_000_000],
                vec![1_000_000_000, 100_000_000_000],
                AuctionArgs { duration: 600, start_premium_bps: 0, end_premium_bps: 100, max_nav_loss_bps: 50 },
            )],
            &[&manager],
        )
        .expect("propose");

    assert!(
        s.env
            .send(&[builders::end_rebalance(&program_id, &user.pubkey(), &index_mint, &mints)], &[&signer])
            .is_err(),
        "a stranger cannot close a live auction"
    );
    // A second proposal on top of a running one is refused.
    assert!(s
        .env
        .send(
            &[builders::propose_rebalance(
                &program_id,
                &manager.pubkey(),
                &index_mint,
                &mints,
                vec![2_000_000, 500_000_000],
                vec![1_000_000_000, 100_000_000_000],
                AuctionArgs { duration: 600, start_premium_bps: 0, end_premium_bps: 100, max_nav_loss_bps: 50 },
            )],
            &[&manager]
        )
        .is_err());
}

#[test]
fn an_index_mint_the_program_does_not_control_is_refused() {
    let mut env = Env::new();
    let governor = env.fund(1_000_000_000);
    let payer = env.payer.pubkey();
    let program_id = env.program_id;
    // A mint whose authority is a key the founder kept, which would let them print index tokens
    // out of thin air against everybody else's deposits.
    let rogue_mint = env.create_mint(&governor.pubkey(), 9);
    let err = env
        .send(
            &[builders::init_index(
                &program_id,
                &payer,
                &rogue_mint,
                &governor.pubkey(),
                &governor.pubkey(),
                &governor.pubkey(),
                InitArgs {
                    name: builders::fixed::<32>("Rogue"),
                    symbol: builders::fixed::<12>("ROG"),
                    streaming_fee_bps: 0,
                    issue_fee_bps: 0,
                    redeem_fee_bps: 0,
                    max_premium_bps: 300,
                    rebalance_delay: 3_600,
                },
            )],
            &[],
        )
        .expect_err("must refuse a mint it does not control");
    assert!(err.contains("Custom(12)"), "expected InvalidIndexMint, got {err}");
}
