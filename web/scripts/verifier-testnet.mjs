// Phase 4 check on NEAR testnet: one honest run is accepted, and forged
// submissions are refused by the game contract.
//
//   node scripts/verifier-testnet.mjs            (dev server running on :3100)
//
// Launches two fresh games (one per factory), plays a short run with a bot
// through the same WASM core the browser uses, gets it signed by
// /api/verify-run, then submits the attacks below before the honest run.
// Keys come from ~/.near-credentials/testnet (popnear.testnet, popnear-player.testnet).
import { Account, JsonRpcProvider } from "near-api-js";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const RPC = "https://rpc.testnet.near.org";
const API = process.env.VERIFY_URL || "http://localhost:3100/api/verify-run";
const provider = new JsonRpcProvider({ url: RPC });
const key = (id) => JSON.parse(readFileSync(path.join(os.homedir(), ".near-credentials/testnet", `${id}.json`), "utf8")).private_key;
const owner = new Account("popnear.testnet", provider, key("popnear.testnet"));
const player = new Account("popnear-player.testnet", provider, key("popnear-player.testnet"));
const TGAS = 1_000_000_000_000n;
const view = (contractId, method, args = {}) => provider.callFunction({ contractId, method, args });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha = (...parts) => {
  const h = createHash("sha256");
  parts.forEach((p) => h.update(p));
  return h.digest();
};

// ---- a game config, its cosmetics hash and its on-chain config hash
const config = {
  version: 1,
  template: "flappy",
  title: "Verifier Check",
  tagline: "Phase 4 testnet check",
  scoreLabel: "gates",
  palette: { sky: "#0b0f2e", skyBottom: "#3a1c71", ground: "#2b1d4f", accent: "#ffd23f", text: "#ffffff" },
  sprites: { player: "<svg></svg>", obstacle: "<svg></svg>", collectible: "<svg></svg>" },
  physics: { gravity: 1400, jumpVelocity: 470, scrollSpeed: 170, speedRampPerSec: 4 },
  obstacles: { spawnIntervalMs: 1550, gapSize: 195, variance: 0.6 },
  scoring: { pointsPerObstacle: 1, collectibleChance: 0.45, pointsPerCollectible: 3 },
};
const sortKeys = (v) =>
  Array.isArray(v) ? v.map(sortKeys) : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sortKeys(v[k])])) : v;
const cosmeticsHash = sha(JSON.stringify(sortKeys(config))).toString("hex");

const wasm = await WebAssembly.instantiate(readFileSync("public/games/pop_sim.wasm"), {});
const m = wasm.instance.exports;
const put = (b) => {
  const p = m.pop_alloc(b.length);
  new Uint8Array(m.memory.buffer, p, b.length).set(b);
  return [p, b.length];
};
const out = () => Buffer.from(new Uint8Array(m.memory.buffer, m.pop_out_ptr(), m.pop_out_len()));
m.pop_rules_hash(...put(Buffer.from(JSON.stringify(config))));
const configHash = sha(Buffer.from("pop-config-v1"), Buffer.from(out().toString(), "hex"), Buffer.from(cosmeticsHash, "hex")).toString("hex");

// ---- helpers
async function launch(factory, symbol) {
  const cost = await view(factory, "game_storage_cost");
  const id = await owner.callFunction({
    contractId: factory,
    methodName: "create_game",
    args: { name: "Verifier Check", symbol, config_hash: configHash, parent_id: 0 },
    deposit: BigInt(cost),
    gas: 150n * TGAS,
  });
  const game = await view(factory, "get_game", { id });
  if (!game) throw new Error(`launch ${symbol} failed`);
  return game.account;
}

async function startRun(game) {
  const cost = await view(game, "run_cost", { account_id: player.accountId });
  const run = await player.callFunction({ contractId: game, methodName: "start_run", deposit: BigInt(cost), gas: 50n * TGAS });
  return { ...run, startedAt: Date.now() };
}

/** An honest run: fly through the pipes for ~20 s (1,200 ticks), then stop and fall. */
function play(seed) {
  m.pop_init(...put(Buffer.from(JSON.stringify(config))), seed);
  for (let t = 0; t < 36000 && !m.pop_over(); t++) {
    m.pop_frame();
    const s = JSON.parse(out().toString()).snapshot;
    const p = s.player;
    const pipe = s.ents.filter((e) => e.k === "pipe" && e.x + 36 > p.x - 30).sort((a, b) => a.x - b.x)[0];
    const target = pipe ? (pipe.a + pipe.b) / 2 + 18 : 330;
    const tap = t === 0 || (t < 1200 && p.y > target && p.v > -40);
    m.pop_step(tap ? 33 : 0, 240);
  }
  m.pop_log();
  return { log: out().toString("base64"), score: m.pop_score(), ticks: m.pop_tick() };
}

async function sign(game, run, log) {
  const res = await fetch(API, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ game, player: player.accountId, sessionId: run.session_id, seed: run.seed, config, cosmeticsHash, log }),
  });
  const j = await res.json();
  if (!res.ok) throw new Error(`verify-run: ${j.error}`);
  return j;
}

/** Submits end_run; returns "accepted (mined …)" or the contract's refusal. */
async function endRun(game, sessionId, s) {
  try {
    const mined = await player.callFunction({
      contractId: game,
      methodName: "end_run",
      args: { session_id: sessionId, score: s.score, ticks: s.ticks, log_hash: s.logHash, signature: s.signature },
      gas: 50n * TGAS,
    });
    return `accepted, mined ${(Number(BigInt(mined)) / 1e18).toFixed(2)} coins`;
  } catch (e) {
    const msg = String(e?.message ?? e);
    return "REFUSED: " + (msg.match(/Smart contract panicked: ([^"\\]+)/)?.[1] ?? msg.slice(0, 120));
  }
}

// ---- the check
console.log(`config hash ${configHash.slice(0, 16)}…  cosmetics ${cosmeticsHash.slice(0, 16)}…`);
// GAME_A / GAME_B reuse games launched by an earlier run (each launch locks ~2.78 NEAR).
const gameA = process.env.GAME_A || (await launch("factory.popnear.testnet", "VCHK"));
const gameB = process.env.GAME_B || (await launch("demo.popnear.testnet", "VCHKB"));
console.log(`games ${gameA} and ${gameB}`);

const runA = await startRun(gameA);
const played = play(runA.seed);
const sigA = await sign(gameA, runA, played.log);
console.log(`run A: session ${runA.session_id}, seed ${runA.seed}, score ${sigA.score} in ${sigA.ticks} ticks (${sigA.verifier} verifier)`);

const rows = [];
rows.push(["Submitted faster than the run could be played", await endRun(gameA, runA.session_id, sigA)]);
const wait = runA.startedAt + Math.ceil((sigA.ticks * 1000) / 60) + 2000 - Date.now();
if (wait > 0) await sleep(wait);
rows.push(["Raised score (+10), same signature", await endRun(gameA, runA.session_id, { ...sigA, score: sigA.score + 10 })]);
rows.push(["Edited input log (another log hash)", await endRun(gameA, runA.session_id, { ...sigA, logHash: sha(Buffer.from("edited")).toString("hex") })]);
rows.push(["Shorter run claimed (ticks − 60)", await endRun(gameA, runA.session_id, { ...sigA, ticks: sigA.ticks - 60 })]);
const runA2 = await startRun(gameA);
await sleep(Math.ceil((sigA.ticks * 1000) / 60) + 2000);
rows.push(["Run A's signature on another session (same game)", await endRun(gameA, runA2.session_id, sigA)]);
const runB = await startRun(gameB);
await sleep(Math.ceil((sigA.ticks * 1000) / 60) + 2000);
rows.push(["Run A's signature on a different game", await endRun(gameB, runB.session_id, sigA)]);
rows.push(["Honest run A", await endRun(gameA, runA.session_id, sigA)]);

console.log("\n| Attempt | Result |\n| --- | --- |");
for (const [a, r] of rows) console.log(`| ${a} | ${r} |`);
const stats = await view(gameA, "get_stats");
console.log(`\n${gameA}: finished runs ${stats.finished_runs}, supply ${(Number(BigInt(stats.supply)) / 1e18).toFixed(2)}, floor ${stats.floor_price} yocto/coin`);
