//! Deterministic tick primitives. The same crate compiles to SBF and to
//! native, so game logic built on it replays bit-identically on-chain.
//!
//! Hard rules: no_std, no deps, no floats, no heap. State and inputs are
//! caller-provided byte slices; on-chain they live in account data.

#![no_std]
#![deny(clippy::float_arithmetic)]
#![deny(unsafe_code)]

pub mod fx;
pub mod rng;

/// A pure state transition: `state' = f(state, inputs, tick_index)`.
///
/// No clock, no host entropy, no reads outside `state`/`inputs`. The
/// off-chain runtime and the on-chain referee both call this exact code.
pub trait TickLogic {
    /// Fixed byte size of the state this logic operates on.
    const STATE_SIZE: usize;

    /// Advance `state` by one tick. `inputs` is the ordered input log
    /// entry for `tick_index`, already sequenced by the runtime.
    fn tick(state: &mut [u8], inputs: &[u8], tick_index: u64) -> Result<(), TickError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickError {
    BadStateSize,
    BadInput,
}

/// Little-endian field access. SBF is little-endian anyway, but spelling it
/// out keeps the state layout independent of the host and avoids unsafe.
pub mod le {
    #[inline]
    pub fn read_i64(b: &[u8], off: usize) -> i64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&b[off..off + 8]);
        i64::from_le_bytes(buf)
    }

    #[inline]
    pub fn write_i64(b: &mut [u8], off: usize, v: i64) {
        b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub fn read_u64(b: &[u8], off: usize) -> u64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&b[off..off + 8]);
        u64::from_le_bytes(buf)
    }

    #[inline]
    pub fn write_u64(b: &mut [u8], off: usize, v: u64) {
        b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub fn read_u32(b: &[u8], off: usize) -> u32 {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&b[off..off + 4]);
        u32::from_le_bytes(buf)
    }

    #[inline]
    pub fn write_u32(b: &mut [u8], off: usize, v: u32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
}
