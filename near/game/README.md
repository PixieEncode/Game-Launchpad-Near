# near/game

The per-game contract (Rust, near-sdk 5.30). One is deployed at each game's
own sub-account by the factory. It is the game's NEP-141 coin, the vault that
backs it, the paid run sessions, play-mining and the buy-only curve in one,
and it replaces `GameToken`, `TreasuryVault`, `SessionManager` and
`BondingCurve` from the Monad version. It has no owner and no admin methods.

## Build and test

```bash
cd near
cargo test                                   # unit tests
cargo near build non-reproducible-wasm --no-abi   # -> target/near/pop_game/pop_game.wasm
```

`cargo near` is installed with `npm install -g cargo-near` (or `cargo install cargo-near`).
On Windows the ABI step fails to link, hence `--no-abi`.

## Methods

| Method | Deposit | What it does |
| --- | --- | --- |
| `new(name, symbol, creator, parent, verifier_pk, config_hash, params)` | | Called once by the factory |
| `buy_runs(count, account_id?)` | count × fee (+ registration if new) | Prepays runs for the caller or another account |
| `start_run()` | fee (+ registration if new), or 0 to use a credit | Splits the fee, opens a session, returns `{session_id, seed}` |
| `end_run(session_id, score, log_hash, signature)` | | Checks the verifier's signature, adds the deposit to the vault, mines |
| `expire_run(session_id)` | | Anyone, after `max_run_secs`: the deposit joins the vault, nothing is mined |
| `redeem(amount)` | 1 yocto | Burns coins and pays vault × amount ÷ supply |
| `buy(amount)` | `quote_buy` (+ registration if new) | After mining closes: all of the cost goes to the vault |
| `claim_creator()` | | Creator only: pays the unlocked escrow |
| `fund()` | any | Adds the deposit to the vault (remixes pay their parent this way) |

Views: `floor_price`, `spot_price`, `quote_buy`, `preview_mined`, `get_session`,
`get_player`, `get_stats`, `get_params`, `leaderboard`, `recent_events`,
`registration_cost`, `run_cost`, `creator_unlocked`, plus NEP-141 and NEP-145.

The signed score message is described in `src/score.rs`.

## Where this differs from the Monad contracts

- **A run's vault share is held until the run ends.** On Monad the fee entered
  the vault at start, and the mint at the end then lowered vault ÷ supply. Here
  `end_run` prices the coins at the floor before the deposit lands, then adds
  it, so neither step lowers the floor. `expire_run` moves abandoned deposits in.
- **No `close_mining`.** Mining closes only on its caps (no admin powers).
- **Token storage is paid on top of the first payment.** A new account's first
  `start_run`, `buy_runs` or `buy` adds `registration_cost` (a few thousandths
  of a NEAR); no separate registration step. This keeps the 70/30 split exact.
- **Ended sessions are deleted** so storage does not grow with every run; the
  result stays in the events and the leaderboard.
- **Redeem needs 1 yoctoNEAR** so a limited access key cannot call it.
- All products use 256-bit maths and round in the direction that keeps the floor.
