//! Cross-language parity fixtures.
//!
//! The Rust builders and the TypeScript SDK have to encode the same twelve instructions the same
//! way forever. Testing each against itself proves nothing, so both are tested against one
//! committed artifact: this file writes it, `packages/sdk/test/parity.test.ts` reads it.
//!
//! Regenerate after any instruction or state change:
//!
//! ```text
//! FLOCK_WRITE_FIXTURES=1 cargo test --test fixtures
//! ```
//!
//! A change that alters the wire format without regenerating fails here, and a change that
//! regenerates without updating the SDK fails there.

use flock_index::{
    builders::{self, AuctionArgs, InitArgs},
    state::{Component, Index, Rebalance, ACCOUNT_TAG_INDEX, STATE_LIVE, STATE_VERSION},
};
use serde_json::{json, Value};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

fn key(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode(name: &str, ix: Instruction) -> Value {
    json!({
        "name": name,
        "programId": ix.program_id.to_string(),
        "accounts": ix.accounts.iter().map(|a| json!({
            "pubkey": a.pubkey.to_string(),
            "isSigner": a.is_signer,
            "isWritable": a.is_writable,
        })).collect::<Vec<_>>(),
        "dataHex": hex(&ix.data),
    })
}

/// An index account with every field set to something distinguishable, so a decoder that reads a
/// neighbouring field by mistake produces a visibly wrong answer rather than a plausible one.
fn sample_index() -> Index {
    let mut idx = bytemuck::Zeroable::zeroed();
    let index: &mut Index = &mut idx;
    index.tag = ACCOUNT_TAG_INDEX;
    index.version = STATE_VERSION;
    index.bump = 254;
    index.state = STATE_LIVE;
    index.decimals = 9;
    index.index_mint = key(1);
    index.governor = key(2);
    index.manager = key(3);
    index.fee_recipient = key(4);
    index.streaming_fee_bps = 95;
    index.issue_fee_bps = 10;
    index.redeem_fee_bps = 15;
    index.max_premium_bps = 300;
    index.rebalance_delay = 86_400;
    index.last_fee_accrual = 1_767_225_600;
    index.created_at = 1_756_684_800;
    index.name = builders::fixed::<32>("Flock DeFi Index");
    index.symbol = builders::fixed::<12>("FDI");
    index.component_count = 2;
    index.components[0] = Component {
        mint: key(5),
        units: 1_234_567,
        target_units: 2_000_000,
        ref_price_e9: 1_050_000_000,
        decimals: 6,
        vault_bump: 253,
        _pad: [0; 6],
    };
    index.components[1] = Component {
        mint: key(6),
        units: 987_654_321,
        target_units: 500_000_000,
        ref_price_e9: 103_930_000_000,
        decimals: 9,
        vault_bump: 252,
        _pad: [0; 6],
    };
    index.rebalance = Rebalance {
        proposer: key(3),
        proposed_at: 1_767_000_000,
        start_ts: 1_767_086_400,
        end_ts: 1_767_093_600,
        nav_floor_e6: 998_500_000,
        start_premium_bps: -50,
        end_premium_bps: 150,
        max_nav_loss_bps: 100,
        active: 1,
        _pad: [0; 1],
    };
    idx
}

fn build() -> Value {
    let program = key(9);
    let payer = key(10);
    let index_mint = key(1);
    let governor = key(2);
    let manager = key(3);
    let fee_recipient = key(4);
    let component_a = key(5);
    let component_b = key(6);
    let user = key(11);
    let user_index_ata = key(12);
    let fee_ata = key(13);
    let user_a = key(14);
    let user_b = key(15);
    let bidder = key(16);
    let mints = [component_a, component_b];
    let pairs = [(component_a, user_a), (component_b, user_b)];

    let instructions = vec![
        encode(
            "initIndex",
            builders::init_index(
                &program,
                &payer,
                &index_mint,
                &governor,
                &manager,
                &fee_recipient,
                InitArgs {
                    name: builders::fixed::<32>("Flock DeFi Index"),
                    symbol: builders::fixed::<12>("FDI"),
                    streaming_fee_bps: 95,
                    issue_fee_bps: 10,
                    redeem_fee_bps: 15,
                    max_premium_bps: 300,
                    rebalance_delay: 86_400,
                },
            ),
        ),
        encode(
            "addComponent",
            builders::add_component(&program, &manager, &payer, &index_mint, &component_a, 1_234_567),
        ),
        encode("sealIndex", builders::seal_index(&program, &manager, &index_mint)),
        encode(
            "issue",
            builders::issue(
                &program,
                &user,
                &index_mint,
                &user_index_ata,
                &fee_ata,
                &pairs,
                5_000_000_000,
                vec![10_000_000, 3_000_000_000],
            ),
        ),
        encode(
            "redeem",
            builders::redeem(
                &program,
                &user,
                &index_mint,
                &user_index_ata,
                &fee_ata,
                &pairs,
                2_500_000_000,
                vec![1, 2],
            ),
        ),
        encode("accrueFees", builders::accrue_fees(&program, &index_mint, &fee_ata, &mints)),
        encode(
            "proposeRebalance",
            builders::propose_rebalance(
                &program,
                &manager,
                &index_mint,
                &mints,
                vec![2_000_000, 500_000_000],
                vec![1_050_000_000, 103_930_000_000],
                AuctionArgs {
                    duration: 7_200,
                    start_premium_bps: -50,
                    end_premium_bps: 150,
                    max_nav_loss_bps: 100,
                },
            ),
        ),
        encode(
            "bid",
            builders::bid(
                &program,
                &bidder,
                &index_mint,
                &mints,
                &user_a,
                &user_b,
                1,
                0,
                2_500_000_000,
                251_250_000,
            ),
        ),
        encode("endRebalance", builders::end_rebalance(&program, &bidder, &index_mint, &mints)),
        encode(
            "setParams",
            builders::set_params(&program, &governor, &index_mint, 95, 10, 15, 300, 86_400),
        ),
        encode(
            "setAuthority",
            builders::set_authority(&program, &governor, &index_mint, 1, &key(17)),
        ),
        encode("setPaused", builders::set_paused(&program, &manager, &index_mint, true)),
    ];

    json!({
        "note": "Generated by program/tests/fixtures.rs. Regenerate with FLOCK_WRITE_FIXTURES=1 cargo test --test fixtures.",
        "programId": program.to_string(),
        "indexMint": index_mint.to_string(),
        "indexAccountHex": hex(bytemuck::bytes_of(&sample_index())),
        "instructions": instructions,
    })
}

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packages/sdk/test/fixtures/instructions.json")
}

#[test]
fn fixtures_match_the_builders() {
    let built = build();
    let rendered = format!("{}\n", serde_json::to_string_pretty(&built).unwrap());
    let path = fixture_path();
    if std::env::var("FLOCK_WRITE_FIXTURES").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &rendered).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("{} is missing ({e}). Write it with FLOCK_WRITE_FIXTURES=1.", path.display())
    });
    assert_eq!(
        on_disk, rendered,
        "the committed fixture no longer matches the builders. If the change is intended, \
         regenerate with FLOCK_WRITE_FIXTURES=1 and update the SDK to match."
    );
}
