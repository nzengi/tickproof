//! Minimal physics arena: 8 balls in a 256x256 box, impulse inputs, wall
//! bounces, light friction. Just enough game to exercise the whole
//! tick -> commit -> dispute pipeline; not meant to be fun yet.

#![no_std]
#![deny(clippy::float_arithmetic)]
#![deny(unsafe_code)]

use tick_core::fx::{self, Fx};
use tick_core::{le, TickError, TickLogic};

pub const N_BALLS: usize = 8;

/// State layout, little-endian:
///   0..8        tick counter (u64)
///   8 + i*32    ball i: pos_x, pos_y, vel_x, vel_y (4 x i64, Q32.32)
pub const STATE_SIZE: usize = 8 + N_BALLS * 32;

/// One input entry: ball index (u32) + impulse dvx, dvy (2 x i64, Q32.32).
pub const INPUT_ENTRY_SIZE: usize = 4 + 8 + 8;

const ARENA_MIN: Fx = 0;
const ARENA_MAX: Fx = fx::from_int(256);
// ~0.999 per tick
const FRICTION: Fx = fx::ONE - (fx::ONE >> 10);

pub struct Arena;

impl Arena {
    /// Spread the balls along the diagonal, everything at rest.
    pub fn init(state: &mut [u8]) -> Result<(), TickError> {
        if state.len() != STATE_SIZE {
            return Err(TickError::BadStateSize);
        }
        state.fill(0);
        for i in 0..N_BALLS {
            let base = 8 + i * 32;
            let p = fx::from_int(32 + (i as i32) * 28);
            le::write_i64(state, base, p);
            le::write_i64(state, base + 8, p);
        }
        Ok(())
    }
}

impl TickLogic for Arena {
    const STATE_SIZE: usize = STATE_SIZE;

    fn tick(state: &mut [u8], inputs: &[u8], _tick_index: u64) -> Result<(), TickError> {
        if state.len() != STATE_SIZE {
            return Err(TickError::BadStateSize);
        }
        if !inputs.len().is_multiple_of(INPUT_ENTRY_SIZE) {
            return Err(TickError::BadInput);
        }

        // impulses first
        let mut off = 0;
        while off < inputs.len() {
            let ball = le::read_u32(inputs, off) as usize;
            if ball >= N_BALLS {
                return Err(TickError::BadInput);
            }
            let base = 8 + ball * 32;
            let dvx = le::read_i64(inputs, off + 4);
            let dvy = le::read_i64(inputs, off + 12);
            le::write_i64(state, base + 16, le::read_i64(state, base + 16).saturating_add(dvx));
            le::write_i64(state, base + 24, le::read_i64(state, base + 24).saturating_add(dvy));
            off += INPUT_ENTRY_SIZE;
        }

        // integrate, bounce, friction
        for i in 0..N_BALLS {
            let base = 8 + i * 32;
            let mut px = le::read_i64(state, base);
            let mut py = le::read_i64(state, base + 8);
            let mut vx = le::read_i64(state, base + 16);
            let mut vy = le::read_i64(state, base + 24);

            px = px.saturating_add(vx);
            py = py.saturating_add(vy);
            bounce(&mut px, &mut vx);
            bounce(&mut py, &mut vy);
            vx = fx::mul(vx, FRICTION);
            vy = fx::mul(vy, FRICTION);

            le::write_i64(state, base, px);
            le::write_i64(state, base + 8, py);
            le::write_i64(state, base + 16, vx);
            le::write_i64(state, base + 24, vy);
        }

        let t = le::read_u64(state, 0);
        le::write_u64(state, 0, t.wrapping_add(1));
        Ok(())
    }
}

/// Reflect off the walls. Clamp afterwards so an absurd velocity can't
/// reflect back out of range and loop.
#[inline]
fn bounce(p: &mut Fx, v: &mut Fx) {
    if *p < ARENA_MIN {
        *p = ARENA_MIN.saturating_add(ARENA_MIN.saturating_sub(*p));
        *v = v.saturating_neg();
    } else if *p > ARENA_MAX {
        *p = ARENA_MAX.saturating_sub(p.saturating_sub(ARENA_MAX));
        *v = v.saturating_neg();
    }
    *p = (*p).clamp(ARENA_MIN, ARENA_MAX);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tick_core::rng::Rng;

    fn input_entry(ball: u32, dvx: i64, dvy: i64) -> [u8; INPUT_ENTRY_SIZE] {
        let mut e = [0u8; INPUT_ENTRY_SIZE];
        e[0..4].copy_from_slice(&ball.to_le_bytes());
        e[4..12].copy_from_slice(&dvx.to_le_bytes());
        e[12..20].copy_from_slice(&dvy.to_le_bytes());
        e
    }

    /// Drive the arena with seeded pseudo-random impulses for `n` ticks.
    fn run(n: u64, seed: u64) -> [u8; STATE_SIZE] {
        let mut state = [0u8; STATE_SIZE];
        Arena::init(&mut state).unwrap();
        let mut rng = Rng::new(seed);
        for t in 0..n {
            // one impulse most ticks, sometimes none
            let inputs = if rng.next_below(8) != 0 {
                let ball = rng.next_below(N_BALLS as u64) as u32;
                let dvx = rng.next_u64() as i64 % fx::from_int(4);
                let dvy = rng.next_u64() as i64 % fx::from_int(4);
                Some(input_entry(ball, dvx, dvy))
            } else {
                None
            };
            let slice: &[u8] = inputs.as_ref().map(|e| &e[..]).unwrap_or(&[]);
            Arena::tick(&mut state, slice, t).unwrap();
        }
        state
    }

    #[test]
    fn two_runs_bit_identical_1m_ticks() {
        let a = run(1_000_000, 0xDEAD_BEEF);
        let b = run(1_000_000, 0xDEAD_BEEF);
        assert_eq!(a, b);
        assert_eq!(le::read_u64(&a, 0), 1_000_000);
    }

    // Frozen FNV-1a of the state after 10k seeded ticks. Catches any
    // semantic drift in fx/rng/arena, not just nondeterminism within one
    // build. Do not update the constant; fix the regression instead.
    #[test]
    fn golden_state_10k_ticks() {
        let state = run(10_000, 7);
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in state.iter() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        assert_eq!(h, GOLDEN_10K, "state hash drifted: {h:#018x}");
    }

    const GOLDEN_10K: u64 = 0xae31_33eb_7ea5_da4c;

    #[test]
    fn positions_stay_in_arena() {
        let state = run(50_000, 3);
        for i in 0..N_BALLS {
            let base = 8 + i * 32;
            let px = le::read_i64(&state, base);
            let py = le::read_i64(&state, base + 8);
            assert!((ARENA_MIN..=ARENA_MAX).contains(&px));
            assert!((ARENA_MIN..=ARENA_MAX).contains(&py));
        }
    }

    #[test]
    fn rejects_bad_input() {
        let mut state = [0u8; STATE_SIZE];
        Arena::init(&mut state).unwrap();
        assert_eq!(Arena::tick(&mut state, &[0u8; 7], 0), Err(TickError::BadInput));
        let e = input_entry(N_BALLS as u32, 1, 1);
        assert_eq!(Arena::tick(&mut state, &e, 0), Err(TickError::BadInput));
    }

    #[test]
    fn rejects_bad_state_size() {
        let mut state = [0u8; STATE_SIZE - 1];
        assert_eq!(Arena::tick(&mut state, &[], 0), Err(TickError::BadStateSize));
    }
}
