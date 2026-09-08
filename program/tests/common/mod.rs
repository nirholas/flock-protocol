// Each integration test binary compiles this module separately, so helpers only one of them uses
// read as dead code in the other. They are part of the harness either way.
#![allow(dead_code)]

//! Test harness: a real SVM, a real SPL token program, and the real deployed `flock_index.so`.
//!
//! Nothing here is mocked. Every assertion in the suite is made against the same bytes that would
//! be deployed to mainnet, which is the only kind of test worth writing for a program that holds
//! other people's money.

use litesvm::LiteSVM;
use solana_sdk::{
    clock::Clock,
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};

pub const UNIT_SCALE: u64 = 1_000_000_000;

pub struct Env {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    pub payer: Keypair,
}

pub fn program_binary() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/deploy/flock_index.so");
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!("{path} is missing ({e}). Build it first with `cargo build-sbf --arch v1`.");
    });
    let version = sbpf_version(&bytes);
    // The two runtimes this program meets disagree about SBPF versions: litesvm 0.6 executes v0 and
    // v1, while current validators refuse v0 outright, which is why `scripts/deploy.mjs` builds v3.
    // Without this check a v3 binary fails here as a bare `InvalidAccountData` from inside the VM,
    // which looks like a bug in the program and is not.
    assert!(
        version <= 1,
        "{path} is sbpf v{version}, which this runtime cannot execute. Rebuild for tests with:\n  \
         touch program/src/lib.rs && cargo build-sbf --arch v1\n\
         (`--arch` is not part of cargo-build-sbf's cache key, so the touch is what forces it.)"
    );
    bytes
}

/// The SBPF version a built program requires, read from the ELF header's `e_flags`.
fn sbpf_version(bytes: &[u8]) -> u32 {
    assert!(bytes.len() > 52 && &bytes[..4] == b"\x7fELF", "not an ELF binary");
    u32::from_le_bytes([bytes[48], bytes[49], bytes[50], bytes[51]])
}

impl Env {
    pub fn new() -> Self {
        let mut svm = LiteSVM::new();
        let program_id = Pubkey::new_unique();
        svm.add_program(program_id, &program_binary());
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        Self { svm, program_id, payer }
    }

    pub fn fund(&mut self, lamports: u64) -> Keypair {
        let kp = Keypair::new();
        self.svm.airdrop(&kp.pubkey(), lamports).unwrap();
        kp
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    /// Move the validator clock forward. Everything time-dependent in the protocol is exercised
    /// through this rather than by sleeping.
    pub fn warp(&mut self, seconds: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp += seconds;
        clock.slot += (seconds as u64).max(1);
        self.svm.set_sysvar(&clock);
    }

    pub fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> Result<(), String> {
        let mut all: Vec<&Keypair> = vec![&self.payer];
        for s in signers {
            if s.pubkey() != self.payer.pubkey() {
                all.push(s);
            }
        }
        let tx = Transaction::new_signed_with_payer(ixs, Some(&self.payer.pubkey()), &all, self.svm.latest_blockhash());
        // Failures carry the program's own logs, so a red test says which check rejected it
        // instead of only which error code came back.
        let result = self
            .svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}\n{}", e.err, e.meta.logs.join("\n")));
        self.svm.expire_blockhash();
        result
    }

    pub fn create_mint(&mut self, authority: &Pubkey, decimals: u8) -> Pubkey {
        let mint = Keypair::new();
        let rent = self.svm.minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN);
        let ixs = [
            system_instruction::create_account(
                &self.payer.pubkey(),
                &mint.pubkey(),
                rent,
                spl_token::state::Mint::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_mint2(&spl_token::id(), &mint.pubkey(), authority, None, decimals)
                .unwrap(),
        ];
        self.send(&ixs, &[&mint]).expect("create mint");
        mint.pubkey()
    }

    pub fn create_token_account(&mut self, mint: &Pubkey, owner: &Pubkey) -> Pubkey {
        let acct = Keypair::new();
        let rent = self.svm.minimum_balance_for_rent_exemption(spl_token::state::Account::LEN);
        let ixs = [
            system_instruction::create_account(
                &self.payer.pubkey(),
                &acct.pubkey(),
                rent,
                spl_token::state::Account::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_account3(&spl_token::id(), &acct.pubkey(), mint, owner).unwrap(),
        ];
        self.send(&ixs, &[&acct]).expect("create token account");
        acct.pubkey()
    }

    pub fn mint_tokens(&mut self, mint: &Pubkey, authority: &Keypair, dest: &Pubkey, amount: u64) {
        let ix =
            spl_token::instruction::mint_to(&spl_token::id(), mint, dest, &authority.pubkey(), &[], amount).unwrap();
        self.send(&[ix], &[authority]).expect("mint tokens");
    }

    pub fn token_balance(&self, account: &Pubkey) -> u64 {
        let acct = self.svm.get_account(account).expect("token account exists");
        spl_token::state::Account::unpack(&acct.data).expect("token account").amount
    }

    pub fn mint_supply(&self, mint: &Pubkey) -> u64 {
        let acct = self.svm.get_account(mint).expect("mint exists");
        spl_token::state::Mint::unpack(&acct.data).expect("mint").supply
    }

    pub fn index(&self, index_pda: &Pubkey) -> flock_index::state::Index {
        let acct = self.svm.get_account(index_pda).expect("index account exists");
        flock_index::state::decode_index(&acct.data).expect("index decodes")
    }
}

impl Env {
    /// Create the index mint the way a launch does it: derive the index PDA from the mint that
    /// does not exist yet, then create the mint with that PDA as its only authority.
    pub fn create_index_mint(&mut self) -> (Pubkey, Pubkey) {
        let mint = Keypair::new();
        let (index, _) = flock_index::state::index_pda(&self.program_id, &mint.pubkey());
        let rent = self.svm.minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN);
        let ixs = [
            system_instruction::create_account(
                &self.payer.pubkey(),
                &mint.pubkey(),
                rent,
                spl_token::state::Mint::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_mint2(&spl_token::id(), &mint.pubkey(), &index, None, 9).unwrap(),
        ];
        self.send(&ixs, &[&mint]).expect("create index mint");
        (mint.pubkey(), index)
    }
}
