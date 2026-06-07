//! Watch the arena live in the terminal. Every frame went through the
//! actual agave program runtime: this is the same engine that produces
//! checkpoints, pointed at a screen instead of a log. The HUD shows the
//! state root that would go into a claim at the current tick.
//!
//! Arrow keys kick ball 0, space kicks a random ball, q quits.

use std::io::{stdout, Write};
use std::time::{Duration, Instant};

use arena::{Arena, INPUT_ENTRY_SIZE, N_BALLS, STATE_SIZE};
use crossterm::event::{self, Event, KeyCode};
use crossterm::{cursor, execute, queue, style, terminal};
use tick_core::{fx, rng::Rng};
use tickproof_runtime::Engine;

const TICK_RATE: u64 = 60;
const ARENA_SIDE: f64 = 256.0;

fn input_entry(ball: u32, dvx: i64, dvy: i64) -> Vec<u8> {
    let mut e = vec![0u8; INPUT_ENTRY_SIZE];
    e[0..4].copy_from_slice(&ball.to_le_bytes());
    e[4..12].copy_from_slice(&dvx.to_le_bytes());
    e[12..20].copy_from_slice(&dvy.to_le_bytes());
    e
}

/// Display only - the engine itself never touches floats.
fn ball_positions(state: &[u8]) -> Vec<(f64, f64)> {
    let scale = (1u64 << 32) as f64;
    (0..N_BALLS)
        .map(|i| {
            let off = 8 + i * 32;
            let x = i64::from_le_bytes(state[off..off + 8].try_into().unwrap());
            let y = i64::from_le_bytes(state[off + 8..off + 16].try_into().unwrap());
            (x as f64 / scale, y as f64 / scale)
        })
        .collect()
}

fn main() -> std::io::Result<()> {
    let elf_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/arena_program.so"
    );
    let elf = std::fs::read(elf_path)
        .expect("arena_program.so missing - run cargo build-sbf in programs/arena-program");

    let mut genesis = [0u8; STATE_SIZE];
    Arena::init(&mut genesis).unwrap();
    let mut engine = Engine::new(&elf, &genesis);
    let mut rng = Rng::new(0x5EED);

    let mut out = stdout();
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;

    let tick_interval = Duration::from_micros(1_000_000 / TICK_RATE);
    let started = Instant::now();
    let mut next_tick = Instant::now();
    let mut last_cu = 0u64;
    let kick = fx::from_int(3);

    'run: loop {
        // collect this tick's inputs from the keyboard
        let mut inputs = Vec::new();
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break 'run,
                    KeyCode::Up => inputs.extend(input_entry(0, 0, -kick)),
                    KeyCode::Down => inputs.extend(input_entry(0, 0, kick)),
                    KeyCode::Left => inputs.extend(input_entry(0, -kick, 0)),
                    KeyCode::Right => inputs.extend(input_entry(0, kick, 0)),
                    KeyCode::Char(' ') => {
                        let ball = rng.next_below(N_BALLS as u64) as u32;
                        let dvx = rng.next_u64() as i64 % fx::from_int(5);
                        let dvy = rng.next_u64() as i64 % fx::from_int(5);
                        inputs.extend(input_entry(ball, dvx, dvy));
                    }
                    _ => {}
                }
            }
        }

        last_cu = engine.step(&inputs).expect("tick rejected");

        // draw at half the tick rate; the terminal can't keep up with 60
        if engine.tick() % 2 == 0 {
            let (cols, rows) = terminal::size()?;
            let w = (cols.saturating_sub(2)).min(96) as usize;
            let h = (rows.saturating_sub(4)).min(40) as usize;

            queue!(out, terminal::Clear(terminal::ClearType::All))?;
            queue!(out, cursor::MoveTo(0, 0))?;
            queue!(out, style::Print(format!("+{}+", "-".repeat(w))))?;
            for row in 0..h {
                queue!(out, cursor::MoveTo(0, row as u16 + 1))?;
                queue!(out, style::Print(format!("|{}|", " ".repeat(w))))?;
            }
            queue!(out, cursor::MoveTo(0, h as u16 + 1))?;
            queue!(out, style::Print(format!("+{}+", "-".repeat(w))))?;

            for (i, (x, y)) in ball_positions(engine.state_data()).iter().enumerate() {
                let cx = 1 + (x / ARENA_SIDE * (w - 1) as f64) as u16;
                let cy = 1 + (y / ARENA_SIDE * (h - 1) as f64) as u16;
                queue!(out, cursor::MoveTo(cx, cy))?;
                queue!(out, style::Print(i.to_string()))?;
            }

            let root = engine.state_root();
            let rate = engine.tick() as f64 / started.elapsed().as_secs_f64();
            queue!(out, cursor::MoveTo(0, h as u16 + 2))?;
            queue!(
                out,
                style::Print(format!(
                    "tick {}  {} CU  {:.0} t/s  root {:02x}{:02x}{:02x}{:02x}..",
                    engine.tick(),
                    last_cu,
                    rate,
                    root[0],
                    root[1],
                    root[2],
                    root[3],
                ))
            )?;
            queue!(out, cursor::MoveTo(0, h as u16 + 3))?;
            queue!(
                out,
                style::Print("arrows kick ball 0, space kicks a random ball, q quits")
            )?;
            out.flush()?;
        }

        next_tick += tick_interval;
        let now = Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else {
            // fell behind (slow terminal); don't try to catch up in a burst
            next_tick = now;
        }
    }

    execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
