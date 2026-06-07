//! Off-chain engine. Drives a tick program through mollusk, i.e. through
//! the actual agave program runtime and sBPF VM - not a reimplementation.
//! Keeps the input log and per-tick CU counts; both are part of what gets
//! committed and replayed in a dispute.

use mollusk_svm::program::loader_keys::LOADER_V3;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

#[derive(Debug)]
pub enum EngineError {
    /// The program rejected the tick. Carries the tick index.
    TickFailed(u64),
}

pub struct Engine {
    mollusk: Mollusk,
    program_id: Pubkey,
    state_key: Pubkey,
    state: Account,
    tick: u64,
    input_log: Vec<Vec<u8>>,
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
        }
    }

    /// Advance one tick. Returns the CUs the program consumed.
    pub fn step(&mut self, inputs: &[u8]) -> Result<u64, EngineError> {
        let mut data = Vec::with_capacity(8 + inputs.len());
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
        self.tick += 1;
        Ok(result.compute_units_consumed)
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
