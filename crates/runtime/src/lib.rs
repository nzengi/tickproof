//! Off-chain engine. Drives a tick program through mollusk, i.e. through
//! the actual agave program runtime and sBPF VM - not a reimplementation.
//! Keeps the input log and per-tick CU counts; both are part of what gets
//! committed and replayed in a dispute.

use mollusk_svm::program::loader_keys::LOADER_V3;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

pub mod log;

#[derive(Debug)]
pub enum EngineError {
    /// The program rejected the tick. Carries the tick index.
    TickFailed(u64),
}

/// What goes on-chain: at tick `tick` the state hashed to `state_root`,
/// having consumed inputs committed by `input_chain`. A dispute bisects
/// between two of these and replays the single tick where the parties
/// diverge; the chain is what stops the asserter from inventing inputs
/// at replay time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub tick: u64,
    pub state_root: [u8; 32],
    pub input_chain: [u8; 32],
}

pub struct Engine {
    mollusk: Mollusk,
    program_id: Pubkey,
    state_key: Pubkey,
    state: Account,
    tick: u64,
    input_log: Vec<Vec<u8>>,
    input_chain: [u8; 32],
}

impl Engine {
    /// `elf` is the compiled tick program, `initial_state` the genesis
    /// game state (must match the program's expected state size).
    pub fn new(elf: &[u8], initial_state: &[u8]) -> Self {
        let program_id = Pubkey::new_unique();
        let mut mollusk = Mollusk::default();
        mollusk.add_program_with_loader_and_elf(&program_id, &LOADER_V3, elf);

        let state_key = Pubkey::new_unique();
        let state = Account {
            lamports: 1_000_000_000,
            data: initial_state.to_vec(),
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        };

        Self {
            mollusk,
            program_id,
            state_key,
            state,
            tick: 0,
            input_log: Vec::new(),
            input_chain: [0u8; 32],
        }
    }

    /// Advance one tick. Returns the CUs the program consumed.
    pub fn step(&mut self, inputs: &[u8]) -> Result<u64, EngineError> {
        let mut data = Vec::with_capacity(9 + inputs.len());
        data.push(0); // Tick instruction
        data.extend_from_slice(&self.tick.to_le_bytes());
        data.extend_from_slice(inputs);

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![AccountMeta::new(self.state_key, false)],
            data,
        };
        let result = self
            .mollusk
            .process_instruction(&ix, &[(self.state_key, self.state.clone())]);

        if result.raw_result.is_err() {
            return Err(EngineError::TickFailed(self.tick));
        }
        let (_, account) = result
            .resulting_accounts
            .iter()
            .find(|(k, _)| *k == self.state_key)
            .expect("state account present in results");
        self.state = account.clone();

        self.input_log.push(inputs.to_vec());
        self.input_chain = tick_merkle::extend_input_chain(&self.input_chain, inputs);
        self.tick += 1;
        Ok(result.compute_units_consumed)
    }

    /// Rebuild an engine from genesis plus a recorded input log. Stops at
    /// the first failing tick, which in a dispute is itself the answer.
    pub fn replay(elf: &[u8], initial_state: &[u8], log: &[Vec<u8>]) -> Result<Self, EngineError> {
        let mut engine = Self::new(elf, initial_state);
        for entry in log {
            engine.step(entry)?;
        }
        Ok(engine)
    }

    pub fn state_root(&self) -> [u8; 32] {
        tick_merkle::state_root(&self.state.data)
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            tick: self.tick,
            state_root: self.state_root(),
            input_chain: self.input_chain,
        }
    }

    pub fn state_data(&self) -> &[u8] {
        &self.state.data
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn input_log(&self) -> &[Vec<u8>] {
        &self.input_log
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena::{Arena, INPUT_ENTRY_SIZE, STATE_SIZE};
    use tick_core::rng::Rng;
    use tick_core::TickLogic;

    fn arena_elf() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/deploy/arena_program.so"
        );
        std::fs::read(path).expect(
            "arena_program.so not found - run `cargo build-sbf` in programs/arena-program first",
        )
    }

    fn random_inputs(rng: &mut Rng) -> Vec<u8> {
        if rng.next_below(8) == 0 {
            return Vec::new();
        }
        let mut e = vec![0u8; INPUT_ENTRY_SIZE];
        let ball = rng.next_below(arena::N_BALLS as u64) as u32;
        let dvx = rng.next_u64() as i64 % tick_core::fx::from_int(4);
        let dvy = rng.next_u64() as i64 % tick_core::fx::from_int(4);
        e[0..4].copy_from_slice(&ball.to_le_bytes());
        e[4..12].copy_from_slice(&dvx.to_le_bytes());
        e[12..20].copy_from_slice(&dvy.to_le_bytes());
        e
    }

    // The core claim, in miniature: the SBF build running under the real
    // program runtime produces bit-identical state to the native build.
    #[test]
    fn sbf_matches_native_1000_ticks() {
        let mut native = [0u8; STATE_SIZE];
        Arena::init(&mut native).unwrap();
        let mut engine = Engine::new(&arena_elf(), &native);

        let mut rng = Rng::new(0xC0FFEE);
        for t in 0..1000u64 {
            let inputs = random_inputs(&mut rng);
            Arena::tick(&mut native, &inputs, t).unwrap();
            engine.step(&inputs).unwrap();
            if t % 100 == 0 {
                assert_eq!(engine.state_data(), &native[..], "diverged at tick {t}");
            }
        }
        assert_eq!(engine.state_data(), &native[..]);
        assert_eq!(engine.tick(), 1000);
        assert_eq!(engine.input_log().len(), 1000);
    }

    // A tick has to fit an on-chain transaction with plenty of headroom,
    // otherwise single-tick replay stops being possible and the whole
    // design falls apart.
    #[test]
    fn tick_cu_well_under_budget() {
        let mut state = [0u8; STATE_SIZE];
        Arena::init(&mut state).unwrap();
        let mut engine = Engine::new(&arena_elf(), &state);

        let mut rng = Rng::new(1);
        let mut max_cu = 0u64;
        for _ in 0..100 {
            let inputs = random_inputs(&mut rng);
            let cu = engine.step(&inputs).unwrap();
            max_cu = max_cu.max(cu);
        }
        println!("max CU per tick over 100 ticks: {max_cu}");
        assert!(max_cu < 100_000, "tick too expensive: {max_cu} CU");
    }

    // Persist the log, read it back, replay from genesis: the replayed
    // engine has to land on the exact checkpoint the live one produced.
    // This is the recovery path a dispute (or a crashed node) relies on.
    #[test]
    fn log_roundtrip_replays_to_same_checkpoint() {
        let mut genesis = [0u8; STATE_SIZE];
        Arena::init(&mut genesis).unwrap();
        let elf = arena_elf();
        let mut live = Engine::new(&elf, &genesis);

        let mut rng = Rng::new(0xD1CE);
        for _ in 0..200 {
            live.step(&random_inputs(&mut rng)).unwrap();
        }

        let mut buf = Vec::new();
        log::write_log(&mut buf, live.input_log()).unwrap();
        let recovered = log::read_log(std::io::Cursor::new(buf)).unwrap();
        let replayed = Engine::replay(&elf, &genesis, &recovered).unwrap();

        assert_eq!(replayed.checkpoint(), live.checkpoint());
        assert_eq!(replayed.state_data(), live.state_data());
    }

    #[test]
    fn checkpoint_tracks_state() {
        let mut genesis = [0u8; STATE_SIZE];
        Arena::init(&mut genesis).unwrap();
        let mut engine = Engine::new(&arena_elf(), &genesis);

        let before = engine.checkpoint();
        assert_eq!(before.tick, 0);
        assert_eq!(before.state_root, tick_merkle::state_root(&genesis));
        assert_eq!(before.input_chain, [0u8; 32]);

        engine.step(&[]).unwrap();
        let after = engine.checkpoint();
        assert_eq!(after.tick, 1);
        assert_ne!(after.state_root, before.state_root);
        assert_ne!(after.input_chain, before.input_chain);
    }

    #[test]
    fn bad_input_rejected_by_program() {
        let mut state = [0u8; STATE_SIZE];
        Arena::init(&mut state).unwrap();
        let mut engine = Engine::new(&arena_elf(), &state);
        let garbage = vec![0u8; 7];
        assert!(matches!(
            engine.step(&garbage),
            Err(EngineError::TickFailed(0))
        ));
    }
}
