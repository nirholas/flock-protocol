# flock-index

The Solana program behind a Flock index. Native Rust, no framework, one account per index.

## Build and test

```bash
touch src/lib.rs && cargo build-sbf --arch v1   # what the tests execute
cargo test                                     # unit tests, then both suites against that .so
```

From the repository root, `pnpm program:test` does both.

`cargo test` loads the binary from `target/deploy`, so a source change that has not been rebuilt for
SBF will test the previous build.

### The SBPF version, which will waste an hour if you meet it cold

The two runtimes this program has to satisfy disagree about which SBPF versions may execute:

| Runtime | Accepts |
|---|---|
| litesvm 0.6 (the test harness) | v0, v1 |
| A current validator or public cluster | v1 and up; **v0 is refused** |

So the tests run a **v1** build and `scripts/deploy.mjs` builds **v3**, which is why the deploy
script does its own build rather than trusting whatever is on disk.

Two traps come with that:

- A v0 binary (the toolchain's default) is refused at deploy time with *"Detected sbpf_version
  required by the executable which are not enabled"*, surfacing as `invalid account data for
  instruction`. It reads like a corrupt account and is a build flag.
- **`--arch` is not part of cargo-build-sbf's cache key.** Switching versions without touching a
  source file silently leaves the previous binary in place, so every command here touches
  `src/lib.rs` first.

The harness reads the ELF `e_flags` and fails with both of those explained rather than letting a
mismatched binary come back as a bare `InvalidAccountData` from inside the VM.

Closing the gap properly means moving the crate to solana-program 3.x so the harness can run
litesvm 0.16, which shares a runtime with current validators. That is a dependency migration, not a
patch, and it is the next thing worth doing here.

## Files

| File | What it holds |
|---|---|
| `src/state.rs` | The zero-copy `Index` record, its PDAs, and the hard caps |
| `src/instruction.rs` | The instruction enum, with each variant's account list |
| `src/processor.rs` | Every handler, and the borrow discipline CPIs require |
| `src/math.rs` | Fixed-point arithmetic, all of it rounding in the fund's favour |
| `src/guards.rs` | The account checks every handler funnels through |
| `src/builders.rs` | Instruction builders, shared by the tests and any Rust client |
| `src/error.rs` | One numbered variant per failure mode |

## Tests

- `tests/lifecycle.rs` - issue, redeem, fees, pause, authority boundaries, a full auction
- `tests/security.rs` - the attacks: a substituted vault, a redirected fee account, a component paid
  in the wrong token, a bid that breaches the NAV floor, a mint the program does not control
- `tests/fixtures.rs` - generates the cross-language parity artifact the TypeScript SDK is tested
  against. Regenerate with `FLOCK_WRITE_FIXTURES=1 cargo test --test fixtures`

## Two things worth knowing before editing

**The stack frame is 4 KB.** The index record is 1304 bytes and the compiler makes several copies of
anything it deserializes, which is why state is read and written in place through `bytemuck` and why
handlers copy out small values rather than whole records.

**A borrow of the index account cannot be held across a CPI.** The runtime hands the program one
`RefCell` per account, so a live borrow during `invoke_signed` is a panic, not a lint. Every handler
opens a borrow, uses it, and drops it before invoking.
