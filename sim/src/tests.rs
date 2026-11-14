//! Determinism and replay tests. Simple bots play each template from the
//! snapshot; every run is recorded and must replay to the identical outcome.

use super::*;

pub fn config(template: &str) -> String {
    let (physics, obstacles, scoring, tuning) = match template {
        "flappy" => (
            r#"{"gravity":1400,"jumpVelocity":470,"scrollSpeed":170,"speedRampPerSec":4}"#,
            r#"{"spawnIntervalMs":1550,"gapSize":195,"variance":0.6}"#,
            r#"{"pointsPerObstacle":1,"collectibleChance":0.45,"pointsPerCollectible":3}"#,
            "null",
        ),
        "runner" => (
            r#"{"gravity":2000,"jumpVelocity":820,"scrollSpeed":270,"speedRampPerSec":8}"#,
            r#"{"spawnIntervalMs":1350,"gapSize":160,"variance":0.55}"#,
            r#"{"pointsPerObstacle":1,"collectibleChance":0.5,"pointsPerCollectible":2}"#,
            "null",
        ),
        "shooter" => (
            r#"{"gravity":1000,"jumpVelocity":500,"scrollSpeed":170,"speedRampPerSec":5}"#,
            r#"{"spawnIntervalMs":800,"gapSize":160,"variance":0.6}"#,
            r#"{"pointsPerObstacle":1,"collectibleChance":0.18,"pointsPerCollectible":5}"#,
            r#"{"playerSpeed":460,"fireRateMs":240,"enemyHp":2}"#,
        ),
        _ => (
            r#"{"gravity":1000,"jumpVelocity":500,"scrollSpeed":230,"speedRampPerSec":7}"#,
            r#"{"spawnIntervalMs":620,"gapSize":160,"variance":0.65}"#,
            r#"{"pointsPerObstacle":1,"collectibleChance":0.6,"pointsPerCollectible":3}"#,
            "null",
        ),
    };
    format!(
        r##"{{"version":1,"template":"{template}","title":"Test","tagline":"","scoreLabel":"pts",
        "palette":{{"sky":"#000000","skyBottom":"#000000","ground":"#111111","accent":"#ff0000","text":"#ffffff"}},
        "sprites":{{"player":"<svg></svg>","obstacle":"<svg></svg>","collectible":"<svg></svg>"}},
        "physics":{physics},"obstacles":{obstacles},"scoring":{scoring},"tuning":{tuning}}}"##
    )
}

fn rules(template: &str) -> Rules {
    Rules::from_config(&config(template)).unwrap()
}

/// A simple bot per template; returns this tick's input.
fn bot(sim: &Sim, tick: u32) -> Input {
    let s = sim.snapshot();
    let p = &s.player;
    let mut flags = 0u8;
    let mut x = p.x as u16;
    match sim.rules().template {
        Template::Flappy => {
            // Aim for the middle of the next gap; flap when below it and falling.
            let target = s
                .ents
                .iter()
                .filter(|e| e.k == "pipe" && e.x + e.w / 2.0 > p.x - 30.0)
                .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
                .map(|e| (e.a + e.b) / 2.0 + 18.0)
                .unwrap_or(330.0);
            if p.y > target && p.v > -40.0 {
                flags |= Input::TAP | Input::DOWN;
            }
        }
        Template::Runner => {
            let close = s
                .ents
                .iter()
                .any(|e| e.k == "obstacle" && e.x - p.x > 0.0 && e.x - p.x < 30.0 + s.speed * 0.2);
            if close && p.grounded {
                flags |= Input::TAP | Input::DOWN;
            }
        }
        Template::Shooter | Template::Dodge => {
            // Drag away from the nearest falling threat, else towards the centre.
            let threat = s
                .ents
                .iter()
                .filter(|e| matches!(e.k, "hazard" | "enemy" | "elite") && e.y > 250.0 && e.a <= 0.0 || (e.k == "enemy" || e.k == "elite") && e.y > 300.0)
                .min_by(|a, b| (a.x - p.x).abs().partial_cmp(&(b.x - p.x).abs()).unwrap());
            let goal = match threat {
                Some(e) if (e.x - p.x).abs() < 90.0 => {
                    if e.x > p.x || p.x > 420.0 {
                        (p.x - 120.0).max(30.0)
                    } else {
                        (p.x + 120.0).min(450.0)
                    }
                }
                _ => 240.0 + 120.0 * ((tick / 90) % 3) as f64 - 120.0,
            };
            flags |= Input::DOWN;
            x = goal.clamp(0.0, 480.0) as u16;
            if tick == 0 {
                flags |= Input::TAP;
            }
        }
    }
    Input { flags, x }
}

/// Plays a run with the bot, recording the input changes. Returns (log, score, ticks).
fn play(template: &str, seed: u32, max_ticks: u32) -> (Vec<u8>, u32, u32) {
    let mut sim = Sim::new(rules(template), seed);
    let mut log: Vec<(u32, Input)> = vec![];
    let mut last = None;
    while !sim.is_over() && sim.tick() < max_ticks {
        let inp = bot(&sim, sim.tick());
        if last != Some(inp) {
            log.push((sim.tick(), inp));
            last = Some(inp);
        }
        sim.step(inp);
    }
    (encode_log(&log), sim.score(), sim.tick())
}

#[test]
fn every_template_replays_to_the_same_score() {
    for t in ["flappy", "runner", "shooter", "dodge"] {
        for seed in [1, 42, 0xdead_beef] {
            let (log, score, ticks) = play(t, seed, MAX_TICKS);
            let out = replay(&rules(t), seed, &log).unwrap();
            assert_eq!((out.score, out.ticks), (score, ticks), "{t} seed {seed}");
            assert!(out.over);
            println!("{t:8} seed {seed:>10}: score {score:>4} in {ticks:>6} ticks, log {} bytes", log.len());
        }
    }
}

#[test]
fn the_bots_score_points() {
    for t in ["flappy", "runner", "shooter", "dodge"] {
        let (_, score, ticks) = play(t, 7, MAX_TICKS);
        assert!(score > 0, "{t}: the bot scored nothing in {ticks} ticks");
    }
}

#[test]
fn identical_inputs_give_identical_snapshots() {
    for t in ["flappy", "runner", "shooter", "dodge"] {
        let mut a = Sim::new(rules(t), 99);
        let mut b = Sim::new(rules(t), 99);
        for tick in 0..600 {
            let inp = bot(&a, tick);
            a.step(inp);
            b.step(inp);
            assert_eq!(a.snapshot(), b.snapshot(), "{t} tick {tick}");
        }
    }
}

#[test]
fn a_log_replayed_on_another_seed_does_not_reproduce_the_run() {
    // A run's log only fits its own seed: on another seed the obstacles differ, so the
    // replay either dies before the log ends (refused) or ends with another result.
    for t in ["flappy", "runner", "shooter", "dodge"] {
        let (log, score, ticks) = play(t, 1, MAX_TICKS);
        match replay(&rules(t), 2, &log) {
            Err(ReplayError::InputAfterEnd { .. }) => {}
            Ok(o) => assert_ne!((o.score, o.ticks), (score, ticks), "{t}"),
            Err(e) => panic!("{t}: {e:?}"),
        }
    }
}

#[test]
fn an_edited_log_changes_the_run() {
    let (log, score, ticks) = play("runner", 5, MAX_TICKS);
    let mut entries = decode_log(&log).unwrap();
    // Drop the first jump after the start: the run plays out differently.
    entries.remove(2);
    let out = replay(&rules("runner"), 5, &encode_log(&entries));
    match out {
        Ok(o) => assert_ne!((o.score, o.ticks), (score, ticks)),
        Err(e) => assert!(matches!(e, ReplayError::InputAfterEnd { .. })),
    }
}

#[test]
fn input_after_the_run_ended_is_refused() {
    let (log, _, ticks) = play("flappy", 3, MAX_TICKS);
    let mut entries = decode_log(&log).unwrap();
    entries.push((ticks + 100, Input { flags: Input::TAP, x: 0 }));
    let out = replay(&rules("flappy"), 3, &encode_log(&entries));
    assert_eq!(out, Err(ReplayError::InputAfterEnd { tick: ticks + 100 }));
}

#[test]
fn doing_nothing_ends_a_flappy_run_with_no_score() {
    let log = encode_log(&[(0, Input { flags: Input::TAP, x: 0 }), (1, Input::default())]);
    let out = replay(&rules("flappy"), 1, &log).unwrap();
    assert_eq!(out.score, 0);
    assert!(out.ticks < 120);
}

#[test]
fn rules_read_the_gameplay_fields() {
    let r = rules("shooter");
    assert_eq!(r.template, Template::Shooter);
    assert_eq!(r.enemy_hp, 2);
    assert_eq!(r.fire_rate, fixed::from_f64(0.24));
    assert!(r.has_collectible);
    // Missing tuning falls back to the template defaults.
    let d = rules("dodge");
    assert_eq!((d.player_speed, d.enemy_hp), (fx(420), 1));
    // Out-of-range or missing numbers are refused.
    let bad = config("flappy").replace("\"gravity\":1400", "\"gravity\":99999");
    assert!(Rules::from_config(&bad).is_err());
    assert!(Rules::from_config("{}").is_err());
}

#[test]
fn the_rules_hash_covers_gameplay_not_cosmetics() {
    let base = rules("flappy").hash();
    let recoloured = config("flappy").replace("#ff0000", "#00ff00").replace("\"Test\"", "\"Other\"");
    assert_eq!(Rules::from_config(&recoloured).unwrap().hash(), base);
    let harder = config("flappy").replace("\"gapSize\":195", "\"gapSize\":150");
    assert_ne!(Rules::from_config(&harder).unwrap().hash(), base);
    let no_items = config("flappy").replace(",\"collectible\":\"<svg></svg>\"", "");
    assert_ne!(Rules::from_config(&no_items).unwrap().hash(), base);
    // The on-chain hash binds the rules and the cosmetics.
    assert_ne!(config_hash(&base, &[0; 32]), config_hash(&base, &[1; 32]));
}
