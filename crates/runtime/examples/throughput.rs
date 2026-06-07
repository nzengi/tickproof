//! Off-chain throughput: how fast does the engine push ticks through the
//! real program runtime, and what does the raw native tick cost for
//! comparison. Run with --release or the numbers are meaningless.

use std::time::Instant;

use arena::{Arena, INPUT_ENTRY_SIZE, N_BALLS, STATE_SIZE};
use tick_core::rng::Rng;
use tick_core::TickLogic;
use tickproof_runtime::Engine;

const WARMUP: u64 = 1_000;
const MEASURED: u64 = 20_000;

fn random_inputs(rng: &mut Rng) -> Vec<u8> {
    if rng.next_below(8) == 0 {
        return Vec::new();
    }
    let mut e = vec![0u8; INPUT_ENTRY_SIZE];
    let ball = rng.next_below(N_BALLS as u64) as u32;
    let dvx = rng.next_u64() as i64 % tick_core::fx::from_int(4);
    let dvy = rng.next_u64() as i64 % tick_core::fx::from_int(4);
    e[0..4].copy_from_slice(&ball.to_le_bytes());
    e[4..12].copy_from_slice(&dvx.to_le_bytes());
    e[12..20].copy_from_slice(&dvy.to_le_bytes());
    e
}

fn main() {
    let elf_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/arena_program.so"
    );
    let elf = std::fs::read(elf_path).expect("run cargo build-sbf in programs/arena-program");

    let mut genesis = [0u8; STATE_SIZE];
    Arena::init(&mut genesis).unwrap();

    // pre-generate inputs so the rng isn't part of the measurement
    let mut rng = Rng::new(7);
    let inputs: Vec<Vec<u8>> = (0..WARMUP + MEASURED)
        .map(|_| random_inputs(&mut rng))
        .collect();

    // full pipeline: instruction build, program runtime, sbpf vm, state
    // copy-back, input log and chain bookkeeping
    let mut engine = Engine::new(&elf, &genesis);
    for entry in &inputs[..WARMUP as usize] {
        engine.step(entry).unwrap();
    }
    let start = Instant::now();
    let mut cu_total = 0u64;
    for entry in &inputs[WARMUP as usize..] {
        cu_total += engine.step(entry).unwrap();
    }
    let elapsed = start.elapsed();
    let per_tick = elapsed / MEASURED as u32;
    println!(
        "engine (agave runtime): {MEASURED} ticks in {elapsed:.2?} = {:.0} ticks/s, {per_tick:.2?}/tick, avg {} CU",
        MEASURED as f64 / elapsed.as_secs_f64(),
        cu_total / MEASURED,
    );

    // native baseline: the same logic as a plain function call
    let mut state = genesis;
    for (t, entry) in inputs[..WARMUP as usize].iter().enumerate() {
        Arena::tick(&mut state, entry, t as u64).unwrap();
    }
    let start = Instant::now();
    for (t, entry) in inputs[WARMUP as usize..].iter().enumerate() {
        Arena::tick(&mut state, entry, WARMUP + t as u64).unwrap();
    }
    let elapsed = start.elapsed();
    println!(
        "native baseline:        {MEASURED} ticks in {elapsed:.2?} = {:.0} ticks/s",
        MEASURED as f64 / elapsed.as_secs_f64(),
    );
}
