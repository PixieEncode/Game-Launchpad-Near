//! What the verifier computes, without the signing: replay a run with the game
//! core, hash the rules, config and log, and compose the message to sign.
//!
//! The signed bytes are composed here from parsed, checked fields under the
//! fixed prefix `pop-score-v1`, never taken from the input as they are:
//!
//!   digest = sha256("pop-score-v1" ‖ borsh(game, player, session_id, seed,
//!                                           config_hash, score, ticks, log_hash))
//!
//! with types (String, String, u64, u32, [u8; 32], u32, u32, [u8; 32]). The game
//! contract (near/game/src/score.rs) rebuilds the same digest and checks it.
//!
//! The signature says only "replaying this log on this session's seed, under
//! these rules, gives this score in this many ticks". It names the player, but
//! does not vouch for who asked: the game contract only accepts it from that
//! player, for that open session, within the run's time window.

use base64::Engine;
use pop_sim::{config_hash, replay, sha256, Rules};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PREFIX: &[u8] = b"pop-score-v1";
/// NEAR account ids are at most 64 characters.
const MAX_ACCOUNT_LEN: usize = 64;

#[derive(Debug, Deserialize)]
pub struct VerifyInput {
    /// The game contract's account.
    pub game: String,
    pub player: String,
    pub session_id: u64,
    /// The seed `start_run` returned for this session.
    pub seed: u32,
    /// The game config (at least its gameplay fields and whether it has a collectible).
    pub config: serde_json::Value,
    /// sha256 of the full config's canonical JSON, 64 hex characters (binds the cosmetics).
    pub cosmetics_hash: String,
    /// The input log, standard base64.
    pub log: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Verified {
    pub score: u32,
    pub ticks: u32,
    pub config_hash: String,
    pub log_hash: String,
    #[serde(skip)]
    pub digest: [u8; 32],
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("cosmetics_hash must be 64 hex characters".into());
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    Ok(out)
}

fn account(s: &str, what: &str) -> Result<(), String> {
    let ok = (2..=MAX_ACCOUNT_LEN).contains(&s.len())
        && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_.".contains(&c));
    if ok {
        Ok(())
    } else {
        Err(format!("{what} is not a NEAR account id"))
    }
}

/// The message the game contract checks. Shared by the verifier and its tests.
#[allow(clippy::too_many_arguments)]
pub fn score_digest(
    game: &str,
    player: &str,
    session_id: u64,
    seed: u32,
    config_hash: &[u8; 32],
    score: u32,
    ticks: u32,
    log_hash: &[u8; 32],
) -> [u8; 32] {
    let mut m = Vec::with_capacity(200);
    m.extend_from_slice(PREFIX);
    for s in [game, player] {
        m.extend_from_slice(&(s.len() as u32).to_le_bytes());
        m.extend_from_slice(s.as_bytes());
    }
    m.extend_from_slice(&session_id.to_le_bytes());
    m.extend_from_slice(&seed.to_le_bytes());
    m.extend_from_slice(config_hash);
    m.extend_from_slice(&score.to_le_bytes());
    m.extend_from_slice(&ticks.to_le_bytes());
    m.extend_from_slice(log_hash);
    Sha256::digest(&m).into()
}

/// Replays the run and builds the digest to sign.
pub fn verify(input: &VerifyInput) -> Result<Verified, String> {
    account(&input.game, "game")?;
    account(&input.player, "player")?;
    let rules = Rules::from_config(&input.config.to_string()).map_err(|e| e.0)?;
    let cosmetics = unhex32(&input.cosmetics_hash)?;
    let log = base64::engine::general_purpose::STANDARD
        .decode(input.log.trim())
        .map_err(|e| format!("log is not base64: {e}"))?;
    let outcome = replay(&rules, input.seed, &log).map_err(|e| format!("replay refused: {e:?}"))?;
    let cfg_hash = config_hash(&rules.hash(), &cosmetics);
    let log_hash = sha256(&log);
    let digest = score_digest(
        &input.game,
        &input.player,
        input.session_id,
        input.seed,
        &cfg_hash,
        outcome.score,
        outcome.ticks,
        &log_hash,
    );
    Ok(Verified {
        score: outcome.score,
        ticks: outcome.ticks,
        config_hash: hex(&cfg_hash),
        log_hash: hex(&log_hash),
        digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pop_sim::{encode_log, Input, Sim};

    fn config() -> serde_json::Value {
        serde_json::json!({
            "version": 1, "template": "flappy", "sprites": {"collectible": "x"},
            "physics": {"gravity": 1400, "jumpVelocity": 470, "scrollSpeed": 170, "speedRampPerSec": 4},
            "obstacles": {"spawnIntervalMs": 1550, "gapSize": 195, "variance": 0.6},
            "scoring": {"pointsPerObstacle": 1, "collectibleChance": 0.45, "pointsPerCollectible": 3}
        })
    }

    /// Flaps whenever it drops below y = 330; returns (log, score, ticks).
    fn play(seed: u32) -> (Vec<u8>, u32, u32) {
        let rules = Rules::from_config(&config().to_string()).unwrap();
        let mut sim = Sim::new(rules, seed);
        let mut log = vec![];
        let mut last = None;
        while !sim.is_over() {
            let p = sim.snapshot().player;
            let inp = if sim.tick() == 0 || (p.y > 330.0 && p.v > -40.0) {
                Input { flags: Input::TAP | Input::DOWN, x: 240 }
            } else {
                Input::default()
            };
            if last != Some(inp) {
                log.push((sim.tick(), inp));
                last = Some(inp);
            }
            sim.step(inp);
        }
        (encode_log(&log), sim.score(), sim.tick())
    }

    fn input(seed: u32, log: &[u8]) -> VerifyInput {
        VerifyInput {
            game: "frog-3.factory.popnear.testnet".into(),
            player: "popnear-player.testnet".into(),
            session_id: 7,
            seed,
            config: config(),
            cosmetics_hash: "ab".repeat(32),
            log: base64::engine::general_purpose::STANDARD.encode(log),
        }
    }

    #[test]
    fn verifies_an_honest_run() {
        let (log, score, ticks) = play(5);
        let v = verify(&input(5, &log)).unwrap();
        assert_eq!((v.score, v.ticks), (score, ticks));
        assert_eq!(v.log_hash, hex(&sha256(&log)));
        // The output fits OutLayer's 900-byte limit with a 128-hex signature.
        let out = serde_json::json!({"ok": true, "score": v.score, "ticks": v.ticks, "config_hash": v.config_hash,
            "log_hash": v.log_hash, "signature": "00".repeat(64)});
        assert!(out.to_string().len() < 900);
    }

    #[test]
    fn the_digest_binds_every_field() {
        let (log, ..) = play(5);
        let base = verify(&input(5, &log)).unwrap().digest;
        let mut i = input(5, &log);
        i.session_id = 8;
        assert_ne!(verify(&i).unwrap().digest, base);
        let mut i = input(5, &log);
        i.player = "someone-else.testnet".into();
        assert_ne!(verify(&i).unwrap().digest, base);
        let mut i = input(5, &log);
        i.game = "frog-4.factory.popnear.testnet".into();
        assert_ne!(verify(&i).unwrap().digest, base);
        let mut i = input(5, &log);
        i.cosmetics_hash = "cd".repeat(32);
        assert_ne!(verify(&i).unwrap().digest, base);
    }

    #[test]
    fn refuses_bad_input() {
        let (log, ..) = play(5);
        let mut i = input(5, &log);
        i.log = "not base64!".into();
        assert!(verify(&i).is_err());
        let mut i = input(5, &log);
        i.player = "Bad Account".into();
        assert!(verify(&i).is_err());
        let mut i = input(5, &log);
        i.cosmetics_hash = "zz".into();
        assert!(verify(&i).is_err());
        let mut i = input(5, &log);
        i.config = serde_json::json!({"template": "flappy"});
        assert!(verify(&i).is_err());
    }

    #[test]
    fn a_log_from_another_seed_does_not_replay() {
        let (log, score, ticks) = play(5);
        match verify(&input(6, &log)) {
            Err(e) => assert!(e.contains("replay refused")),
            Ok(v) => assert_ne!((v.score, v.ticks), (score, ticks)),
        }
    }

    #[test]
    fn signatures_check_against_the_digest() {
        use ed25519_dalek::{Signer, SigningKey, Verifier};
        let (log, ..) = play(5);
        let v = verify(&input(5, &log)).unwrap();
        let key = SigningKey::from_bytes(&[3; 32]);
        let sig = key.sign(&v.digest);
        assert!(key.verifying_key().verify(&v.digest, &sig).is_ok());
    }
}

#[cfg(test)]
mod vector {
    /// The same inputs and answer are pinned in near/game/src/tests.rs, so the
    /// contract and the verifier always compose the same message.
    #[test]
    fn known_answer_digest() {
        let d = super::score_digest(
            "frog-1.factory.popnear.testnet",
            "popnear-player.testnet",
            42,
            2_207_280_663,
            &[0x11; 32],
            140,
            6_000,
            &[0x22; 32],
        );
        assert_eq!(super::hex(&d), KNOWN);
    }
    const KNOWN: &str = "3d89f033654de083647ec7f057f5cf00de18edda28631af83edd092be88880f4";
}
