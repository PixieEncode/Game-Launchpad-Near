// Calls the verifier on OutLayer over HTTPS with the payment key in .env.local.
//   node scripts/outlayer-call.mjs '{"op":"public_key"}'
// Prints OutLayer's response (never the payment key).
import { readFileSync } from "node:fs";

const env = Object.fromEntries(
  readFileSync(".env.local", "utf8")
    .split(/\r?\n/)
    .filter((l) => /^[A-Z_]+=/.test(l))
    .map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)]),
);
const key = process.env.OUTLAYER_PAYMENT_KEY || env.OUTLAYER_PAYMENT_KEY;
if (!key) throw new Error("OUTLAYER_PAYMENT_KEY is not set");
const api = env.OUTLAYER_API_URL || "https://testnet-api.outlayer.ai";
const project = env.OUTLAYER_PROJECT || "popnear.testnet/pop-verifier";
const input = JSON.parse(process.argv[2] || '{"op":"public_key"}');

const t0 = Date.now();
const res = await fetch(`${api}/call/${project}`, {
  method: "POST",
  headers: { "Content-Type": "application/json", "X-Payment-Key": key },
  body: JSON.stringify({ input }),
});
const text = await res.text();
console.log(`HTTP ${res.status} in ${Date.now() - t0} ms`);
console.log(text.length > 3000 ? text.slice(0, 3000) + "…" : text);
