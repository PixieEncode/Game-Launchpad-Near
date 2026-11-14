//! mulberry32, bit-for-bit the generator in web/public/games/core.js.

use crate::fixed::Fx;

#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed)
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        t ^ (t >> 14)
    }

    /// A value in [0, 1) as fixed point (what `ctx.rng()` returns, at 16-bit precision).
    pub fn f(&mut self) -> Fx {
        (self.next_u32() >> 16) as Fx
    }

    /// 2r - 1: a value in [-1, 1).
    pub fn signed(&mut self) -> Fx {
        2 * self.f() - crate::fixed::ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_javascript_generator() {
        // mulberry32(42) in JS: first outputs × 2^32.
        let mut r = Rng::new(42);
        let got: Vec<u32> = (0..3).map(|_| r.next_u32()).collect();
        assert_eq!(got, vec![2_581_720_956, 1_925_393_290, 3_661_312_704]);
    }
}
