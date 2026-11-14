//! The OutLayer program (wasm32-wasip2). JSON in on stdin, JSON out on stdout
//! (at most 900 bytes):
//!
//!   {"op":"verify", game, player, session_id, seed, config, cosmetics_hash, log}
//!     -> {"ok":true, score, ticks, config_hash, log_hash, signature}
//!   {"op":"public_key"}
//!     -> {"ok":true, "public_key": <64 hex>}
//!
//! The signing key never enters this module: it is declared in the
//! `outlayer.manifest` section (manifest.json: one ed25519 key, `caller: "none"`,
//! bound to the project), derived inside the TEE, and used by the host through
//! the `outlayer:signing-keys` interface. Built natively (for tests) the program
//! has no key and refuses to sign.

use pop_verifier::{verify, VerifyInput};
use serde_json::{json, Value};
use std::io::Read;

#[cfg(target_arch = "wasm32")]
#[used]
#[link_section = "outlayer.manifest"]
static OUTLAYER_MANIFEST: [u8; include_bytes!("../manifest.json").len()] = *include_bytes!("../manifest.json");

#[cfg(target_arch = "wasm32")]
mod host {
    wit_bindgen::generate!({ world: "signing-keys-host", path: "wit" });
    use self::outlayer::signing_keys::api;

    const KEY: &str = "score";

    pub fn public_key() -> Result<Vec<u8>, String> {
        api::public_key(KEY, None)
    }
    pub fn sign(digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        api::sign(KEY, None, digest)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub fn public_key() -> Result<Vec<u8>, String> {
        Err("no signing key outside OutLayer".into())
    }
    pub fn sign(_: &[u8; 32]) -> Result<Vec<u8>, String> {
        Err("no signing key outside OutLayer".into())
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn err(msg: impl Into<String>) -> Value {
    let mut m: String = msg.into();
    m.truncate(400);
    json!({ "ok": false, "error": m })
}

fn run(raw: &str) -> Value {
    let v: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => return err(format!("input is not JSON: {e}")),
    };
    match v.get("op").and_then(|o| o.as_str()) {
        Some("public_key") => match host::public_key() {
            Ok(pk) => json!({ "ok": true, "public_key": hex(&pk) }),
            Err(e) => err(e),
        },
        Some("verify") => {
            let input: VerifyInput = match serde_json::from_value(v) {
                Ok(i) => i,
                Err(e) => return err(format!("bad verify input: {e}")),
            };
            let out = match verify(&input) {
                Ok(o) => o,
                Err(e) => return err(e),
            };
            match host::sign(&out.digest) {
                Ok(sig) => json!({
                    "ok": true,
                    "score": out.score,
                    "ticks": out.ticks,
                    "config_hash": out.config_hash,
                    "log_hash": out.log_hash,
                    "signature": hex(&sig),
                }),
                Err(e) => err(e),
            }
        }
        _ => err("op must be \"verify\" or \"public_key\""),
    }
}

fn main() {
    let mut raw = String::new();
    let answer = match std::io::stdin().read_to_string(&mut raw) {
        Ok(_) => run(&raw),
        Err(e) => err(format!("cannot read stdin: {e}")),
    };
    print!("{answer}");
}
