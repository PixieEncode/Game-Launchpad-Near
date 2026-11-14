# near/factory

The factory contract (Rust, near-sdk 5.30). It replaces `GameFactory.sol`:
it launches each game at its own sub-account and keeps the registry.

`create_game(name, symbol, config_hash, parent_id)` (attach at least
`game_storage_cost`, about 2.78 NEAR) runs one batch on
`<symbol>-<id>.<factory>`: create the account, fund it with the attached NEAR,
deploy the embedded game contract, call `new`. A callback records the game only
if the batch succeeded and refunds the caller otherwise. The game account gets
no access key, so a launched game can never be changed. The factory owner can
only change the default params and verifier key used for future games.

Views: `game_count`, `get_game(id)`, `list_games(offset, limit)` (newest
first), `game_id_by_account`, `game_storage_cost`, `get_default_params`,
`get_verifier_pk`, `get_owner`, `game_code_hash`.

## Build

The factory embeds the game WASM with `include_bytes!`, so build the game first:

```bash
cd near
cargo near build non-reproducible-wasm --no-abi --manifest-path game/Cargo.toml
cargo near build non-reproducible-wasm --no-abi --manifest-path factory/Cargo.toml
cargo test
```

## Size and storage cost (measured, Phase 2)

| Contract | WASM | Locked for storage |
| --- | --- | --- |
| Game | 247.9 KB | 2.78 NEAR per game (code + 30 KB state margin) |
| Factory (embeds the game) | 436.6 KB | about 4.4 NEAR per factory |

NEAR's price is 1 NEAR per 100 KB, so the docs' estimate holds.
