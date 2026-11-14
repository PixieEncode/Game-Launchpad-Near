//! The message the score verifier signs, and hex helpers.
//!
//! digest = sha256("pop-score-v1" ‖ borsh((game, player, session_id, seed, config_hash, score, ticks, log_hash)))
//! with types (String, String, u64, u32, [u8; 32], u32, u32, [u8; 32]). Borsh writes
//! a string as a u32 little-endian length then its bytes, integers as
//! little-endian, and fixed arrays as raw bytes. The verifier signs the 32-byte
//! digest with ed25519; `end_run` checks it with `env::ed25519_verify`.
//!
//! Binding the game account, player, session and seed stops a signature being
//! reused anywhere else; binding the config hash and log hash ties the score to
//! one game's rules and one exact input log. `ticks` is the run's length at 60
//! ticks per second; `end_run` checks it against the time since `start_run`.

use near_sdk::{borsh, env, AccountId};

/// Fixed prefix of every signed score message (verifier/src/lib.rs composes the same).
pub const PREFIX: &[u8] = b"pop-score-v1";

pub fn score_message(
    game: &AccountId,
    player: &AccountId,
    session_id: u64,
    seed: u32,
    config_hash: &[u8; 32],
    score: u32,
    ticks: u32,
    log_hash: &[u8; 32],
) -> Vec<u8> {
    borsh::to_vec(&(
        game.as_str(),
        player.as_str(),
        session_id,
        seed,
        *config_hash,
        score,
        ticks,
        *log_hash,
    ))
    .unwrap()
}

pub fn score_digest(
    game: &AccountId,
    player: &AccountId,
    session_id: u64,
    seed: u32,
    config_hash: &[u8; 32],
    score: u32,
    ticks: u32,
    log_hash: &[u8; 32],
) -> [u8; 32] {
    let mut m = PREFIX.to_vec();
    m.extend(score_message(
        game,
        player,
        session_id,
        seed,
        config_hash,
        score,
        ticks,
        log_hash,
    ));
    let h = env::sha256(m);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => env::panic_str("Invalid hex"),
    }
}

fn decode<const N: usize>(s: &str) -> [u8; N] {
    let s = s.strip_prefix("0x").unwrap_or(s).as_bytes();
    if s.len() != N * 2 {
        env::panic_str(&format!("Expected {} hex characters", N * 2));
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = nibble(s[2 * i]) << 4 | nibble(s[2 * i + 1]);
    }
    out
}

/// 64 hex characters (a SHA-256 hash) to bytes.
pub fn hex32(s: &str) -> [u8; 32] {
    decode::<32>(s)
}

/// 128 hex characters (an ed25519 signature) to bytes.
pub fn hex64(s: &str) -> [u8; 64] {
    decode::<64>(s)
}

pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}
