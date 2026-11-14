//! 16.16 fixed-point maths. No floating point anywhere in the simulation, so
//! every machine (browser, server, enclave) computes the same run.

pub type Fx = i64;

pub const ONE: Fx = 1 << 16;
/// One tick: 1/60 s.
pub const DT: Fx = ONE / 60;
pub const PI: Fx = 205_887; // π × 65536
pub const TWO_PI: Fx = 411_775;

/// Whole number to fixed point.
pub const fn fx(n: i64) -> Fx {
    n << 16
}

/// num / den as fixed point (for constants like 0.42 = fr(42, 100)).
pub const fn fr(num: i64, den: i64) -> Fx {
    (num << 16) / den
}

pub fn mul(a: Fx, b: Fx) -> Fx {
    ((a as i128 * b as i128) >> 16) as i64
}

pub fn div(a: Fx, b: Fx) -> Fx {
    if b == 0 {
        return 0;
    }
    (((a as i128) << 16) / b as i128) as i64
}

pub fn clamp(v: Fx, lo: Fx, hi: Fx) -> Fx {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Displacement over one tick at `v` per second.
pub fn per_tick(v: Fx) -> Fx {
    mul(v, DT)
}

/// sin(x) for x in radians, Bhaskara I approximation (error < 0.2%).
pub fn sin(x: Fx) -> Fx {
    let mut x = x % TWO_PI;
    if x < 0 {
        x += TWO_PI;
    }
    let (x, neg) = if x > PI { (x - PI, true) } else { (x, false) };
    let p = mul(x, PI - x);
    let r = div(16 * p, 5 * mul(PI, PI) - 4 * p);
    if neg {
        -r
    } else {
        r
    }
}

/// From a config number. Parsing and rounding f64 are exact IEEE operations, so
/// this is deterministic; nothing after it uses floats.
pub fn from_f64(v: f64) -> Fx {
    (v * 65_536.0).round() as Fx
}

pub fn to_f64(v: Fx) -> f64 {
    v as f64 / 65_536.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_close() {
        for deg in (0..360).step_by(15) {
            let x = fr(deg as i64 * 314_159, 18_000_000);
            let got = to_f64(sin(x));
            let want = (deg as f64).to_radians().sin();
            assert!((got - want).abs() < 0.003, "sin({deg}) = {got}, want {want}");
        }
    }

    #[test]
    fn rounding() {
        assert_eq!(mul(fx(3), fr(1, 2)), fr(3, 2));
        assert_eq!(div(fx(3), fx(2)), fr(3, 2));
        assert_eq!(per_tick(fx(60)), 65_520);
    }
}
