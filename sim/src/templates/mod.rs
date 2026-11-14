//! The four templates. Each is a straight port of web/public/games/templates/*.js
//! with the drawing removed; every rng() call happens in the same order.

mod dodge;
mod flappy;
mod runner;
mod shooter;

use crate::fixed::*;
use crate::{Core, Ent, PlayerView, Template};

pub enum Game {
    Flappy(flappy::Flappy),
    Runner(runner::Runner),
    Shooter(shooter::Shooter),
    Dodge(dodge::Dodge),
}

impl Game {
    pub fn new(core: &mut Core) -> Game {
        match core.rules.template {
            Template::Flappy => Game::Flappy(flappy::Flappy::new()),
            Template::Runner => Game::Runner(runner::Runner::new(core)),
            Template::Shooter => Game::Shooter(shooter::Shooter::new(core)),
            Template::Dodge => Game::Dodge(dodge::Dodge::new(core)),
        }
    }

    pub fn on_input(&mut self, core: &mut Core) {
        match self {
            Game::Flappy(g) => g.on_input(core),
            Game::Runner(g) => g.on_input(core),
            Game::Shooter(_) | Game::Dodge(_) => {}
        }
    }

    pub fn update(&mut self, core: &mut Core) {
        match self {
            Game::Flappy(g) => g.update(core),
            Game::Runner(g) => g.update(core),
            Game::Shooter(g) => g.update(core),
            Game::Dodge(g) => g.update(core),
        }
    }

    pub fn player(&self) -> PlayerView {
        match self {
            Game::Flappy(g) => g.player(),
            Game::Runner(g) => g.player(),
            Game::Shooter(g) => g.player(),
            Game::Dodge(g) => g.player(),
        }
    }

    pub fn entities(&self) -> Vec<Ent> {
        match self {
            Game::Flappy(g) => g.entities(),
            Game::Runner(g) => g.entities(),
            Game::Shooter(g) => g.entities(),
            Game::Dodge(g) => g.entities(),
        }
    }
}

/// Collectible radius: 45% of the 32 px nominal size.
pub const ITEM_R: Fx = fr(144, 10);
/// 6.28, as written in the templates (not 2π).
pub const SIX28: Fx = fr(628, 100);

/// Circle vs axis-aligned box (left, right, top, bottom).
pub fn circle_box(cx: Fx, cy: Fx, r: Fx, bx: (Fx, Fx, Fx, Fx)) -> bool {
    let dx = cx - clamp(cx, bx.0, bx.1);
    let dy = cy - clamp(cy, bx.2, bx.3);
    mul(dx, dx) + mul(dy, dy) < mul(r, r)
}

/// Circle vs circle.
pub fn circles(ax: Fx, ay: Fx, ar: Fx, bx: Fx, by: Fx, br: Fx) -> bool {
    let dx = ax - bx;
    let dy = ay - by;
    let r = ar + br;
    mul(dx, dx) + mul(dy, dy) < mul(r, r)
}

#[allow(clippy::too_many_arguments)]
pub fn ent(id: u32, k: &'static str, x: Fx, y: Fx, w: Fx, h: Fx, a: Fx, b: Fx) -> Ent {
    Ent {
        id,
        k,
        x: to_f64(x),
        y: to_f64(y),
        w: to_f64(w),
        h: to_f64(h),
        a: to_f64(a),
        b: to_f64(b),
    }
}

pub fn view(x: Fx, y: Fx, v: Fx, grounded: bool) -> PlayerView {
    PlayerView { x: to_f64(x), y: to_f64(y), v: to_f64(v), grounded }
}

/// A bobbing collectible (flappy, runner).
pub struct BobItem {
    pub id: u32,
    pub x: Fx,
    pub base_y: Fx,
    pub y: Fx,
    pub phase: Fx,
}

/// Moves, bobs and collects the items (flappy, runner).
pub fn update_bob_items(core: &mut Core, items: &mut Vec<BobItem>, t: Fx, speed: Fx, hit: (Fx, Fx, Fx, Fx)) {
    let mut keep = Vec::with_capacity(items.len());
    for mut it in items.drain(..) {
        it.x -= per_tick(speed);
        it.y = it.base_y + mul(sin(mul(t, fx(4)) + it.phase), fx(6));
        if circle_box(it.x, it.y, ITEM_R, hit) {
            core.collect(it.x, it.y);
            continue;
        }
        if it.x < fx(-40) {
            continue;
        }
        keep.push(it);
    }
    *items = keep;
}

pub fn bob_entities(items: &[BobItem]) -> impl Iterator<Item = Ent> + '_ {
    items.iter().map(|i| ent(i.id, "item", i.x, i.y, ITEM_R * 2, ITEM_R * 2, 0, 0))
}
