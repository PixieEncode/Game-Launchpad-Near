//! The input log: the ticks on which the input changed, and what it changed to.
//!
//! Format (version 1): byte 0x01, then per entry
//!   varint(tick − previous tick), flags (u8), zigzag varint(x − previous x).
//! The first entry's deltas are taken from tick 0 and x 0. Ticks strictly
//! increase, x is at most 480, and a log holds at most `MAX_ENTRIES` entries.
//! The log hash signed on chain is sha256 of these bytes.

use crate::{Input, MAX_TICKS};

pub const VERSION: u8 = 1;
pub const MAX_ENTRIES: usize = 200_000;

#[derive(Debug, PartialEq, Eq)]
pub enum LogError {
    Empty,
    BadVersion,
    Truncated,
    TickOrder,
    TickTooLate,
    BadX,
    TooLong,
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn get_varint(bytes: &[u8], i: &mut usize) -> Result<u64, LogError> {
    let mut v: u64 = 0;
    for shift in (0..64).step_by(7) {
        let b = *bytes.get(*i).ok_or(LogError::Truncated)?;
        *i += 1;
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(LogError::Truncated)
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

pub fn encode_log(entries: &[(u32, Input)]) -> Vec<u8> {
    let mut out = vec![VERSION];
    let (mut t0, mut x0) = (0u32, 0i64);
    for (i, (t, inp)) in entries.iter().enumerate() {
        let dt = if i == 0 { *t } else { t - t0 };
        put_varint(&mut out, dt as u64);
        out.push(inp.flags);
        put_varint(&mut out, zigzag(inp.x as i64 - x0));
        t0 = *t;
        x0 = inp.x as i64;
    }
    out
}

pub fn decode_log(bytes: &[u8]) -> Result<Vec<(u32, Input)>, LogError> {
    if bytes.is_empty() {
        return Err(LogError::Empty);
    }
    if bytes[0] != VERSION {
        return Err(LogError::BadVersion);
    }
    let mut i = 1;
    let mut out: Vec<(u32, Input)> = Vec::new();
    let (mut t, mut x) = (0u64, 0i64);
    while i < bytes.len() {
        if out.len() >= MAX_ENTRIES {
            return Err(LogError::TooLong);
        }
        let dt = get_varint(bytes, &mut i)?;
        if !out.is_empty() && dt == 0 {
            return Err(LogError::TickOrder);
        }
        t += dt;
        if t >= MAX_TICKS as u64 {
            return Err(LogError::TickTooLate);
        }
        let flags = *bytes.get(i).ok_or(LogError::Truncated)?;
        i += 1;
        x += unzigzag(get_varint(bytes, &mut i)?);
        if !(0..=480).contains(&x) {
            return Err(LogError::BadX);
        }
        out.push((t as u32, Input { flags, x: x as u16 }));
    }
    if out.is_empty() {
        return Err(LogError::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let entries = vec![
            (0, Input { flags: Input::TAP | Input::DOWN, x: 240 }),
            (1, Input { flags: Input::DOWN, x: 240 }),
            (30, Input { flags: Input::DOWN, x: 10 }),
            (700, Input { flags: 0, x: 480 }),
        ];
        let bytes = encode_log(&entries);
        assert!(bytes.len() < 20, "{} bytes", bytes.len());
        assert_eq!(decode_log(&bytes).unwrap(), entries);
    }

    #[test]
    fn refuses_malformed_logs() {
        assert_eq!(decode_log(&[]), Err(LogError::Empty));
        assert_eq!(decode_log(&[2, 0, 0, 0]), Err(LogError::BadVersion));
        assert_eq!(decode_log(&[1]), Err(LogError::Empty));
        assert_eq!(decode_log(&[1, 0, 0]), Err(LogError::Truncated));
        // Two entries on the same tick.
        assert_eq!(decode_log(&[1, 0, 0, 0, 0, 0, 0]), Err(LogError::TickOrder));
        // x beyond 480.
        let bad = encode_log(&[(0, Input { flags: 0, x: 481 })]);
        assert_eq!(decode_log(&bad), Err(LogError::BadX));
        let late = encode_log(&[(MAX_TICKS, Input::default())]);
        assert_eq!(decode_log(&late), Err(LogError::TickTooLate));
    }
}
