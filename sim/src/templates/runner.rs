//! templates/runner.js: tap to jump over ground obstacles.

use super::*;
use crate::{Core, Event, W};

const GROUND_Y: Fx = fx(610);
const PW: Fx = fx(48);
const PH: Fx = fx(48);
const OB_W: Fx = fx(52);
const OB_H: Fx = fx(68);
const COYOTE: Fx = fr(9, 100); // s of grace to still jump after leaving the ground
const BUFFER: Fx = fr(12, 100); // s a press is remembered before landing
const HOLD_MAX: Fx = fr(2, 10); // s of "hold for a higher jump"
const HOLD_GRAV: Fx = fr(1, 2); // gravity multiplier while holding (rising only)

struct Part {
    dx: Fx,
    l: Fx,
    r: Fx,
    top: Fx,
}

struct Obstacle {
    id: u32,
    x: Fx,
    w: Fx,
    h: Fx,
    part_w: Fx,
    scored: bool,
    parts: Vec<Part>,
    min_clr: Fx,
}

pub struct Runner {
    px: Fx,
    py: Fx,
    vy: Fx,
    grounded: bool,
    coyote: Fx,
    jump_buffer: Fx,
    hold: Fx,
    obstacles: Vec<Obstacle>,
    items: Vec<BobItem>,
    spawn_in: Fx,
    t: Fx,
    last_near: Fx,
    apex: Fx,
    airtime: Fx,
    airtime_max: Fx,
}

impl Runner {
    pub fn new(core: &mut Core) -> Self {
        let g = core.rules.gravity;
        let jv = core.rules.jump_velocity;
        let airtime = div(2 * jv, g);
        Runner {
            px: fx(110),
            py: GROUND_Y,
            vy: 0,
            grounded: true,
            coyote: 0,
            jump_buffer: 0,
            hold: 0,
            obstacles: vec![],
            items: vec![],
            spawn_in: ONE,
            t: 0,
            last_near: fx(-9),
            apex: div(mul(jv, jv), 2 * g),
            airtime,
            airtime_max: airtime + mul(mul(HOLD_MAX, ONE - HOLD_GRAV), fr(16, 10)),
        }
    }

    pub fn on_input(&mut self, core: &mut Core) {
        if !self.try_jump(core) {
            self.jump_buffer = BUFFER;
        }
    }

    fn try_jump(&mut self, core: &mut Core) -> bool {
        if !(self.grounded || self.coyote > 0) {
            return false;
        }
        self.vy = -core.rules.jump_velocity;
        self.grounded = false;
        self.coyote = 0;
        self.jump_buffer = 0;
        self.hold = HOLD_MAX;
        core.event(Event::Jump);
        true
    }

    pub fn update(&mut self, core: &mut Core) {
        let speed = core.speed();
        self.t += DT;

        self.jump_buffer -= DT;
        if self.grounded {
            self.coyote = COYOTE;
        } else {
            self.coyote -= DT;
        }
        if !self.grounded {
            let mut gmul = ONE;
            if self.vy < 0 && self.hold > 0 {
                if core.input.held() {
                    self.hold -= DT;
                    gmul = HOLD_GRAV;
                } else {
                    self.hold = 0; // released: normal arc from here
                }
            }
            self.vy += per_tick(mul(core.rules.gravity, gmul));
            self.py += per_tick(self.vy);
            if self.py >= GROUND_Y {
                let impact = self.vy;
                self.py = GROUND_Y;
                self.vy = 0;
                self.grounded = true;
                core.event(Event::Land { impact: (impact >> 16) as i32 });
                if self.jump_buffer > 0 {
                    self.jump_buffer = 0;
                    self.try_jump(core);
                }
            }
        }

        self.spawn_in -= DT;
        if self.spawn_in <= 0 {
            let interval = self.next_interval(core, speed);
            self.spawn(core, speed, interval);
            self.spawn_in = interval;
        }

        // Hitbox: 60% wide, 80% tall of the sprite.
        let hy = self.py - PH / 2;
        let hw = mul(PW, fr(3, 10));
        let hh = mul(PH, fr(4, 10));
        let hit = (self.px - hw, self.px + hw, hy - hh, hy + hh);
        let (l, rt, _top, b) = hit;
        let mut dead = false;
        let ppo = core.rules.points_per_obstacle;
        let mut keep = Vec::with_capacity(self.obstacles.len());
        for mut o in self.obstacles.drain(..) {
            o.x -= per_tick(speed);
            for p in &o.parts {
                if rt > o.x + p.l && l < o.x + p.r {
                    if b > p.top {
                        dead = true;
                    } else if p.top - b < o.min_clr {
                        o.min_clr = p.top - b;
                    }
                }
            }
            if !o.scored && o.x + o.w / 2 < l {
                o.scored = true;
                core.add_score(ppo, o.x, GROUND_Y - o.h - fx(10));
                if o.min_clr < fx(14) && self.t - self.last_near > fr(4, 10) {
                    self.last_near = self.t;
                    core.near_miss(self.px, hy);
                }
            }
            if o.x + o.w >= fx(-40) {
                keep.push(o);
            }
        }
        self.obstacles = keep;
        update_bob_items(core, &mut self.items, self.t, speed, hit);
        if dead {
            core.game_over(self.px, hy);
        }
    }

    fn next_interval(&self, core: &mut Core, speed: Fx) -> Fx {
        let (base, var) = (core.rules.spawn_interval, core.rules.variance);
        let j = mul(base, ONE + mul(mul(core.rng.signed(), fr(4, 10)), var));
        // Always leave room to land and jump again (even after a held, floatier jump).
        j.max(self.airtime_max + fr(3, 10) + div(mul(OB_W, fr(22, 10)), speed))
    }

    fn spawn(&mut self, core: &mut Core, speed: Fx, interval: Fx) {
        let v = core.rules.variance;
        let sc = ONE + mul(mul(core.rng.signed(), fr(3, 10)), v);
        let mut w = mul(OB_W, sc);
        let mut h = mul(OB_H, sc);
        let max_h = fx(26).max(self.apex - fx(34)); // always jumpable with a plain tap
        if h > max_h {
            w = mul(w, div(max_h, h));
            h = max_h;
        }
        let mut count = 1;
        // rng() is only drawn when the variance is above 0.4, as in the JS.
        if v > fr(4, 10)
            && core.rng.f() < mul(v - fr(4, 10), fr(8, 10))
            && mul(self.airtime, speed) > 2 * w + fx(80)
        {
            count = 2;
        }
        let x = W + w / 2 + fx(8);
        let inset = mul(w, fr(14, 100));
        let mut parts = Vec::new();
        for i in 0..count {
            let dx = mul(fx(i), mul(w, fr(9, 10)));
            let _wobble_phase = core.rng.f(); // visual only, but it draws from the rng
            parts.push(Part {
                dx,
                l: dx - w / 2 + inset,
                r: dx + w / 2 - inset,
                top: GROUND_Y - h + inset,
            });
        }
        let id = core.id();
        self.obstacles.push(Obstacle {
            id,
            x,
            w: if count == 2 { mul(w, fr(19, 10)) } else { w },
            h,
            part_w: w,
            scored: false,
            parts,
            min_clr: fx(1_000_000),
        });
        if core.rules.has_collectible && core.rng.f() < core.rules.collectible_chance {
            let ix = x + mul(speed, interval) / 2;
            let span = fx(4).max(mul(self.apex, fr(85, 100)).min(fx(210)) - fx(56));
            let iy = GROUND_Y - (fx(56) + mul(core.rng.f(), span));
            let phase = mul(core.rng.f(), SIX28);
            let id = core.id();
            self.items.push(BobItem { id, x: ix, base_y: iy, y: iy, phase });
        }
    }

    pub fn player(&self) -> crate::PlayerView {
        view(self.px, self.py, self.vy, self.grounded)
    }

    pub fn entities(&self) -> Vec<crate::Ent> {
        let mut v = Vec::new();
        for o in &self.obstacles {
            for (i, p) in o.parts.iter().enumerate() {
                // One entity per part; ids stay stable (part 2 = id + 1_000_000).
                v.push(ent(o.id + i as u32 * 1_000_000, "obstacle", o.x + p.dx, GROUND_Y, o.part_w, o.h, 0, 0));
            }
        }
        v.extend(bob_entities(&self.items));
        v
    }
}
