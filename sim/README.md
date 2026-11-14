# sim

The deterministic game core (Rust). Everything that changes a score lives
here: the four templates (flappy, runner, shooter, dodge), their physics,
spawning, collisions, collectibles and the near-miss bonus. Integer (16.16
fixed-point) maths only, a fixed 60 ticks per second, and the tick count is
the only clock. The seeded generator is mulberry32, bit-for-bit the one the
browser engine used.

- The browser (`web/public/games/core.js`) loads `pop_sim.wasm`, steps it at
  60 ticks per game-second and only draws its snapshots. Hit-stop and slow-mo
  slow how fast ticks are shown, never how many happen.
- Each run's input changes are recorded as a compact log (`src/log.rs`) and
  posted with the score when the run ends.
- The verifier replays that log with this same crate and signs the score.

Hitboxes use each slot's nominal size (player 48 px, and so on), not the
uploaded image's aspect ratio, so every machine computes the same run.

## Build and test

```bash
cd sim
cargo test --release                                  # determinism and replay tests
cargo build --release --target wasm32-unknown-unknown # -> web/public/games/pop_sim.wasm
cp target/wasm32-unknown-unknown/release/pop_sim.wasm ../web/public/games/
cargo run --release --example replay -- runs.json     # replay recorded runs natively
```

## Evidence so far

- Every template: bot runs recorded and replayed give the identical score and
  tick count (`src/tests.rs`).
- 12 runs recorded through the WASM build (the file the browser loads) replay
  to identical results natively (`examples/replay.rs`).
- A browser run of the flappy sample (108 points, 6,576 ticks) replayed in Node
  to the same result.
- A log replayed on another seed, an edited log, or input after the run ended
  does not reproduce the run.
