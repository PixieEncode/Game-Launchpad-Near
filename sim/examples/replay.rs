//! Replays recorded runs natively and checks each against the score the
//! recorder (e.g. the WASM build in a browser or Node) reported.
//!
//! cargo run --release --example replay -- runs.json
//! runs.json: [{"config": {...}, "seed": 1, "log_hex": "01...", "score": 3, "ticks": 368}, ...]

use pop_sim::{replay, Rules};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: replay <runs.json>");
    let runs: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut bad = 0;
    for r in &runs {
        let rules = Rules::from_config(&r["config"].to_string()).unwrap();
        let seed = r["seed"].as_u64().unwrap() as u32;
        let out = replay(&rules, seed, &unhex(r["log_hex"].as_str().unwrap())).unwrap();
        let (want_s, want_t) = (r["score"].as_u64().unwrap() as u32, r["ticks"].as_u64().unwrap() as u32);
        let ok = out.score == want_s && out.ticks == want_t;
        if !ok {
            bad += 1;
        }
        println!(
            "{:8} seed {:>6}: recorded {:>4} pts / {:>6} ticks, native replay {:>4} / {:>6}  {}",
            r["config"]["template"].as_str().unwrap(),
            seed,
            want_s,
            want_t,
            out.score,
            out.ticks,
            if ok { "MATCH" } else { "MISMATCH" }
        );
    }
    println!("{} runs, {} mismatches", runs.len(), bad);
    std::process::exit(if bad == 0 { 0 } else { 1 });
}
