# flock

Command line for creating and operating an index.

```bash
pnpm build && node dist/index.js --help
# or during development
pnpm dev -- inspect <indexMint>
```

## Commands

| Command | What it does |
|---|---|
| `create` | Creates the mint (index PDA as sole authority, no freeze authority), the account, every component, and seals it |
| `inspect` | Composition, weights, NAV, accrued fee, and the live auction with its per-leg sizes |
| `issue` / `redeem` | In-kind subscription and redemption, with a slippage bound |
| `accrue` | Mints the streaming fee owed so far |
| `propose` | Turns a target weighting plus live prices into an auction |
| `bid` | Fills part of a running auction, printing the market edge first |
| `end` | Closes an auction |

## Two flags worth knowing

`--dry-run` on `propose` and `bid` prints the exact move and the exact cost without sending
anything. Use it before every proposal.

`--weights MINT=BPS,...` must add up to 10000. The CLI refuses a weighting that does not rather than
normalising it, because a weighting that does not add up is a mistake in the methodology.

## Configuration

| Flag | Environment | Default |
|---|---|---|
| `--url` | `SOLANA_RPC_URL` | mainnet-beta |
| `--program-id` | `FLOCK_PROGRAM_ID` | `deployments/<cluster>.json` |
| `--keypair` | `FLOCK_KEYPAIR` | `~/.config/solana/id.json` |
