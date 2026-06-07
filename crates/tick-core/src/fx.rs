//! Q32.32 fixed-point on i64. Floats are denied crate-wide; this is the
//! only math game logic gets.

pub type Fx = i64;

pub const FRAC_BITS: u32 = 32;
pub const ONE: Fx = 1 << FRAC_BITS;

#[inline]
pub const fn from_int(v: i32) -> Fx {
    (v as i64) << FRAC_BITS
}

#[inline]
pub const fn to_int(v: Fx) -> i32 {
    (v >> FRAC_BITS) as i32
}

/// (a * b) >> 32 in i128, saturated to i64.
#[inline]
pub fn mul(a: Fx, b: Fx) -> Fx {
    let wide = (a as i128 * b as i128) >> FRAC_BITS;
    clamp(wide)
}

/// (a << 32) / b, saturated. Division by zero saturates instead of
/// trapping: a hostile input must not open a panic path that behaves
/// differently off-chain vs on-chain.
#[inline]
pub fn div(a: Fx, b: Fx) -> Fx {
    if b == 0 {
        return if a >= 0 { i64::MAX } else { i64::MIN };
    }
    clamp(((a as i128) << FRAC_BITS) / (b as i128))
}

#[inline]
fn clamp(v: i128) -> Fx {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_basic() {
        assert_eq!(mul(from_int(3), from_int(4)), from_int(12));
        assert_eq!(mul(from_int(-3), from_int(4)), from_int(-12));
        assert_eq!(mul(ONE / 2, ONE / 2), ONE / 4);
    }

    #[test]
    fn div_basic() {
        assert_eq!(div(from_int(12), from_int(4)), from_int(3));
        assert_eq!(div(from_int(1), from_int(2)), ONE / 2);
    }

    #[test]
    fn div_by_zero_saturates() {
        assert_eq!(div(from_int(5), 0), i64::MAX);
        assert_eq!(div(from_int(-5), 0), i64::MIN);
    }

    #[test]
    fn mul_saturates() {
        assert_eq!(mul(i64::MAX, from_int(2)), i64::MAX);
        assert_eq!(mul(i64::MIN, from_int(2)), i64::MIN);
    }
}
