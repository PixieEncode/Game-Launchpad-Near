# Build notes

A running record for the NEAR Hackathon build. The hackathon rules ask the
README to list every library used and to say which code is AI-generated;
Phase 8 copies this file into the README.

Everything before the `pre-hackathon` tag is prior work from the Monad Blitz
version. Only changes after that tag are hackathon work.

## Testnet accounts

| Account | Role |
| --- | --- |
| [popnear.testnet](https://testnet.nearblocks.io/address/popnear.testnet) | Owns the factories. Phase 2 deploys them as sub-accounts, e.g. `factory.popnear.testnet` and `demo.popnear.testnet` |
| [popnear-player.testnet](https://testnet.nearblocks.io/address/popnear-player.testnet) | Test player |
| [factory.popnear.testnet](https://testnet.nearblocks.io/address/factory.popnear.testnet) | Factory with the standard params (300-run mining) |
| [demo.popnear.testnet](https://testnet.nearblocks.io/address/demo.popnear.testnet) | Small-cap factory: mining closes after 10 runs, for demos |
| [popnear-funds.testnet](https://testnet.nearblocks.io/address/popnear-funds.testnet) | Faucet account only: its 9.95 NEAR went to `popnear.testnet` ([6K5eEoX6…](https://testnet.nearblocks.io/txns/6K5eEoX6dC3GGj17dXVR5hPdChn1qLhdEKdVeC8t1FJN)) |
| popnear-funds2.testnet, popnear-funds3.testnet | Faucet accounts only: 9.95 NEAR each went to `popnear.testnet` (Phase 4 and 5 games) |

Both were created through the testnet faucet with 10 NEAR each. Their keys
live in `~/.near-credentials/testnet/` on the developer's machine, never in
this repo.

## Testnet deploy log

Both factories launch games with the **dev verifier** key
`06727a24…ea3b81` until the OutLayer verifier is live (Phase 4); those games
must be relaunched then. The dev key's private half is in `web/.env.local`,
which is never committed.

| What | Transaction |
| --- | --- |
| Deploy `factory.popnear.testnet` | [7pxrn75R…](https://testnet.nearblocks.io/txns/7pxrn75RMcz7AXCYqxfdYrHkrepcRbnnbxqgpppd24gr) |
| Launch game 1, `frog-1.factory.popnear.testnet` | [AzznCobY…](https://testnet.nearblocks.io/txns/AzznCobYu3BkJYmHYEmpgT8ENoA11j9N1g3ESAUdvgSV) |
| First paid run on game 1 (`start_run`) | [ERbNjddZ…](https://testnet.nearblocks.io/txns/ERbNjddZ6iRTr1PJ7WY5jCMFeVNyM9iRYCxSLDvTTZSg) |
| `end_run`, score 40, dev-signed: mined 140 FROG, floor 0.0001 NEAR | [6iV5zCvq…](https://testnet.nearblocks.io/txns/6iV5zCvqssfih8PxVmg3GkoYuioMD9FXoA6gLhQ1RJjo) |
| Launch remix, `frog2-2.factory.popnear.testnet` (parent game 1) | [6n77KMDb…](https://testnet.nearblocks.io/txns/6n77KMDbvL4AwF9ywZCsEdmUQMQh6sY8PBfTDj3zqfAP) |
| Remix run: parent vault 0.014 → 0.016 NEAR (10% of the fee) | [qUbw7HX7…](https://testnet.nearblocks.io/txns/qUbw7HX7WNayPBNYvw9FG1bZz91uN1846GqShhRff7s) |
| Deploy `demo.popnear.testnet` (10-run cap) | [DMDy9EAD…](https://testnet.nearblocks.io/txns/DMDy9EAD4Ksi7JYYb1iQf9T3nDo5aNKRZHmBjM6ANLJk) |

## Phase 4: score verification on testnet

`web/scripts/verifier-testnet.mjs` launches games, plays a run with a bot
through the same WASM core the browser uses, signs it through
`/api/verify-run`, and submits forged and honest results. Result on
`vchk-4.factory.popnear.testnet` and `vchkb-2.demo.popnear.testnet`
(dev verifier key):

| Attempt | Result |
| --- | --- |
| Submitted faster than the run could be played | refused: "Run submitted faster than it could be played" |
| Raised score (+10), same signature | refused: "Bad signature" |
| Edited input log (another log hash) | refused: "Bad signature" |
| Shorter run claimed (ticks − 60) | refused: "Bad signature" |
| Run A's signature on another session of the same game | refused: "Bad signature" |
| Run A's signature on a different game | refused: "Bad signature" |
| Honest run (13 points, 1,226 ticks) | accepted, mined 17.28 coins |

The verifier is deployed on OutLayer testnet as `popnear.testnet/pop-verifier`
(wasm sha256 `1225c52e…5a18e`). Its enclave key is
`7b2bfc37d4edcbfda5415cae6300e8b5d9fa7e9251618ed701f6190a3df093ee`, read from
the program itself over HTTPS. Through OutLayer it replayed a recorded flappy
run to the same 123 points in 8,386 ticks, and its signature verifies against
that key. Both factories now launch new games with this key; games launched
earlier keep the development key (`06727a24…ea3b81`) and are "dev verifier"
games. OutLayer runs take under 0.5 s but can wait a minute or more in the
testnet queue, so `/api/verify-run` calls it asynchronously and polls. `vchk-3.factory…` and `vchkb-1.demo…` were
launched with a game build missing the `pop-score-v1` prefix; they accept no
scores and are not used.

### With the OutLayer verifier

The same check on games launched with the enclave key, signed through
OutLayer (`SKIP_TOO_FAST=1 node scripts/verifier-testnet.mjs`), on
`vchk-6.factory.popnear.testnet` and `vchkb-4.demo.popnear.testnet`:

| Attempt | Result |
| --- | --- |
| Raised score (+10), same signature | refused: "Bad signature" |
| Edited input log (another log hash) | refused: "Bad signature" |
| Shorter run claimed (ticks − 60) | refused: "Bad signature" |
| Run A's signature on another session of the same game | refused: "Bad signature" |
| Run A's signature on a different game | refused: "Bad signature" |
| Honest run (15 points, 1,240 ticks) | accepted, mined 74.37 coins |

The "submitted too fast" attempt is skipped here: OutLayer's signature takes
longer than a 20-second run, so it cannot arrive early. The time check does
not depend on the verifier and was shown above with the dev key.

The first attempt (`vchk-5`, `vchkb-3`) used a 2-minute `end_run` grace; the
OutLayer queue took longer, so even the honest run arrived "too late". The
grace is now 5 minutes and runs expire after 15 (`max_run_secs` 900); those two
games keep the old window.

## Libraries added during the hackathon

| Library | Version | Where | Why |
| --- | --- | --- | --- |
| near-sdk | 5.30 | `near/game`, `near/factory` | NEAR contract SDK |
| near-contract-standards | 5.30 | `near/game` | NEP-141 coin and NEP-145 storage |
| uint | 0.10 | `near/game` | 256-bit integers for overflow-safe maths |
| ed25519-dalek | 2 (tests only) | `near/game` | Signs scores in unit tests |
| near-cli-rs | 0.31.1 (tool) | dev machine | Testnet accounts and deploys |
| cargo-near | 0.23.0 (tool) | dev machine, CI | Builds contract WASM |
| near-workspaces | 0.24 | `near/tests` | Sandbox tests on a local NEAR node |
| tokio, anyhow, serde_json, sha2, hex | 1, 1, 1, 0.10, 0.4 | `near/tests` | Async runtime and helpers for the sandbox tests |
| serde, serde_json, sha2 | 1, 1, 0.10 | `sim` | Config parsing, snapshots, rules and log hashes |
| wit-bindgen, base64 | 0.36, 0.22 | `verifier` | OutLayer signing-key bindings; input log decoding |
| near-api-js | 7.5.3 | `web` | NEAR calls from scripts (and the app from Phase 5) |
| outlayer-cli | 0.2.3 (tool) | dev machine (WSL) | Uploads and deploys the verifier |

`verifier/wit/signing-keys.wit` is copied unchanged from OutLayer's
`worker/wit/deps/signing-keys.wit` (github.com/out-layer/outlayer).

## AI-generated code

| Change | Files | Tool |
| --- | --- | --- |
| Phase 0 repo setup: LICENSE, this file, folder stubs | `LICENSE`, `NOTES.md`, `near/`, `sim/`, `verifier/` | Claude Code (Claude Opus 5.5) |
| Phase 1 game contract and its unit tests | `near/Cargo.toml`, `near/game/` | Claude Code (Claude Opus 5.5) |
| Phase 2 factory contract, its tests, testnet deploy | `near/factory/`, `web/src/lib/contracts/deployment.testnet.json` | Claude Code (Claude Opus 5.5) |
| Phase 4 deterministic game core, WASM API, browser renderers, verifier, `/api/verify-run`, `end_run` time check | `sim/`, `verifier/`, `web/public/games/`, `web/src/lib/verify-run.ts`, `web/src/app/api/verify-run/`, `web/scripts/verifier-testnet.mjs`, `near/game/src/` | Claude Code (Claude Opus 5.5) |
| Phase 3 floor simulation, scenario runner, sandbox tests, CI | `near/game/src/tests/sim.rs`, `near/simulation/`, `near/tests/`, `.github/workflows/` | Claude Code (Claude Opus 5.5) |
