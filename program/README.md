# flock-index

The Solana program behind a Flock index. Native Rust, no framework, one account per index.

## Build and test

```bash
cargo build-sbf     # writes target/deploy/flock_index.so
cargo test          # unit tests, then both integration suites against that .so in litesvm
```

`cargo test` loads the binary from `target/deploy`, so a source change that has not been rebuilt for
SBF will test the previous build. Run `cargo build-sbf` first whenever the handlers change.

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
