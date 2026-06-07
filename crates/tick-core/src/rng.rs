//! xorshift64* PRNG. Seeded only from committed data (checkpoint hash,
//! tick index), never from host entropy - the seed is part of the record.
//! Not cryptographic; dispute integrity comes from replay, not from the
//! RNG being unpredictable.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    /// Zero is a fixed point of xorshift, remap it (deterministically).
    #[inline]
    pub const fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, n) via Lemire reduction.
    #[inline]
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    // Frozen outputs. If these change, the PRNG semantics changed and
    // every committed replay breaks - do not update the constants.
    #[test]
    fn golden_values() {
        let mut r = Rng::new(1);
        assert_eq!(r.next_u64(), 0x47E4_CE4B_896C_DD1D);
        assert_eq!(r.next_u64(), 0xABCF_A6A8_E079_651D);
        assert_eq!(r.next_u64(), 0xB9D1_0D8F_EB73_1F57);
    }

    #[test]
    fn zero_seed_remapped() {
        assert_eq!(Rng::new(0), Rng::new(0x9E37_79B9_7F4A_7C15));
    }

    #[test]
    fn next_below_bounds() {
        let mut r = Rng::new(7);
        for n in [1u64, 2, 10, 1000] {
            for _ in 0..100 {
                assert!(r.next_below(n) < n);
            }
        }
    }
}
