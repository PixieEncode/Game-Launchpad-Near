//! A small C-style API for the browser and Node (no wasm-bindgen).
//!
//! Strings go in through `pop_alloc` (the caller writes bytes there) and come
//! out through `pop_out_ptr` / `pop_out_len` (JSON). One run at a time.
//!
//!   pop_init(config_ptr, config_len, seed) -> 0 ok, 1 bad config
//!   pop_step(flags, x)                       one tick
//!   pop_score(), pop_over(), pop_tick()
//!   pop_frame()                              JSON {snapshot, events} into the out buffer
//!   pop_rules_hash(config_ptr, config_len)   hex rules hash into the out buffer (0 ok)
//!   pop_replay(config_ptr, config_len, seed, log_ptr, log_len) -> 0 ok; JSON outcome out
//!   pop_log_*                                record an input log while playing

use crate::{replay, Input, Rules, Sim};
use std::cell::RefCell;

thread_local! {
    static SIM: RefCell<Option<Sim>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static LOG: RefCell<Vec<(u32, Input)>> = const { RefCell::new(Vec::new()) };
    static LAST: RefCell<Option<Input>> = const { RefCell::new(None) };
}

fn set_out(bytes: Vec<u8>) {
    OUT.with(|o| *o.borrow_mut() = bytes);
}

unsafe fn read(ptr: *const u8, len: usize) -> &'static [u8] {
    std::slice::from_raw_parts(ptr, len)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Memory for the caller to write `len` bytes into. Leaked on purpose: inputs are small and few.
#[no_mangle]
pub extern "C" fn pop_alloc(len: usize) -> *mut u8 {
    let mut v = vec![0u8; len.max(1)];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[no_mangle]
pub extern "C" fn pop_free(ptr: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(ptr, len.max(1), len.max(1))) }
}

#[no_mangle]
pub extern "C" fn pop_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn pop_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

#[no_mangle]
pub unsafe extern "C" fn pop_init(ptr: *const u8, len: usize, seed: u32) -> u32 {
    let cfg = String::from_utf8_lossy(read(ptr, len)).into_owned();
    match Rules::from_config(&cfg) {
        Ok(rules) => {
            SIM.with(|s| *s.borrow_mut() = Some(Sim::new(rules, seed)));
            LOG.with(|l| l.borrow_mut().clear());
            LAST.with(|l| *l.borrow_mut() = None);
            0
        }
        Err(e) => {
            set_out(e.0.into_bytes());
            1
        }
    }
}

/// One tick. The input is also appended to the run's log when it changed.
#[no_mangle]
pub extern "C" fn pop_step(flags: u32, x: u32) {
    let input = Input { flags: flags as u8, x: x.min(480) as u16 };
    SIM.with(|s| {
        if let Some(sim) = s.borrow_mut().as_mut() {
            if sim.is_over() {
                return;
            }
            let changed = LAST.with(|l| {
                let mut l = l.borrow_mut();
                let c = *l != Some(input);
                *l = Some(input);
                c
            });
            if changed {
                LOG.with(|l| l.borrow_mut().push((sim.tick(), input)));
            }
            sim.step(input);
        }
    });
}

#[no_mangle]
pub extern "C" fn pop_score() -> u32 {
    SIM.with(|s| s.borrow().as_ref().map(|s| s.score()).unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn pop_over() -> u32 {
    SIM.with(|s| s.borrow().as_ref().map(|s| s.is_over() as u32).unwrap_or(1))
}

#[no_mangle]
pub extern "C" fn pop_tick() -> u32 {
    SIM.with(|s| s.borrow().as_ref().map(|s| s.tick()).unwrap_or(0))
}

/// {"snapshot": …, "events": […]} for drawing the current tick.
#[no_mangle]
pub extern "C" fn pop_frame() {
    SIM.with(|s| {
        if let Some(sim) = s.borrow_mut().as_mut() {
            let events = sim.take_events();
            let json = serde_json::json!({ "snapshot": sim.snapshot(), "events": events });
            set_out(json.to_string().into_bytes());
        }
    });
}

/// The run's input log (binary) into the out buffer.
#[no_mangle]
pub extern "C" fn pop_log() {
    LOG.with(|l| set_out(crate::encode_log(&l.borrow())));
}

#[no_mangle]
pub unsafe extern "C" fn pop_rules_hash(ptr: *const u8, len: usize) -> u32 {
    let cfg = String::from_utf8_lossy(read(ptr, len)).into_owned();
    match Rules::from_config(&cfg) {
        Ok(r) => {
            set_out(hex(&r.hash()).into_bytes());
            0
        }
        Err(e) => {
            set_out(e.0.into_bytes());
            1
        }
    }
}

/// Replays a logged run: {"score","ticks","over"} or an error message.
#[no_mangle]
pub unsafe extern "C" fn pop_replay(cptr: *const u8, clen: usize, seed: u32, lptr: *const u8, llen: usize) -> u32 {
    let cfg = String::from_utf8_lossy(read(cptr, clen)).into_owned();
    let rules = match Rules::from_config(&cfg) {
        Ok(r) => r,
        Err(e) => {
            set_out(e.0.into_bytes());
            return 1;
        }
    };
    match replay(&rules, seed, read(lptr, llen)) {
        Ok(o) => {
            set_out(serde_json::to_vec(&o).unwrap());
            0
        }
        Err(e) => {
            set_out(format!("{e:?}").into_bytes());
            2
        }
    }
}
