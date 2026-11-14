//! 256-bit helpers. Products such as `deposit * 1e18` and the curve's squared
//! term overflow u128, so every product is taken in U256 and only the final
//! quotient is narrowed back. Each helper says which way it rounds.

#![allow(clippy::manual_div_ceil)]

uint::construct_uint! {
    /// 256-bit unsigned integer.
    pub struct U256(4);
}

/// 10^18: one whole coin in its smallest units (the coin has 18 decimals).
pub const ONE_COIN: u128 = 1_000_000_000_000_000_000;
pub const BPS: u128 = 10_000;

pub fn u(x: u128) -> U256 {
    U256::from(x)
}

/// Narrows a U256 that is known to fit. Panics otherwise, which aborts the call.
pub fn narrow(x: U256) -> u128 {
    assert!(x <= U256::from(u128::MAX), "Amount overflows u128");
    x.as_u128()
}

/// a * b / d, rounded down.
pub fn mul_div_down(a: u128, b: u128, d: u128) -> u128 {
    assert!(d != 0, "Division by zero");
    narrow(u(a) * u(b) / u(d))
}

/// a * b / d, rounded up.
pub fn mul_div_up(a: u128, b: u128, d: u128) -> u128 {
    assert!(d != 0, "Division by zero");
    let n = u(a) * u(b);
    let q = n / u(d);
    narrow(if q * u(d) == n { q } else { q + U256::one() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_both_ways() {
        assert_eq!(mul_div_down(10, 10, 3), 33);
        assert_eq!(mul_div_up(10, 10, 3), 34);
        assert_eq!(mul_div_up(10, 3, 3), 10);
    }

    #[test]
    fn survives_products_beyond_u128() {
        // 0.014 NEAR (1.4e22 yocto) * 1e18 = 1.4e40, past u128::MAX (~3.4e38).
        let deposit = 14_000_000_000_000_000_000_000u128;
        let launch = 100_000_000_000_000_000_000u128; // 0.0001 NEAR
        assert_eq!(mul_div_down(deposit, ONE_COIN, launch), 140 * ONE_COIN);
    }
}
