# verifier

The OutLayer program that replays a run and signs the score
(wasm32-wasip2, deployed as project `popnear.testnet/pop-verifier`).

Input (JSON on stdin):

```json
{"op": "verify", "game": "…", "player": "…", "session_id": 1, "seed": 123,
 "config": {…gameplay fields…}, "cosmetics_hash": "<64 hex>", "log": "<base64>"}
```

It replays the log with the same game core the browser runs (`../sim`),
computes the config hash (`sha256("pop-config-v1" ‖ rules hash ‖ cosmetics
hash)`) and the log hash, and signs

```
sha256("pop-score-v1" ‖ borsh(game, player, session_id, seed, config_hash, score, ticks, log_hash))
```

Output: `{"ok": true, "score", "ticks", "config_hash", "log_hash", "signature"}`
(well under OutLayer's 900-byte limit). `{"op": "public_key"}` returns the key.

## The signing key

`manifest.json` is embedded as the `outlayer.manifest` custom section and
declares one ed25519 key, `caller: "none"`, bound to the project. OutLayer
derives it inside the TEE; this module never sees it and asks the host to
sign through `outlayer:signing-keys` (`wit/signing-keys.wit`, copied from
OutLayer's `worker/wit/deps/signing-keys.wit`). The key is the same for every
caller and every version of the project, so a game pins it once.

The module signs only the message it composes itself, under a fixed prefix,
from fields it parsed and checked. The signature means "replaying this log on
this seed under these rules gives this score in this many ticks"; the game
contract decides who may use it (the session's player, within the run's time
window).

## Build and deploy

```bash
cd verifier
cargo test --release                              # includes a known-answer digest shared with the contract
cargo build --release --target wasm32-wasip2
outlayer upload target/wasm32-wasip2/release/pop-verifier.wasm
outlayer deploy pop-verifier <fastfs url> --hash <sha256>
```

The OutLayer CLI is `cargo install outlayer-cli` (it builds on Linux/WSL).
