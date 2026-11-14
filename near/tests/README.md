# near/tests

Sandbox tests: the real factory and game contracts on a local NEAR node, with
real accounts and real access keys (near-workspaces).

| Test | What it proves |
| --- | --- |
| `factory_creates_a_game_and_the_registry_lists_it` | `create_game` launches a working game at `frog-1.<factory>`, the registry views list it, a first run mines 140 coins at a 0.0001 NEAR floor, and the game account has no access keys |
| `a_remix_run_pays_the_parents_vault` | A remix run sends 10% of its fee into the parent game's vault |
| `a_failed_parent_payment_stays_in_the_remix_vault` | When the parent's `fund` call fails, that share is kept in the remix's own vault |
| `a_limited_key_plays_prepaid_runs_but_cannot_redeem` | A function-call key limited to `start_run` and `end_run` plays a prepaid run start to finish and cannot redeem |

The score signature is rebuilt here independently of the contract code
(sha2 + manual borsh), so these tests also check the message format the
verifier must sign.

## Run

The NEAR sandbox runs on Linux and macOS (on Windows, use WSL). Build both
contracts first; the tests read them from `near/target/near/`.

```bash
cd near
cargo near build non-reproducible-wasm --no-abi --manifest-path game/Cargo.toml
cargo near build non-reproducible-wasm --no-abi --manifest-path factory/Cargo.toml
cargo test -p pop-sandbox-tests
```

CI runs them on every push that touches `near/` (`.github/workflows/contracts.yml`).
