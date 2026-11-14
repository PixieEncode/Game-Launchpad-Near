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

## AI-generated code

| Change | Files | Tool |
| --- | --- | --- |
| Phase 0 repo setup: LICENSE, this file, folder stubs | `LICENSE`, `NOTES.md`, `near/`, `sim/`, `verifier/` | Claude Code (Claude Opus 5.5) |
| Phase 1 game contract and its unit tests | `near/Cargo.toml`, `near/game/` | Claude Code (Claude Opus 5.5) |
| Phase 2 factory contract, its tests, testnet deploy | `near/factory/`, `web/src/lib/contracts/deployment.testnet.json` | Claude Code (Claude Opus 5.5) |
| Phase 3 floor simulation, scenario runner, sandbox tests, CI | `near/game/src/tests/sim.rs`, `near/simulation/`, `near/tests/`, `.github/workflows/` | Claude Code (Claude Opus 5.5) |
