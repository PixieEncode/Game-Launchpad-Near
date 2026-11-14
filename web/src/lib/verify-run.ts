/**
 * Score verification for a finished run (server only).
 *
 * With OUTLAYER_PAYMENT_KEY set, the run goes to the verifier on OutLayer
 * (verifier/, project OUTLAYER_PROJECT): it replays the input log inside a TEE
 * and signs the score with a key that never leaves the enclave. This server
 * holds only the payment key, so it cannot forge a score.
 *
 * Without it (development), the same game core (public/games/pop_sim.wasm)
 * replays the run here and a development key (DEV_VERIFIER_SEED) signs it.
 * Games launched with that key are marked "dev verifier".
 *
 * Either way the signed message is the one the game contract checks
 * (near/game/src/score.rs, verifier/src/lib.rs):
 *   sha256("pop-score-v1" ‖ borsh(game, player, session_id, seed, config_hash, score, ticks, log_hash))
 */
import { createHash, createPrivateKey, sign as edSign } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";

export type VerifyRequest = {
  game: string;
  player: string;
  sessionId: number;
  seed: number;
  /** The game config (its gameplay fields are what the replay reads). */
  config: Record<string, unknown>;
  /** sha256 of the full config's canonical JSON, 64 hex. */
  cosmeticsHash: string;
  /** The input log the engine posted, base64. */
  log: string;
};

export type Verified = {
  score: number;
  ticks: number;
  configHash: string;
  logHash: string;
  signature: string;
  verifier: "outlayer" | "dev";
};

const OUTLAYER_API = process.env.OUTLAYER_API_URL || "https://testnet-api.outlayer.ai";
const OUTLAYER_PROJECT = process.env.OUTLAYER_PROJECT || "popnear.testnet/pop-verifier";

const sha256 = (...parts: Uint8Array[]) => {
  const h = createHash("sha256");
  for (const p of parts) h.update(p);
  return new Uint8Array(h.digest());
};
const hex = (b: Uint8Array) => Buffer.from(b).toString("hex");

/** Only the fields the replay reads: keeps the verifier's input small. */
export function gameplayConfig(config: Record<string, unknown>) {
  const sprites = (config.sprites ?? {}) as Record<string, unknown>;
  return {
    version: config.version,
    template: config.template,
    sprites: sprites.collectible ? { collectible: "x" } : {},
    physics: config.physics,
    obstacles: config.obstacles,
    scoring: config.scoring,
    ...(config.tuning ? { tuning: config.tuning } : {}),
  };
}

export function scoreDigest(f: {
  game: string;
  player: string;
  sessionId: number;
  seed: number;
  configHash: Uint8Array;
  score: number;
  ticks: number;
  logHash: Uint8Array;
}) {
  const str = (s: string) => {
    const b = Buffer.from(s, "utf8");
    const len = Buffer.alloc(4);
    len.writeUInt32LE(b.length);
    return Buffer.concat([len, b]);
  };
  const u32 = (n: number) => {
    const b = Buffer.alloc(4);
    b.writeUInt32LE(n >>> 0);
    return b;
  };
  const u64 = (n: number) => {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(BigInt(n));
    return b;
  };
  return sha256(
    Buffer.from("pop-score-v1"),
    str(f.game),
    str(f.player),
    u64(f.sessionId),
    u32(f.seed),
    f.configHash,
    u32(f.score),
    u32(f.ticks),
    f.logHash,
  );
}

// ------------------------------------------------------------ OutLayer path

async function viaOutlayer(req: VerifyRequest, paymentKey: string): Promise<Verified> {
  const res = await fetch(`${OUTLAYER_API}/call/${OUTLAYER_PROJECT}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Payment-Key": paymentKey },
    body: JSON.stringify({
      input: {
        op: "verify",
        game: req.game,
        player: req.player,
        session_id: req.sessionId,
        seed: req.seed,
        config: gameplayConfig(req.config),
        cosmetics_hash: req.cosmeticsHash,
        log: req.log,
      },
    }),
  });
  const body = await res.json().catch(() => null);
  if (!res.ok || !body) throw new Error(`OutLayer call failed: HTTP ${res.status}`);
  const out = typeof body.output === "string" ? JSON.parse(body.output) : body.output;
  if (!out?.ok) throw new Error(`Verifier refused the run: ${out?.error ?? "no output"}`);
  return {
    score: out.score,
    ticks: out.ticks,
    configHash: out.config_hash,
    logHash: out.log_hash,
    signature: out.signature,
    verifier: "outlayer",
  };
}

// ---------------------------------------------------- development path

type SimExports = {
  memory: WebAssembly.Memory;
  pop_alloc(len: number): number;
  pop_free(ptr: number, len: number): void;
  pop_out_ptr(): number;
  pop_out_len(): number;
  pop_rules_hash(ptr: number, len: number): number;
  pop_replay(cptr: number, clen: number, seed: number, lptr: number, llen: number): number;
};

let sim: Promise<SimExports> | null = null;
function loadSim() {
  sim ??= readFile(path.join(process.cwd(), "public/games/pop_sim.wasm"))
    .then((b) => WebAssembly.instantiate(b, {}))
    .then((r) => r.instance.exports as unknown as SimExports);
  return sim;
}

function call(m: SimExports, f: (put: (b: Uint8Array) => [number, number]) => number) {
  const put = (b: Uint8Array): [number, number] => {
    const p = m.pop_alloc(b.length);
    new Uint8Array(m.memory.buffer, p, b.length).set(b);
    return [p, b.length];
  };
  const rc = f(put);
  const out = Buffer.from(new Uint8Array(m.memory.buffer, m.pop_out_ptr(), m.pop_out_len())).toString("utf8");
  return { rc, out };
}

async function viaDevKey(req: VerifyRequest, seedHex: string): Promise<Verified> {
  const m = await loadSim();
  const cfg = Buffer.from(JSON.stringify(gameplayConfig(req.config)));
  const log = Buffer.from(req.log, "base64");
  const rules = call(m, (put) => m.pop_rules_hash(...put(cfg)));
  if (rules.rc !== 0) throw new Error(`Game rules rejected: ${rules.out}`);
  const replay = call(m, (put) => {
    const [cp, cl] = put(cfg);
    const [lp, ll] = put(log);
    return m.pop_replay(cp, cl, req.seed >>> 0, lp, ll);
  });
  if (replay.rc !== 0) throw new Error(`Replay refused: ${replay.out}`);
  const { score, ticks } = JSON.parse(replay.out) as { score: number; ticks: number };
  if (!/^[0-9a-f]{64}$/.test(req.cosmeticsHash)) throw new Error("cosmeticsHash must be 64 hex characters");
  const configHash = sha256(Buffer.from("pop-config-v1"), Buffer.from(rules.out, "hex"), Buffer.from(req.cosmeticsHash, "hex"));
  const logHash = sha256(log);
  const digest = scoreDigest({ ...req, configHash, score, ticks, logHash });
  const key = createPrivateKey({
    key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.from(seedHex, "hex")]),
    format: "der",
    type: "pkcs8",
  });
  return {
    score,
    ticks,
    configHash: hex(configHash),
    logHash: hex(logHash),
    signature: hex(new Uint8Array(edSign(null, digest, key))),
    verifier: "dev",
  };
}

export async function verifyRun(req: VerifyRequest): Promise<Verified> {
  const paymentKey = process.env.OUTLAYER_PAYMENT_KEY;
  if (paymentKey) return viaOutlayer(req, paymentKey);
  const seed = process.env.DEV_VERIFIER_SEED;
  if (seed && /^[0-9a-f]{64}$/.test(seed)) return viaDevKey(req, seed);
  throw new Error("No verifier configured: set OUTLAYER_PAYMENT_KEY (or DEV_VERIFIER_SEED for development)");
}
