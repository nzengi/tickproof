//! Full dispute lifecycle against the real referee and game programs.
//! The test plays both sides: it runs an honest engine for the ground
//! truth, derives a corrupted trace for whichever party is lying, and
//! drives challenge / bisection / replay through mollusk until the
//! referee settles it.

use std::collections::HashMap;

use arena::{Arena, INPUT_ENTRY_SIZE, N_BALLS, STATE_SIZE};
use mollusk_svm::program::loader_keys::LOADER_V3;
use mollusk_svm::result::InstructionResult;
use mollusk_svm::{Mollusk, MolluskContext};
use referee::{party, status, Claim, Session, CHALLENGE_WINDOW_SLOTS, SESSION_LEN};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use tick_core::rng::Rng;
use tickproof_runtime::{Checkpoint, Engine};

const BOND: u64 = 1_000_000;
const N_TICKS: u64 = 16;

fn elf(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../target/deploy/{name}.so",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path)
        .unwrap_or_else(|_| panic!("{name}.so missing - run cargo build-sbf in programs/ first"))
}

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

fn claim_bytes(c: &Checkpoint) -> Claim {
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&c.state_root);
    out[32..].copy_from_slice(&c.input_chain);
    out
}

/// Ground truth: claim and full state at every tick, plus the input log.
struct Trace {
    claims: Vec<Claim>,
    states: Vec<Vec<u8>>,
    log: Vec<Vec<u8>>,
}

fn honest_trace() -> Trace {
    let mut genesis = [0u8; STATE_SIZE];
    Arena::init(&mut genesis).unwrap();
    let mut engine = Engine::new(&elf("arena_program"), &genesis);
    let mut rng = Rng::new(0xBEEF);

    let mut claims = vec![claim_bytes(&engine.checkpoint())];
    let mut states = vec![genesis.to_vec()];
    for _ in 0..N_TICKS {
        engine.step(&random_inputs(&mut rng)).unwrap();
        claims.push(claim_bytes(&engine.checkpoint()));
        states.push(engine.state_data().to_vec());
    }
    Trace {
        claims,
        states,
        log: engine.input_log().to_vec(),
    }
}

/// A liar's view: correct up to tick k, wrong state roots from there on.
/// The input chain is left intact - the lie is about execution, not about
/// which inputs were played.
fn corrupt_from(trace: &Trace, k: usize) -> Vec<Claim> {
    let mut claims = trace.claims.clone();
    for c in claims.iter_mut().skip(k) {
        c[0] ^= 0xFF;
    }
    claims
}

struct Harness {
    ctx: MolluskContext<HashMap<Pubkey, Account>>,
    referee_id: Pubkey,
    game_id: Pubkey,
    session: Pubkey,
    operator: Pubkey,
    challenger: Pubkey,
    scratch: Pubkey,
}

fn harness() -> Harness {
    let referee_id = Pubkey::new_unique();
    let game_id = Pubkey::new_unique();
    let mut mollusk = Mollusk::default();
    mollusk.add_program_with_loader_and_elf(&referee_id, &LOADER_V3, &elf("referee"));
    mollusk.add_program_with_loader_and_elf(&game_id, &LOADER_V3, &elf("arena_program"));

    let session = Pubkey::new_unique();
    let operator = Pubkey::new_unique();
    let challenger = Pubkey::new_unique();
    let scratch = Pubkey::new_unique();

    let mut store = HashMap::new();
    // funded for rent plus both bonds up front; the referee only checks
    // balances, it doesn't pull transfers itself
    store.insert(
        session,
        Account {
            lamports: 20_000_000 + 2 * BOND,
            data: vec![0u8; SESSION_LEN],
            owner: referee_id,
            ..Account::default()
        },
    );
    for key in [operator, challenger] {
        store.insert(
            key,
            Account {
                lamports: 1_000_000_000,
                ..Account::default()
            },
        );
    }
    store.insert(
        scratch,
        Account {
            lamports: 10_000_000,
            data: vec![0u8; STATE_SIZE],
            owner: game_id,
            ..Account::default()
        },
    );

    Harness {
        ctx: mollusk.with_context(store),
        referee_id,
        game_id,
        session,
        operator,
        challenger,
        scratch,
    }
}

impl Harness {
    fn send(&self, accounts: Vec<AccountMeta>, data: Vec<u8>) -> InstructionResult {
        self.ctx.process_instruction(&Instruction {
            program_id: self.referee_id,
            accounts,
            data,
        })
    }

    fn session(&self) -> Session {
        let store = self.ctx.account_store.borrow();
        Session::read(&store.get(&self.session).unwrap().data).unwrap()
    }

    fn lamports(&self, key: &Pubkey) -> u64 {
        self.ctx.account_store.borrow().get(key).unwrap().lamports
    }

    fn init(&self, genesis: &Claim) {
        let mut data = vec![0u8];
        data.extend_from_slice(&BOND.to_le_bytes());
        data.extend_from_slice(genesis);
        let r = self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new_readonly(self.operator, true),
                AccountMeta::new_readonly(self.game_id, false),
            ],
            data,
        );
        assert!(r.raw_result.is_ok(), "init failed: {:?}", r.raw_result);
    }

    fn checkpoint(&self, tick: u64, claim: &Claim) -> InstructionResult {
        let mut data = vec![1u8];
        data.extend_from_slice(&tick.to_le_bytes());
        data.extend_from_slice(claim);
        self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new_readonly(self.operator, true),
            ],
            data,
        )
    }

    fn finalize(&self) -> InstructionResult {
        self.send(vec![AccountMeta::new(self.session, false)], vec![2u8])
    }

    fn challenge(&self) {
        let r = self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new_readonly(self.challenger, true),
            ],
            vec![3u8],
        );
        assert!(r.raw_result.is_ok(), "challenge failed: {:?}", r.raw_result);
    }

    fn bisect(&self, claim: &Claim) {
        let mut data = vec![4u8];
        data.extend_from_slice(claim);
        let r = self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new_readonly(self.operator, true),
            ],
            data,
        );
        assert!(r.raw_result.is_ok(), "bisect failed: {:?}", r.raw_result);
    }

    fn pick(&self, agree: bool) {
        let r = self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new_readonly(self.challenger, true),
            ],
            vec![5u8, agree as u8],
        );
        assert!(r.raw_result.is_ok(), "pick failed: {:?}", r.raw_result);
    }

    fn replay(&self, pre_state: &[u8], inputs: &[u8]) -> InstructionResult {
        let mut data = vec![6u8];
        data.extend_from_slice(&(inputs.len() as u32).to_le_bytes());
        data.extend_from_slice(inputs);
        data.extend_from_slice(pre_state);
        self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new(self.scratch, true),
                AccountMeta::new_readonly(self.game_id, false),
                AccountMeta::new(self.operator, false),
                AccountMeta::new(self.challenger, false),
            ],
            data,
        )
    }

    fn timeout(&self) -> InstructionResult {
        self.send(
            vec![
                AccountMeta::new(self.session, false),
                AccountMeta::new(self.operator, false),
                AccountMeta::new(self.challenger, false),
            ],
            vec![7u8],
        )
    }

    /// Plays the bisection out: the operator answers with claims from the
    /// asserted trace, the challenger agrees wherever the asserted claim
    /// matches their own view.
    fn run_bisection(&self, asserted: &[Claim], challenger_view: &[Claim]) {
        loop {
            let s = self.session();
            match (s.status, s.turn) {
                (status::BISECTING, party::OPERATOR) => {
                    let mid = s.lo_tick + (s.hi_tick - s.lo_tick) / 2;
                    self.bisect(&asserted[mid as usize]);
                }
                (status::BISECTING, party::CHALLENGER) => {
                    let agree = s.mid_claim == challenger_view[s.mid_tick as usize];
                    self.pick(agree);
                }
                _ => return,
            }
        }
    }
}

#[test]
fn cheating_operator_loses_dispute() {
    let trace = honest_trace();
    let lie_at = 11usize;
    let asserted = corrupt_from(&trace, lie_at);

    let h = harness();
    h.init(&trace.claims[0]);
    let r = h.checkpoint(N_TICKS, &asserted[N_TICKS as usize]);
    assert!(r.raw_result.is_ok(), "checkpoint failed: {:?}", r.raw_result);
    h.challenge();
    h.run_bisection(&asserted, &trace.claims);

    let s = h.session();
    assert_eq!(s.status, status::AWAITING_REPLAY);
    assert_eq!(s.lo_tick, lie_at as u64 - 1, "bisection missed the lie");
    assert_eq!(s.hi_tick, lie_at as u64);

    let challenger_before = h.lamports(&h.challenger);
    let r = h.replay(&trace.states[s.lo_tick as usize], &trace.log[s.lo_tick as usize]);
    assert!(r.raw_result.is_ok(), "replay failed: {:?}", r.raw_result);
    println!("replay instruction CU: {}", r.compute_units_consumed);

    let s = h.session();
    assert_eq!(s.status, status::RESOLVED);
    assert_eq!(s.winner, party::CHALLENGER);
    assert_eq!(h.lamports(&h.challenger), challenger_before + 2 * BOND);
}

#[test]
fn honest_operator_wins_dispute() {
    let trace = honest_trace();
    // this time the challenger is the one with the broken view
    let challenger_view = corrupt_from(&trace, 5);

    let h = harness();
    h.init(&trace.claims[0]);
    let r = h.checkpoint(N_TICKS, &trace.claims[N_TICKS as usize]);
    assert!(r.raw_result.is_ok());
    h.challenge();
    h.run_bisection(&trace.claims, &challenger_view);

    let s = h.session();
    assert_eq!(s.status, status::AWAITING_REPLAY);
    assert_eq!(s.lo_tick, 4);

    let operator_before = h.lamports(&h.operator);
    let r = h.replay(&trace.states[s.lo_tick as usize], &trace.log[s.lo_tick as usize]);
    assert!(r.raw_result.is_ok(), "replay failed: {:?}", r.raw_result);

    let s = h.session();
    assert_eq!(s.winner, party::OPERATOR);
    assert_eq!(h.lamports(&h.operator), operator_before + 2 * BOND);
}

#[test]
fn silent_operator_forfeits_by_timeout() {
    let trace = honest_trace();
    let asserted = corrupt_from(&trace, 3);

    let mut h = harness();
    h.init(&trace.claims[0]);
    h.checkpoint(N_TICKS, &asserted[N_TICKS as usize]);
    h.challenge();

    // operator never shows up for their bisection move
    let deadline = h.session().deadline;
    let early = h.timeout();
    assert!(early.raw_result.is_err(), "timeout landed before deadline");

    h.ctx.mollusk.warp_to_slot(deadline + 1);
    let r = h.timeout();
    assert!(r.raw_result.is_ok(), "timeout failed: {:?}", r.raw_result);
    assert_eq!(h.session().winner, party::CHALLENGER);
}

#[test]
fn finalize_needs_the_window_to_pass() {
    let trace = honest_trace();

    let mut h = harness();
    h.init(&trace.claims[0]);
    h.checkpoint(N_TICKS, &trace.claims[N_TICKS as usize]);

    assert!(h.finalize().raw_result.is_err(), "finalized inside window");

    h.ctx.mollusk.warp_to_slot(CHALLENGE_WINDOW_SLOTS + 1);
    assert!(h.finalize().raw_result.is_ok());
    let s = h.session();
    assert_eq!(s.lo_tick, N_TICKS);

    // with the head finalized the operator can assert again, and the
    // finalized range can no longer be challenged
    let r = h.checkpoint(N_TICKS + 16, &trace.claims[N_TICKS as usize]);
    assert!(r.raw_result.is_ok());
}
