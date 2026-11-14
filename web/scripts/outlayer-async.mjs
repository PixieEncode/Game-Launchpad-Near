// Async call to the verifier on OutLayer: submit, then poll /calls/{id}.
//   node scripts/outlayer-async.mjs '{"op":"public_key"}'
import { readFileSync } from "node:fs";
const env = Object.fromEntries(readFileSync(".env.local", "utf8").split(/\r?\n/).filter((l) => /^[A-Z_]+=/.test(l)).map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)]));
const key = env.OUTLAYER_PAYMENT_KEY;
const api = env.OUTLAYER_API_URL || "https://testnet-api.outlayer.ai";
const project = env.OUTLAYER_PROJECT || "popnear.testnet/pop-verifier";
const headers = { "Content-Type": "application/json", "X-Payment-Key": key };
const input = JSON.parse(process.argv[2] || '{"op":"public_key"}');
const res = await fetch(`${api}/call/${project}`, { method: "POST", headers, body: JSON.stringify({ input, async: true }) });
const first = await res.text();
console.log(`submit: HTTP ${res.status} ${first.slice(0, 400)}`);
const id = JSON.parse(first).call_id;
for (let i = 0; i < 40 && id; i++) {
  await new Promise((r) => setTimeout(r, 6000));
  const r = await fetch(`${api}/calls/${id}`, { headers });
  const t = await r.text();
  const j = (() => { try { return JSON.parse(t); } catch { return null; } })();
  const st = j?.status;
  console.log(`poll ${i + 1}: HTTP ${r.status} status=${st}`);
  if (st && !/pending|running|queued|processing/i.test(st)) { console.log(t.slice(0, 1500)); break; }
}
