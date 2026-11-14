//! templates/shooter.js: vertical shooter with auto-fire.

use super::*;
use crate::{Core, Event, Input, H, W};

const PLAYER: Fx = fx(48);
const ENEMY: Fx = fx(48);
const SHOT_SPEED: Fx = fx(900);
const MAX_FALL: Fx = fx(620);
const SHOT_R: Fx = fx(7);

struct Shot {
    id: u32,
    x: Fx,
    y: Fx,
    active: bool,
}

struct Enemy {
    id: u32,
    base_x: Fx,
    x: Fx,
    y: Fx,
    size: Fx,
    r: Fx,
    hp: u32,
    amp: Fx,
    freq: Fx,
    phase: Fx,
    mult: Fx,
    kb: Fx,
    elite: bool,
    passed: bool,
}

struct Item {
    id: u32,
    base_x: Fx,
    x: Fx,
    y: Fx,
    phase: Fx,
}

/// Formation overrides for spawn_enemy.
struct Opts {
    x: Fx,
    y: Fx,
    amp: Fx,
    freq: Fx,
    phase: Fx,
    mult: Fx,
}

pub struct Shooter {
    move_speed: Fx,
    fire_every: Fx,
    hp: u32,
    px: Fx,
    py: Fx,
    vx: Fx,
    base_y: Fx,
    recoil: Fx,
    enemies: Vec<Enemy>,
    shots: Vec<Shot>,
    items: Vec<Item>,
    fire_in: Fx,
    spawn_in: Fx,
    formation_in: Fx,
    elite_in: Fx,
    t: Fx,
    last_near: Fx,
}

impl Shooter {
    pub fn new(core: &mut Core) -> Self {
        let base_y = H - fx(110);
        Shooter {
            move_speed: core.rules.player_speed,
            fire_every: core.rules.fire_rate,
            hp: core.rules.enemy_hp,
            px: W / 2,
            py: base_y,
            vx: 0,
            base_y,
            recoil: 0,
            enemies: vec![],
            shots: vec![],
            items: vec![],
            fire_in: fr(15, 100),
            spawn_in: fr(6, 10),
            formation_in: fx(9),
            elite_in: fx(15),
            t: 0,
            last_near: fx(-9),
        }
    }

    pub fn update(&mut self, core: &mut Core) {
        self.t += DT;
        let elapsed = core.elapsed();
        let fall = core.speed().min(MAX_FALL);
        let inp = core.input;
        let pr = mul(PLAYER, fr(32, 100)); // forgiving hitbox

        // ---- movement: drag follows the pointer, keys move at playerSpeed
        let min_x = PLAYER / 2 + fx(6);
        let max_x = W - PLAYER / 2 - fx(6);
        let dir = inp.has(Input::RIGHT) as i64 - inp.has(Input::LEFT) as i64;
        let mut vx = 0;
        if dir != 0 {
            vx = dir * self.move_speed;
            self.px += per_tick(vx);
        } else if inp.has(Input::DOWN) {
            let max_step = per_tick(mul(self.move_speed, fr(26, 10)));
            let dx = clamp(inp.pointer_x() - self.px, -max_step, max_step);
            vx = dx * 60;
            self.px += dx;
        }
        self.px = clamp(self.px, min_x, max_x);
        self.vx = vx;
        // Recoil kicks the ship down after each shot, and the hitbox with it.
        self.recoil = (self.recoil - per_tick(fx(60))).max(0);
        self.py = self.base_y + self.recoil;

        // ---- auto-fire
        self.fire_in -= DT;
        if self.fire_in <= 0 {
            self.fire_in += self.fire_every;
            if self.fire_in < 0 {
                self.fire_in = self.fire_every;
            }
            self.fire(core, self.px, self.py - PLAYER / 2);
        }

        // ---- spawning (slightly faster over time), formations and elites
        self.spawn_in -= DT;
        self.formation_in -= DT;
        self.elite_in -= DT;
        if self.formation_in <= 0 {
            self.formation_in = fx(9) + mul(core.rng.f(), fx(4));
            self.spawn_formation(core);
            self.spawn_in = self.spawn_in.max(fr(14, 10));
        } else if self.elite_in <= 0 {
            self.elite_in = fx(14) + mul(core.rng.f(), fx(3));
            self.spawn_enemy(core, None, true);
            self.spawn_in = self.spawn_in.max(fr(9, 10));
        } else if self.spawn_in <= 0 {
            let base = mul(core.rules.spawn_interval, fr(6, 10).max(ONE - mul(elapsed, fr(1, 100))));
            let jitter = ONE + mul(mul(core.rng.signed(), fr(4, 10)), core.rules.variance);
            self.spawn_in = fr(28, 100).max(mul(base, jitter));
            self.spawn_enemy(core, None, false);
            if core.rules.has_collectible && core.rng.f() < core.rules.collectible_chance {
                self.spawn_item(core);
            }
        }

        // ---- shots
        for s in self.shots.iter_mut().filter(|s| s.active) {
            s.y -= per_tick(SHOT_SPEED);
            if s.y < fx(-30) {
                s.active = false;
            }
        }

        // ---- enemies
        let ppo = core.rules.points_per_obstacle;
        let mut keep = Vec::with_capacity(self.enemies.len());
        for mut e in std::mem::take(&mut self.enemies) {
            if e.kb < 0 {
                e.kb = (e.kb + per_tick(fx(900))).min(0);
            }
            e.y += per_tick(mul(fall, e.mult) + e.kb);
            let wobble = mul(sin(mul(self.t, e.freq) + e.phase), e.amp);
            e.x = clamp(e.base_x + wobble, e.size / 2, W - e.size / 2);

            for s in self.shots.iter_mut() {
                if e.hp == 0 {
                    break;
                }
                if !s.active {
                    continue;
                }
                if circles(s.x, s.y, SHOT_R, e.x, e.y, e.r) {
                    s.active = false;
                    e.hp -= 1;
                    if e.hp > 0 {
                        e.kb = fx(-170);
                        core.event(Event::Hit { id: e.id, x: (s.x >> 16) as i32, y: (s.y >> 16) as i32 });
                    }
                }
            }
            if e.hp == 0 {
                let pts = ppo * if e.elite { 3 } else { 1 };
                core.add_score(pts, e.x, e.y);
                core.event(Event::Kill { id: e.id, x: (e.x >> 16) as i32, y: (e.y >> 16) as i32, elite: e.elite });
                continue;
            }

            if circles(self.px, self.py, pr, e.x, e.y, e.r) {
                core.game_over(self.px, self.py);
                self.enemies = keep;
                return;
            }
            // Near miss: the enemy passes the ship's line close by.
            if !e.passed && e.y > self.py {
                e.passed = true;
                let gx = (e.x - self.px).abs() - (pr + e.r);
                if gx < fx(16) && self.t - self.last_near > fr(1, 2) {
                    self.last_near = self.t;
                    core.near_miss(self.px, self.py);
                }
            }
            if e.y - e.size / 2 > H + fx(10) {
                continue; // slipped past: no penalty
            }
            keep.push(e);
        }
        self.enemies = keep;

        // ---- collectibles
        let mut keep = Vec::with_capacity(self.items.len());
        for mut it in std::mem::take(&mut self.items) {
            it.y += per_tick(mul(fall, fr(75, 100)));
            it.x = it.base_x + mul(sin(mul(self.t, fx(3)) + it.phase), fx(14));
            if circles(self.px, self.py, pr + fx(6), it.x, it.y, ITEM_R) {
                core.collect(it.x, it.y);
                continue;
            }
            if it.y > H + fx(30) {
                continue;
            }
            keep.push(it);
        }
        self.items = keep;
    }

    fn fire(&mut self, core: &mut Core, x: Fx, y: Fx) {
        match self.shots.iter_mut().find(|s| !s.active) {
            Some(s) => {
                s.x = x;
                s.y = y;
                s.active = true;
                s.id = core.id();
            }
            None => {
                let id = core.id();
                self.shots.push(Shot { id, x, y, active: true });
            }
        }
        self.recoil = fx(5);
        core.event(Event::Shoot { x: (x >> 16) as i32, y: (y >> 16) as i32 });
    }

    fn spawn_enemy(&mut self, core: &mut Core, opts: Option<Opts>, elite: bool) {
        let v = core.rules.variance;
        let size = if elite { mul(ENEMY, fr(14, 10)) } else { ENEMY };
        let amp = match &opts {
            Some(o) => o.amp,
            None => fx(12) + mul(mul(fx(70), v), core.rng.f()),
        };
        let margin = size / 2 + amp + fx(4);
        let base_x = match &opts {
            Some(o) => o.x,
            None => margin + mul(core.rng.f(), ONE.max(W - margin * 2)),
        };
        let y = match &opts {
            Some(o) => o.y,
            None => -size / 2,
        };
        let (freq, phase, mult) = match &opts {
            Some(o) => (o.freq, o.phase, o.mult),
            None => {
                let freq = fr(14, 10) + mul(core.rng.f(), fr(18, 10));
                let phase = mul(core.rng.f(), TWO_PI);
                let mult = if elite {
                    fr(7, 10)
                } else {
                    fr(85, 100) + mul(mul(core.rng.f(), fr(3, 10)), ONE / 2 + v)
                };
                (freq, phase, mult)
            }
        };
        let id = core.id();
        self.enemies.push(Enemy {
            id,
            base_x,
            x: base_x,
            y,
            size,
            r: mul(size, fr(36, 100)),
            hp: if elite { self.hp + 2 } else { self.hp },
            amp,
            freq,
            phase,
            mult,
            kb: 0,
            elite,
            passed: false,
        });
    }

    fn spawn_formation(&mut self, core: &mut Core) {
        let phase = mul(core.rng.f(), SIX28);
        let freq = fr(12, 10);
        if core.rng.f() < ONE / 2 {
            // V: 5 ships, leader in front.
            let cx = fx(120) + mul(core.rng.f(), W - fx(240));
            for i in -2i64..=2 {
                let x = clamp(cx + fx(i * 54), fx(50), W - fx(50));
                let o = Opts { x, y: fx(-30 - i.abs() * 46), amp: fx(18), freq, phase, mult: fr(8, 10) };
                self.spawn_enemy(core, Some(o), false);
            }
            core.event(Event::Wave { kind: "v" });
        } else {
            // A line across with a 2-slot hole to fly through.
            let slots = 6;
            let step = (W - fx(80)) / (slots - 1);
            let hole = (mul(core.rng.f(), fx(slots - 1)) >> 16) as i64;
            for j in 0..slots {
                if j == hole || j == hole + 1 {
                    continue;
                }
                let o = Opts { x: fx(40) + step * j, y: fx(-30), amp: fx(8), freq, phase, mult: fr(75, 100) };
                self.spawn_enemy(core, Some(o), false);
            }
            core.event(Event::Wave { kind: "wall" });
        }
    }

    fn spawn_item(&mut self, core: &mut Core) {
        let base_x = fx(40) + mul(core.rng.f(), W - fx(80));
        let phase = mul(core.rng.f(), SIX28);
        let id = core.id();
        // y = -(32 px nominal height)/2 - 40, as in the JS.
        self.items.push(Item { id, base_x, x: base_x, y: fx(-56), phase });
    }

    pub fn player(&self) -> crate::PlayerView {
        view(self.px, self.py, self.vx, true)
    }

    pub fn entities(&self) -> Vec<crate::Ent> {
        let mut v: Vec<crate::Ent> = self
            .enemies
            .iter()
            .map(|e| ent(e.id, if e.elite { "elite" } else { "enemy" }, e.x, e.y, e.size, e.size, fx(e.hp as i64), e.r))
            .collect();
        v.extend(self.shots.iter().filter(|s| s.active).map(|s| ent(s.id, "shot", s.x, s.y, fx(8), fx(20), 0, 0)));
        v.extend(self.items.iter().map(|i| ent(i.id, "item", i.x, i.y, ITEM_R * 2, ITEM_R * 2, 0, 0)));
        v
    }
}
