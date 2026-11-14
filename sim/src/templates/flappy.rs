//! templates/flappy.js: tap to flap through pipe pairs.

use super::*;
use crate::{Core, Event, H, W};

const GROUND_Y: Fx = fx(660);
const PW: Fx = fx(48);
const OBST_W: Fx = fx(72);

struct Pipe {
    id: u32,
    x: Fx,
    gap_top: Fx,
    gap_bot: Fx,
    scored: bool,
    min_clr: Fx,
}

pub struct Flappy {
    px: Fx,
    py: Fx,
    vy: Fx,
    pipes: Vec<Pipe>,
    items: Vec<BobItem>,
    last_center: Option<Fx>,
    spawn_in: Fx,
    t: Fx,
    last_near: Fx,
}

impl Flappy {
    pub fn new() -> Self {
        Flappy {
            px: fx(140),
            py: mul(H, fr(42, 100)),
            vy: 0,
            pipes: vec![],
            items: vec![],
            last_center: None,
            spawn_in: fr(8, 10),
            t: 0,
            last_near: fx(-9),
        }
    }

    pub fn on_input(&mut self, core: &mut Core) {
        self.vy = -core.rules.jump_velocity;
        core.event(Event::Jump);
    }

    pub fn update(&mut self, core: &mut Core) {
        let speed = core.speed();
        self.t += DT;
        self.vy += per_tick(core.rules.gravity);
        self.py += per_tick(self.vy);
        // Soft ceiling: a bump, not a sticky wall.
        let ceil = mul(PW, fr(4, 10)) + fx(6);
        if self.py < ceil {
            self.py = ceil;
            if self.vy < 0 {
                self.vy = fx(120).min(mul(-self.vy, fr(25, 100)));
            }
        }

        self.spawn_in -= DT;
        if self.spawn_in <= 0 {
            let interval = self.next_interval(core, speed);
            self.spawn(core, speed, interval);
            self.spawn_in = interval;
        }

        // Hitbox: 68% of the sprite.
        let hw = mul(PW, fr(34, 100));
        let hit = (self.px - hw, self.px + hw, self.py - hw, self.py + hw);
        let (l, rt, top, b) = hit;
        let mut dead = false;
        let ppo = core.rules.points_per_obstacle;
        let inset = mul(OBST_W, fr(8, 100));
        let mut keep = Vec::with_capacity(self.pipes.len());
        for mut o in self.pipes.drain(..) {
            o.x -= per_tick(speed);
            let pl = o.x - OBST_W / 2 + inset;
            let pr = o.x + OBST_W / 2 - inset;
            if rt > pl && l < pr {
                if top < o.gap_top - fx(2) || b > o.gap_bot + fx(2) {
                    dead = true;
                } else {
                    let clr = (top - o.gap_top).min(o.gap_bot - b);
                    if clr < o.min_clr {
                        o.min_clr = clr;
                    }
                }
            }
            if !o.scored && o.x + OBST_W / 2 < l {
                o.scored = true;
                core.add_score(ppo, o.x + OBST_W / 2, (o.gap_top + o.gap_bot) / 2);
                if o.min_clr < fx(12) && self.t - self.last_near > fr(4, 10) {
                    self.last_near = self.t;
                    core.near_miss(self.px, self.py);
                }
            }
            if o.x + OBST_W >= fx(-40) {
                keep.push(o);
            }
        }
        self.pipes = keep;
        update_bob_items(core, &mut self.items, self.t, speed, hit);
        if b >= GROUND_Y {
            dead = true;
        }
        if dead {
            core.game_over(self.px, self.py);
        }
    }

    fn next_interval(&self, core: &mut Core, speed: Fx) -> Fx {
        let (base, var) = (core.rules.spawn_interval, core.rules.variance);
        let j = mul(base, ONE + mul(mul(core.rng.signed(), fr(4, 10)), var));
        j.max(div(OBST_W + fx(150), speed))
    }

    fn spawn(&mut self, core: &mut Core, speed: Fx, interval: Fx) {
        let gap = core.rules.gap_size;
        let var = core.rules.variance;
        let margin = fx(60);
        let min_c = margin + gap / 2;
        let max_c = GROUND_Y - margin - gap / 2;
        let mid = (min_c + max_c) / 2;
        let half = (max_c - min_c) / 2;
        let mut center = mid + mul(mul(core.rng.signed(), half), var);
        if let Some(lc) = self.last_center {
            center = clamp(center, lc - fx(240), lc + fx(240));
        }
        center = clamp(center, min_c, max_c);
        self.last_center = Some(center);
        let x = W + OBST_W / 2 + fx(8);
        let id = core.id();
        self.pipes.push(Pipe {
            id,
            x,
            gap_top: center - gap / 2,
            gap_bot: center + gap / 2,
            scored: false,
            min_clr: fx(1_000_000),
        });
        if core.rules.has_collectible && core.rng.f() < core.rules.collectible_chance {
            let ix = x + mul(speed, interval) / 2;
            let off = core.rng.f() - ONE / 2;
            let iy = clamp(center + mul(mul(off, fr(1, 2)), gap), fx(60), GROUND_Y - fx(60));
            let phase = mul(core.rng.f(), SIX28);
            let id = core.id();
            self.items.push(BobItem { id, x: ix, base_y: iy, y: iy, phase });
        }
    }

    pub fn player(&self) -> crate::PlayerView {
        view(self.px, self.py, self.vy, false)
    }

    pub fn entities(&self) -> Vec<crate::Ent> {
        let mut v: Vec<crate::Ent> = self
            .pipes
            .iter()
            .map(|p| ent(p.id, "pipe", p.x, 0, OBST_W, 0, p.gap_top, p.gap_bot))
            .collect();
        v.extend(bob_entities(&self.items));
        v
    }
}
