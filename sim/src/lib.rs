//! Proof-of-Play deterministic game core.
//!
//! The four templates (flappy, runner, shooter, dodge) with everything that
//! changes the score: physics, spawning, collisions, collectibles and the
//! near-miss bonus. Integer maths only, a fixed 60 ticks per second, and the
//! tick count is the only clock. The browser draws from `snapshot()`; the
//! verifier calls `replay()` on the player's input log and signs the score.
//!
//! Visual effects (hit-stop, slow-mo, shakes, tweens) are not here: they only
//! change how fast ticks are shown, never how many happen.

pub mod fixed;
pub mod log;
pub mod rng;
mod templates;
// The C-style API for the browser and Node; not in WASI builds (the verifier).
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm;

use fixed::*;
use rng::Rng;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub use log::{decode_log, encode_log, LogError};

pub const W: Fx = fx(480);
pub const H: Fx = fx(720);
pub const TICKS_PER_SEC: u32 = 60;
/// A run is capped at 10 minutes of ticks.
pub const MAX_TICKS: u32 = 10 * 60 * TICKS_PER_SEC;
const NEAR_MISS_COOLDOWN_TICKS: u32 = 42; // 0.7 s, as in core.js
const MAX_SPEED: Fx = fx(1200);

// ------------------------------------------------------------------- rules

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    Flappy,
    Runner,
    Shooter,
    Dodge,
}

/// Everything in a game config that can change a score, as fixed point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rules {
    pub template: Template,
    pub gravity: Fx,
    pub jump_velocity: Fx,
    pub scroll_speed: Fx,
    pub speed_ramp: Fx,
    pub spawn_interval: Fx, // seconds
    pub gap_size: Fx,
    pub variance: Fx,
    pub points_per_obstacle: u32,
    pub collectible_chance: Fx,
    pub points_per_collectible: u32,
    pub player_speed: Fx,
    pub fire_rate: Fx, // seconds
    pub enemy_hp: u32,
    pub has_collectible: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RulesError(pub String);

impl Rules {
    /// Reads the gameplay fields from a full game config (the JSON the web app stores).
    pub fn from_config(json: &str) -> Result<Rules, RulesError> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| RulesError(format!("config is not JSON: {e}")))?;
        let num = |path: &[&str], lo: f64, hi: f64| -> Result<f64, RulesError> {
            let mut cur = &v;
            for p in path {
                cur = cur.get(p).ok_or_else(|| RulesError(format!("missing {}", path.join("."))))?;
            }
            let n = cur
                .as_f64()
                .ok_or_else(|| RulesError(format!("{} must be a number", path.join("."))))?;
            if !(lo..=hi).contains(&n) {
                return Err(RulesError(format!("{} must be in {lo}..={hi}", path.join("."))));
            }
            Ok(n)
        };
        let opt = |path: &[&str], lo: f64, hi: f64, default: f64| -> Result<f64, RulesError> {
            let mut cur = &v;
            for p in path {
                match cur.get(p) {
                    Some(x) => cur = x,
                    None => return Ok(default),
                }
            }
            if cur.is_null() {
                return Ok(default);
            }
            num(path, lo, hi)
        };
        let template = match v.get("template").and_then(|t| t.as_str()) {
            Some("flappy") => Template::Flappy,
            Some("runner") => Template::Runner,
            Some("shooter") => Template::Shooter,
            Some("dodge") => Template::Dodge,
            _ => return Err(RulesError("template must be flappy, runner, shooter or dodge".into())),
        };
        let has_collectible = v
            .get("sprites")
            .and_then(|s| s.get("collectible"))
            .map(|c| c.is_string())
            .unwrap_or(false);
        Ok(Rules {
            template,
            gravity: from_f64(num(&["physics", "gravity"], 300.0, 2500.0)?),
            jump_velocity: from_f64(num(&["physics", "jumpVelocity"], 250.0, 1000.0)?),
            scroll_speed: from_f64(num(&["physics", "scrollSpeed"], 120.0, 600.0)?),
            speed_ramp: from_f64(num(&["physics", "speedRampPerSec"], 0.0, 15.0)?),
            spawn_interval: from_f64(num(&["obstacles", "spawnIntervalMs"], 300.0, 2600.0)? / 1000.0),
            gap_size: from_f64(num(&["obstacles", "gapSize"], 110.0, 280.0)?),
            variance: from_f64(num(&["obstacles", "variance"], 0.0, 1.0)?),
            points_per_obstacle: num(&["scoring", "pointsPerObstacle"], 1.0, 10.0)?.round() as u32,
            collectible_chance: from_f64(num(&["scoring", "collectibleChance"], 0.0, 1.0)?),
            points_per_collectible: num(&["scoring", "pointsPerCollectible"], 0.0, 25.0)?.round() as u32,
            player_speed: from_f64(opt(&["tuning", "playerSpeed"], 200.0, 900.0, 420.0)?),
            fire_rate: from_f64(opt(&["tuning", "fireRateMs"], 120.0, 800.0, 280.0)? / 1000.0),
            enemy_hp: opt(&["tuning", "enemyHp"], 1.0, 5.0, 1.0)?.round() as u32,
            has_collectible,
        })
    }

    /// Canonical text of the rules: what the rules hash covers.
    pub fn canonical(&self) -> String {
        format!(
            "pop-rules-v1|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.template,
            self.gravity,
            self.jump_velocity,
            self.scroll_speed,
            self.speed_ramp,
            self.spawn_interval,
            self.gap_size,
            self.variance,
            self.points_per_obstacle,
            self.collectible_chance,
            self.points_per_collectible,
            self.player_speed,
            self.fire_rate,
            self.enemy_hp,
            self.has_collectible as u8
        )
    }

    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.canonical().as_bytes()).into()
    }
}

/// The hash a game stores on chain: sha256("pop-config-v1" ‖ rules hash ‖ cosmetics hash).
/// The cosmetics hash covers the rest of the config (sprites, palette, sounds) and is
/// computed by the web app, so the verifier only needs the small rules.
pub fn config_hash(rules_hash: &[u8; 32], cosmetics_hash: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"pop-config-v1");
    h.update(rules_hash);
    h.update(cosmetics_hash);
    h.finalize().into()
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

// ------------------------------------------------------------------- input

/// One tick's input. `x` is the pointer's x in logical pixels (0..480).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub flags: u8,
    pub x: u16,
}

impl Input {
    pub const DOWN: u8 = 1; // pointer held
    pub const LEFT: u8 = 2;
    pub const RIGHT: u8 = 4;
    pub const UP: u8 = 8;
    pub const ACTION: u8 = 16; // space
    /// A tap / jump press this tick (pointer down, Space, Up or W).
    pub const TAP: u8 = 32;
    /// The press that started the run was Left or Right: it starts the run but is not a tap.
    pub const MOVE_START: u8 = 64;

    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }
    fn pointer_x(&self) -> Fx {
        fx(self.x as i64)
    }
    /// Held jump (runner): pointer, Up or Space.
    fn held(&self) -> bool {
        self.has(Self::DOWN) || self.has(Self::UP) || self.has(Self::ACTION)
    }
}

// ------------------------------------------------------------------ events

/// What happened this tick, for the renderer's sounds and effects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "e", rename_all = "snake_case")]
pub enum Event {
    Score { n: u32, x: i32, y: i32 },
    Collect { n: u32, x: i32, y: i32 },
    NearMiss { x: i32, y: i32 },
    Jump,
    Land { impact: i32 },
    Shoot { x: i32, y: i32 },
    Hit { id: u32, x: i32, y: i32 },
    Kill { id: u32, x: i32, y: i32, elite: bool },
    HazardLand { id: u32, x: i32 },
    Wave { kind: &'static str },
    GameOver { x: i32, y: i32, timeout: bool },
}

fn px(v: Fx) -> i32 {
    (v >> 16) as i32
}

// -------------------------------------------------------------------- core

/// Shared run state the templates read and write through.
pub struct Core {
    pub rules: Rules,
    pub rng: Rng,
    pub tick: u32,
    pub score: u32,
    pub over: bool,
    pub input: Input,
    events: Vec<Event>,
    last_near_tick: Option<u32>,
    next_id: u32,
}

impl Core {
    /// Seconds since the run started.
    pub fn elapsed(&self) -> Fx {
        self.tick as Fx * ONE / TICKS_PER_SEC as Fx
    }

    pub fn speed(&self) -> Fx {
        (self.rules.scroll_speed + mul(self.rules.speed_ramp, self.elapsed())).min(MAX_SPEED)
    }

    pub fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn add_score(&mut self, n: u32, x: Fx, y: Fx) {
        if self.over || n == 0 {
            return;
        }
        self.score += n;
        self.events.push(Event::Score { n, x: px(x), y: px(y) });
    }

    pub fn collect(&mut self, x: Fx, y: Fx) {
        if self.over {
            return;
        }
        let n = self.rules.points_per_collectible;
        self.score += n;
        self.events.push(Event::Collect { n, x: px(x), y: px(y) });
    }

    /// +1 bonus, at most once per 0.7 s.
    pub fn near_miss(&mut self, x: Fx, y: Fx) -> bool {
        if self.over {
            return false;
        }
        if let Some(t) = self.last_near_tick {
            if self.tick - t < NEAR_MISS_COOLDOWN_TICKS {
                return false;
            }
        }
        self.last_near_tick = Some(self.tick);
        self.score += 1;
        self.events.push(Event::NearMiss { x: px(x), y: px(y) });
        true
    }

    pub fn event(&mut self, e: Event) {
        self.events.push(e);
    }

    pub fn game_over(&mut self, x: Fx, y: Fx) {
        if self.over {
            return;
        }
        self.over = true;
        self.events.push(Event::GameOver { x: px(x), y: px(y), timeout: false });
    }
}

/// One run of one game.
pub struct Sim {
    core: Core,
    game: templates::Game,
    started: bool,
}

impl Sim {
    pub fn new(rules: Rules, seed: u32) -> Sim {
        let mut core = Core {
            rules,
            rng: Rng::new(seed),
            tick: 0,
            score: 0,
            over: false,
            input: Input::default(),
            events: Vec::new(),
            last_near_tick: None,
            next_id: 0,
        };
        let game = templates::Game::new(&mut core);
        Sim { core, game, started: false }
    }

    /// Advances one tick with this tick's input. The first call starts the run.
    pub fn step(&mut self, input: Input) {
        if self.core.over {
            return;
        }
        self.core.input = input;
        let first = !self.started;
        self.started = true;
        if input.has(Input::TAP) && !(first && input.has(Input::MOVE_START)) {
            self.game.on_input(&mut self.core);
        }
        self.core.tick += 1;
        self.game.update(&mut self.core);
        if !self.core.over && self.core.tick >= MAX_TICKS {
            self.core.over = true;
            self.core.events.push(Event::GameOver { x: 0, y: 0, timeout: true });
        }
    }

    pub fn score(&self) -> u32 {
        self.core.score
    }
    pub fn is_over(&self) -> bool {
        self.core.over
    }
    pub fn tick(&self) -> u32 {
        self.core.tick
    }
    pub fn rules(&self) -> &Rules {
        &self.core.rules
    }

    /// Events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.core.events)
    }

    /// Positions for drawing.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.core.tick,
            score: self.core.score,
            over: self.core.over,
            speed: to_f64(self.core.speed()),
            player: self.game.player(),
            ents: self.game.entities(),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PlayerView {
    pub x: f64,
    pub y: f64,
    /// Template-specific: vertical speed (flappy, runner), horizontal speed (shooter, dodge).
    pub v: f64,
    pub grounded: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Ent {
    pub id: u32,
    /// pipe, obstacle, item, hazard, enemy, elite, shot
    pub k: &'static str,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Template-specific extras (pipe gap top/bottom; hazard warning time; enemy hp).
    pub a: f64,
    pub b: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Snapshot {
    pub tick: u32,
    pub score: u32,
    pub over: bool,
    pub speed: f64,
    pub player: PlayerView,
    pub ents: Vec<Ent>,
}

// ------------------------------------------------------------------ replay

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub score: u32,
    pub ticks: u32,
    pub over: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayError {
    Log(LogError),
    /// The log has input after the run already ended.
    InputAfterEnd { tick: u32 },
}

/// Replays a run from its input log. After the last logged change the input
/// holds until the run ends (by dying or the 10-minute cap); a log that carries
/// input after the end is refused.
pub fn replay(rules: &Rules, seed: u32, log: &[u8]) -> Result<Outcome, ReplayError> {
    let entries = decode_log(log).map_err(ReplayError::Log)?;
    let mut sim = Sim::new(rules.clone(), seed);
    let mut cur = Input::default();
    let mut next = 0;
    while !sim.is_over() {
        let t = sim.tick();
        // A TAP lasts one tick; the log records it set and then cleared.
        while next < entries.len() && entries[next].0 == t {
            cur = entries[next].1;
            next += 1;
        }
        sim.step(cur);
    }
    if next < entries.len() {
        return Err(ReplayError::InputAfterEnd { tick: entries[next].0 });
    }
    Ok(Outcome { score: sim.score(), ticks: sim.tick(), over: sim.is_over() })
}

#[cfg(test)]
mod tests;
