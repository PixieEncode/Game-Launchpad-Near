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

Both were created through the testnet faucet with 10 NEAR each. Their keys
live in `~/.near-credentials/testnet/` on the developer's machine, never in
this repo.

## Libraries added during the hackathon

| Library | Version | Where | Why |
| --- | --- | --- | --- |
| near-sdk | 5.30 | `near/game` | NEAR contract SDK |
| near-contract-standards | 5.30 | `near/game` | NEP-141 coin and NEP-145 storage |
| uint | 0.10 | `near/game` | 256-bit integers for overflow-safe maths |
| ed25519-dalek | 2 (tests only) | `near/game` | Signs scores in unit tests |
| near-cli-rs | 0.31.1 (tool) | dev machine | Testnet accounts and deploys |
| cargo-near | 0.23.0 (tool) | dev machine | Builds contract WASM |

## AI-generated code

| Change | Files | Tool |
| --- | --- | --- |
| Phase 0 repo setup: LICENSE, this file, folder stubs | `LICENSE`, `NOTES.md`, `near/`, `sim/`, `verifier/` | Claude Code (Claude Opus 5.5) |
| Phase 1 game contract and its unit tests | `near/Cargo.toml`, `near/game/` | Claude Code (Claude Opus 5.5) |
