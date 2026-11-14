//! templates/dodge.js: dodge falling hazards, catch collectibles.

use super::*;
use crate::{Core, Event, Input, W};

const PLAYER: Fx = fx(52);
const HAZARD: Fx = fx(42);
const MAX_FALL: Fx = fx(680);
const WARN: Fx = fr(42, 100); // s of telegraph before a hazard drops in
const FLOOR_Y: Fx = fx(664);

struct Hazard {
    id: u32,
    x: Fx,
    y: Fx,
    r: Fx,
    size: Fx,
    warn: Fx,
    mult: Fx,
}

struct Item {
    id: u32,
    base_x: Fx,
    x: Fx,
    y: Fx,
    phase: Fx,
    delay: Fx,
}

pub struct Dodge {
    move_speed: Fx,
    px: Fx,
    py: Fx,
    vx: Fx,
    hazards: Vec<Hazard>,
    items: Vec<Item>,
    spawn_in: Fx,
    item_in: Fx,
    wave_in: Fx,
    t: Fx,
    last_near: Fx,
}

impl Dodge {
    pub fn new(core: &mut Core) -> Self {
        Dodge {
            move_speed: core.rules.player_speed,
            px: W / 2,
            py: FLOOR_Y + fx(2),
            vx: 0,
            hazards: vec![],
            items: vec![],
            spawn_in: fr(1, 2),
            item_in: fr(12, 10),
            wave_in: fx(10),
            t: 0,
            last_near: fx(-9),
        }
    }

    pub fn update(&mut self, core: &mut Core) {
        self.t += DT;
        let elapsed = core.elapsed();
        let fall = core.speed().min(MAX_FALL);
        let inp = core.input;

        // ---- movement: keys at playerSpeed, else drag towards the pointer
        let min_x = PLAYER / 2 + fx(4);
        let max_x = W - PLAYER / 2 - fx(4);
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
        // The run cycle bobs the body, and with it the hitbox.
        let moving = vx > fx(40) || vx < fx(-40);
        let bob = if moving { mul(sin(mul(self.t, fx(18))).abs(), fx(4)) } else { 0 };
        self.py = FLOOR_Y + fx(2) - bob;

        let hw = mul(PLAYER, fr(32, 100));
        let top = self.py - mul(PLAYER, fr(78, 100));
        let bottom = self.py - mul(PLAYER, fr(8, 100));
        let (l, rt) = (self.px - hw, self.px + hw);

        // ---- spawning: interval shrinks to 55% over ~40 s
        let var = core.rules.variance;
        let ramp = fr(55, 100).max(ONE - mul(elapsed, fr(11, 1000)));
        self.spawn_in -= DT;
        self.wave_in -= DT;
        if self.wave_in <= 0 {
            self.wave_in = fx(10) + mul(core.rng.f(), fx(4));
            if core.rules.has_collectible && core.rng.f() < fr(45, 100) {
                self.pump_wave(core);
            } else {
                self.dump_wave(core);
            }
            self.spawn_in = self.spawn_in.max(fr(16, 10));
        } else if self.spawn_in <= 0 {
            let jitter = ONE + mul(mul(core.rng.signed(), fr(45, 100)), var);
            self.spawn_in = fr(22, 100).max(mul(mul(core.rules.spawn_interval, ramp), jitter));
            self.spawn_hazard(core, None, WARN, None);
            // At higher variance, sometimes a second hazard in the same wave.
            if var > fr(1, 2) && elapsed > fx(8) && core.rng.f() < mul(var - fr(1, 2), fr(6, 10)) {
                self.spawn_hazard(core, None, WARN, None);
            }
        }
        if core.rules.has_collectible {
            self.item_in -= DT;
            if self.item_in <= 0 {
                self.item_in = mul(core.rules.spawn_interval, fr(8, 10) + mul(core.rng.f(), fr(6, 10)));
                if core.rng.f() < core.rules.collectible_chance {
                    let x = fx(40) + mul(core.rng.f(), W - fx(80));
                    self.spawn_item(core, x, 0);
                }
            }
        }

        // ---- hazards
        let ppo = core.rules.points_per_obstacle;
        let mut keep = Vec::with_capacity(self.hazards.len());
        let hazards = std::mem::take(&mut self.hazards);
        for mut h in hazards {
            if h.warn > 0 {
                h.warn -= DT;
                keep.push(h);
                continue;
            }
            h.y += per_tick(mul(fall, h.mult));
            if circle_box(h.x, h.y, h.r, (l, rt, top, bottom)) {
                core.game_over(h.x, h.y);
                self.hazards = keep;
                return;
            }
            if h.y + h.r >= FLOOR_Y {
                core.add_score(ppo, h.x, FLOOR_Y - fx(10));
                core.event(Event::HazardLand { id: h.id, x: (h.x >> 16) as i32 });
                let gap = (h.x - self.px).abs() - (hw + h.r);
                if gap < fx(18) && self.t - self.last_near > fr(35, 100) {
                    self.last_near = self.t;
                    core.near_miss(self.px, FLOOR_Y - PLAYER);
                }
                continue;
            }
            keep.push(h);
        }
        self.hazards = keep;

        // ---- collectibles
        let mut keep = Vec::with_capacity(self.items.len());
        for mut it in std::mem::take(&mut self.items) {
            if it.delay > 0 {
                it.delay -= DT;
                keep.push(it);
                continue;
            }
            it.y += per_tick(mul(fall, fr(8, 10)));
            it.x = it.base_x + mul(sin(mul(self.t, fr(25, 10)) + it.phase), fx(18));
            if circle_box(it.x, it.y, ITEM_R, (l - fx(6), rt + fx(6), top - fx(6), bottom)) {
                core.collect(it.x, it.y);
                continue;
            }
            if it.y - ITEM_R > FLOOR_Y {
                continue;
            }
            keep.push(it);
        }
        self.items = keep;
    }

    /// force_x: fixed column (waves) or None; warn: telegraph seconds; mult: fall multiplier.
    fn spawn_hazard(&mut self, core: &mut Core, force_x: Option<Fx>, warn: Fx, mult: Option<Fx>) {
        let v = core.rules.variance;
        let sc = match force_x {
            None => ONE + mul(mul(core.rng.signed(), fr(25, 100)), v),
            Some(_) => ONE,
        };
        let size = mul(HAZARD, sc);
        let x = match force_x {
            Some(x) => x,
            None => {
                // A share of hazards aim near the player, so camping in one spot fails.
                if core.rng.f() < fr(3, 10) {
                    self.px + mul(core.rng.signed(), fx(70))
                } else {
                    size / 2 + mul(core.rng.f(), W - size)
                }
            }
        };
        let x = clamp(x, size / 2 + fx(2), W - size / 2 - fx(2));
        let mult = match mult {
            Some(m) => m,
            None => ONE + mul(mul(core.rng.signed(), fr(18, 100)), v),
        };
        let _spin = core.rng.signed(); // visual only, but it draws from the rng
        let id = core.id();
        self.hazards.push(Hazard { id, x, y: -size / 2, r: mul(size, fr(36, 100)), size, warn, mult });
    }

    fn dump_wave(&mut self, core: &mut Core) {
        // A wall of hazards with a 2-column gap.
        let cols = 8;
        let step = W / cols;
        let gap = (mul(core.rng.f(), fx(cols - 1)) >> 16) as i64;
        for i in 0..cols {
            if i == gap || i == gap + 1 {
                continue;
            }
            self.spawn_hazard(core, Some(mul(step, fx(i) + ONE / 2)), fr(7, 10), Some(fr(9, 10)));
        }
        core.event(Event::Wave { kind: "dump" });
    }

    fn pump_wave(&mut self, core: &mut Core) {
        let start = core.rng.f() < ONE / 2;
        for i in 0..6 {
            let f = fr(i, 5);
            let pos = if start { f } else { ONE - f };
            self.spawn_item(core, fx(60) + mul(pos, W - fx(120)), mul(fx(i), fr(18, 100)));
        }
        core.event(Event::Wave { kind: "pump" });
    }

    fn spawn_item(&mut self, core: &mut Core, base_x: Fx, delay: Fx) {
        let phase = mul(core.rng.f(), SIX28);
        let id = core.id();
        self.items.push(Item { id, base_x, x: base_x, y: -ITEM_R, phase, delay });
    }

    pub fn player(&self) -> crate::PlayerView {
        view(self.px, self.py, self.vx, true)
    }

    pub fn entities(&self) -> Vec<crate::Ent> {
        let mut v: Vec<crate::Ent> = self
            .hazards
            .iter()
            .map(|h| ent(h.id, "hazard", h.x, h.y, h.size, h.size, h.warn.max(0), h.r))
            .collect();
        v.extend(
            self.items
                .iter()
                .filter(|i| i.delay <= 0)
                .map(|i| ent(i.id, "item", i.x, i.y, ITEM_R * 2, ITEM_R * 2, 0, 0)),
        );
        v
    }
}
