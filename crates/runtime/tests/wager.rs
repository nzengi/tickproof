//! Full wager lifecycle against the real wager, referee and game
//! programs. The matches here are actual two-player arena games driven
//! through the engine - player A steers ball 0, player B ball 1 - and
//! every settlement path is exercised: cooperative signing, the referee
//! proof, a cheating reporter losing the dispute and the honest player
//! settling anyway, and the deadline refund.

use std::collections::HashMap;

use arena::{side, Arena, INPUT_ENTRY_SIZE, STATE_SIZE};
use mollusk_svm::program::loader_keys::LOADER_V3;
use mollusk_svm::result::InstructionResult;
use mollusk_svm::{Mollusk, MolluskContext};
use referee::{
    party, status as referee_status, Claim, Session, CHALLENGE_WINDOW_SLOTS, SESSION_LEN,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use tick_core::rng::Rng;
use tickproof_runtime::{Checkpoint, Engine};
use wager::{phase, Match, MATCH_LEN};

const STAKE: u64 = 5_000_000;
const BOND: u64 = 1_000_000;
const DEADLINE_SLOTS: u64 = 10_000;
const N_TICKS: u64 = 24;

fn elf(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../target/deploy/{name}.so",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path)
        .unwrap_or_else(|_| panic!("{name}.so missing - run cargo build-sbf in programs/ first"))
}

/// One impulse from each player most ticks: A kicks ball 0, B ball 1.
fn match_inputs(rng: &mut Rng) -> Vec<u8> {
    let mut inputs = Vec::new();
    for ball in [0u32, 1u32] {
        if rng.next_below(4) == 0 {
            continue;
        }
        let mut e = [0u8; INPUT_ENTRY_SIZE];
        let dvx = rng.next_u64() as i64 % tick_core::fx::from_int(4);
        let dvy = rng.next_u64() as i64 % tick_core::fx::from_int(4);
        e[0..4].copy_from_slice(&ball.to_le_bytes());
        e[4..12].copy_from_slice(&dvx.to_le_bytes());
        e[12..20].copy_from_slice(&dvy.to_le_bytes());
        inputs.extend_from_slice(&e);
    }
    inputs
}

fn claim_bytes(c: &Checkpoint) -> Claim {
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&c.state_root);
    out[32..].copy_from_slice(&c.input_chain);
    out
}

struct Trace {
    claims: Vec<Claim>,
    states: Vec<Vec<u8>>,
    log: Vec<Vec<u8>>,
}

fn honest_trace(seed: u64) -> Trace {
    let mut genesis = [0u8; STATE_SIZE];
    Arena::init(&mut genesis).unwrap();
    let mut engine = Engine::new(&elf("arena_program"), &genesis);
    let mut rng = Rng::new(seed);

    let mut claims = vec![claim_bytes(&engine.checkpoint())];
    let mut states = vec![genesis.to_vec()];
    for _ in 0..N_TICKS {
        engine.step(&match_inputs(&mut rng)).unwrap();
        claims.push(claim_bytes(&engine.checkpoint()));
        states.push(engine.state_data().to_vec());
    }
    Trace {
        claims,
        states,
        log: engine.input_log().to_vec(),
    }
}

fn corrupt_from(trace: &Trace, k: usize) -> Vec<Claim> {
    let mut claims = trace.claims.clone();
    for c in claims.iter_mut().skip(k) {
        c[0] ^= 0xFF;
    }
    claims
}

struct Harness {
    ctx: MolluskContext<HashMap<Pubkey, Account>>,
    wager_id: Pubkey,
    referee_id: Pubkey,
    game_id: Pubkey,
    game_match: Pubkey,
    player_a: Pubkey,
    player_b: Pubkey,
    session_a: Pubkey,
    session_b: Pubkey,
    scratch: Pubkey,
}

fn harness() -> Harness {
    let wager_id = Pubkey::new_unique();
    let referee_id = Pubkey::new_unique();
    let game_id = Pubkey::new_unique();
    let mut mollusk = Mollusk::default();
    mollusk.add_program_with_loader_and_elf(&wager_id, &LOADER_V3, &elf("wager"));
    mollusk.add_program_with_loader_and_elf(&referee_id, &LOADER_V3, &elf("referee"));
    mollusk.add_program_with_loader_and_elf(&game_id, &LOADER_V3, &elf("arena_program"));

    let game_match = Pubkey::new_unique();
    let player_a = Pubkey::new_unique();
    let player_b = Pubkey::new_unique();
    let session_a = Pubkey::new_unique();
    let session_b = Pubkey::new_unique();
    let scratch = Pubkey::new_unique();

    let mut store = HashMap::new();
    // funded for rent plus both stakes up front; the wager program only
    // checks balances, it doesn't pull transfers itself
    store.insert(
        game_match,
        Account {
            lamports: 20_000_000 + 2 * STAKE,
            data: vec![0u8; MATCH_LEN],
            owner: wager_id,
            ..Account::default()
        },
    );
    // sessions likewise carry rent plus both bonds
    for key in [session_a, session_b] {
        store.insert(
            key,
            Account {
                lamports: 20_000_000 + 2 * BOND,
                data: vec![0u8; SESSION_LEN],
                owner: referee_id,
                ..Account::default()
            },
        );
    }
    for key in [player_a, player_b] {
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
        wager_id,
        referee_id,
        game_id,
        game_match,
        player_a,
        player_b,
        session_a,
        session_b,
        scratch,
    }
}

impl Harness {
    fn send_to(&self, program: Pubkey, accounts: Vec<AccountMeta>, data: Vec<u8>) -> InstructionResult {
        self.ctx.process_instruction(&Instruction {
            program_id: program,
            accounts,
            data,
        })
    }

    fn send(&self, accounts: Vec<AccountMeta>, data: Vec<u8>) -> InstructionResult {
        self.send_to(self.wager_id, accounts, data)
    }

    fn game_match(&self) -> Match {
        let store = self.ctx.account_store.borrow();
        Match::read(&store.get(&self.game_match).unwrap().data).unwrap()
    }

    fn session(&self, key: &Pubkey) -> Session {
        let store = self.ctx.account_store.borrow();
        Session::read(&store.get(key).unwrap().data).unwrap()
    }

    fn lamports(&self, key: &Pubkey) -> u64 {
        self.ctx.account_store.borrow().get(key).unwrap().lamports
    }

    fn create(&self, genesis: &Claim) {
        let mut data = vec![0u8];
        data.extend_from_slice(&STAKE.to_le_bytes());
        data.extend_from_slice(&N_TICKS.to_le_bytes());
        data.extend_from_slice(&DEADLINE_SLOTS.to_le_bytes());
        data.extend_from_slice(genesis);
        let r = self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new_readonly(self.player_a, true),
                AccountMeta::new_readonly(self.player_b, false),
                AccountMeta::new_readonly(self.game_id, false),
                AccountMeta::new_readonly(self.referee_id, false),
            ],
            data,
        );
        assert!(r.raw_result.is_ok(), "create failed: {:?}", r.raw_result);
    }

    fn join(&self) -> InstructionResult {
        self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new_readonly(self.player_b, true),
            ],
            vec![1u8],
        )
    }

    fn cancel(&self) -> InstructionResult {
        self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new(self.player_a, true),
            ],
            vec![2u8],
        )
    }

    fn bind(&self, player: Pubkey, session: Pubkey) -> InstructionResult {
        self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new_readonly(player, true),
                AccountMeta::new_readonly(session, false),
            ],
            vec![3u8],
        )
    }

    fn settle_coop(&self, winner: u8, b_signs: bool) -> InstructionResult {
        self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new(self.player_a, true),
                AccountMeta::new(self.player_b, b_signs),
            ],
            vec![4u8, winner],
        )
    }

    fn settle(&self, session: Pubkey, state: &[u8]) -> InstructionResult {
        let mut data = vec![5u8];
        data.extend_from_slice(state);
        self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new_readonly(session, false),
                AccountMeta::new(self.scratch, true),
                AccountMeta::new_readonly(self.game_id, false),
                AccountMeta::new(self.player_a, false),
                AccountMeta::new(self.player_b, false),
            ],
            data,
        )
    }

    fn expire(&self) -> InstructionResult {
        self.send(
            vec![
                AccountMeta::new(self.game_match, false),
                AccountMeta::new(self.player_a, false),
                AccountMeta::new(self.player_b, false),
            ],
            vec![6u8],
        )
    }

    // referee moves, operator is whichever player runs the session

    fn referee_init(&self, session: Pubkey, operator: Pubkey, genesis: &Claim) {
        let mut data = vec![0u8];
        data.extend_from_slice(&BOND.to_le_bytes());
        data.extend_from_slice(genesis);
        let r = self.send_to(
            self.referee_id,
            vec![
                AccountMeta::new(session, false),
                AccountMeta::new_readonly(operator, true),
                AccountMeta::new_readonly(self.game_id, false),
            ],
            data,
        );
        assert!(r.raw_result.is_ok(), "referee init failed: {:?}", r.raw_result);
    }

    fn referee_checkpoint(&self, session: Pubkey, operator: Pubkey, tick: u64, claim: &Claim) {
        let mut data = vec![1u8];
        data.extend_from_slice(&tick.to_le_bytes());
        data.extend_from_slice(claim);
        let r = self.send_to(
            self.referee_id,
            vec![
                AccountMeta::new(session, false),
                AccountMeta::new_readonly(operator, true),
            ],
            data,
        );
        assert!(r.raw_result.is_ok(), "checkpoint failed: {:?}", r.raw_result);
    }

    fn referee_finalize(&self, session: Pubkey) -> InstructionResult {
        self.send_to(
            self.referee_id,
            vec![AccountMeta::new(session, false)],
            vec![2u8],
        )
    }

    fn referee_challenge(&self, session: Pubkey, challenger: Pubkey) {
        let r = self.send_to(
            self.referee_id,
            vec![
                AccountMeta::new(session, false),
                AccountMeta::new_readonly(challenger, true),
            ],
            vec![3u8],
        );
        assert!(r.raw_result.is_ok(), "challenge failed: {:?}", r.raw_result);
    }

    /// Bisection played out between an asserted trace and the
    /// challenger's view, exactly as in the dispute tests.
    fn referee_bisection(
        &self,
        session: Pubkey,
        operator: Pubkey,
        challenger: Pubkey,
        asserted: &[Claim],
        challenger_view: &[Claim],
    ) {
        loop {
            let s = self.session(&session);
            match (s.status, s.turn) {
                (referee_status::BISECTING, party::OPERATOR) => {
                    let mid = s.lo_tick + (s.hi_tick - s.lo_tick) / 2;
                    let mut data = vec![4u8];
                    data.extend_from_slice(&asserted[mid as usize]);
                    let r = self.send_to(
                        self.referee_id,
                        vec![
                            AccountMeta::new(session, false),
                            AccountMeta::new_readonly(operator, true),
                        ],
                        data,
                    );
                    assert!(r.raw_result.is_ok(), "bisect failed: {:?}", r.raw_result);
                }
                (referee_status::BISECTING, party::CHALLENGER) => {
                    let s = self.session(&session);
                    let agree = s.mid_claim == challenger_view[s.mid_tick as usize];
                    let r = self.send_to(
                        self.referee_id,
                        vec![
                            AccountMeta::new(session, false),
                            AccountMeta::new_readonly(challenger, true),
                        ],
                        vec![5u8, agree as u8],
                    );
                    assert!(r.raw_result.is_ok(), "pick failed: {:?}", r.raw_result);
                }
                _ => return,
            }
        }
    }

    fn referee_replay(
        &self,
        session: Pubkey,
        operator: Pubkey,
        challenger: Pubkey,
        pre_state: &[u8],
        inputs: &[u8],
    ) {
        let mut data = vec![6u8];
        data.extend_from_slice(&(inputs.len() as u32).to_le_bytes());
        data.extend_from_slice(inputs);
        data.extend_from_slice(pre_state);
        let r = self.send_to(
            self.referee_id,
            vec![
                AccountMeta::new(session, false),
                AccountMeta::new(self.scratch, true),
                AccountMeta::new_readonly(self.game_id, false),
                AccountMeta::new(operator, false),
                AccountMeta::new(challenger, false),
            ],
            data,
        );
        assert!(r.raw_result.is_ok(), "replay failed: {:?}", r.raw_result);
    }

    /// Assert, wait out the challenge window, finalize.
    fn prove_unchallenged(&mut self, session: Pubkey, operator: Pubkey, trace: &Trace) {
        self.referee_checkpoint(session, operator, N_TICKS, &trace.claims[N_TICKS as usize]);
        let slot = self.ctx.mollusk.sysvars.clock.slot;
        self.ctx.mollusk.warp_to_slot(slot + CHALLENGE_WINDOW_SLOTS + 1);
        let r = self.referee_finalize(session);
        assert!(r.raw_result.is_ok(), "finalize failed: {:?}", r.raw_result);
    }
}

#[test]
fn coop_settle_pays_the_winner() {
    let trace = honest_trace(0xA11CE);
    let h = harness();
    h.create(&trace.claims[0]);
    assert!(h.join().raw_result.is_ok());
    assert_eq!(h.game_match().phase, phase::LIVE);

    let a_before = h.lamports(&h.player_a);
    let r = h.settle_coop(wager::side::A, true);
    assert!(r.raw_result.is_ok(), "coop settle failed: {:?}", r.raw_result);

    let m = h.game_match();
    assert_eq!(m.phase, phase::SETTLED);
    assert_eq!(m.winner, wager::side::A);
    assert_eq!(h.lamports(&h.player_a), a_before + 2 * STAKE);
}

#[test]
fn coop_settle_needs_both_signatures() {
    let trace = honest_trace(0xA11CE);
    let h = harness();
    h.create(&trace.claims[0]);
    h.join();
    assert!(h.settle_coop(wager::side::A, false).raw_result.is_err());
}

#[test]
fn referee_proof_settles_the_match() {
    let trace = honest_trace(0xB0B);
    let mut h = harness();
    h.create(&trace.claims[0]);
    h.join();

    // player A runs the session and proves the result on-chain
    h.referee_init(h.session_a, h.player_a, &trace.claims[0]);
    let r = h.bind(h.player_a, h.session_a);
    assert!(r.raw_result.is_ok(), "bind failed: {:?}", r.raw_result);

    // nothing settles while the claim is merely asserted
    h.referee_checkpoint(h.session_a, h.player_a, N_TICKS, &trace.claims[N_TICKS as usize]);
    let final_state = &trace.states[N_TICKS as usize];
    assert!(h.settle(h.session_a, final_state).raw_result.is_err());

    let slot = h.ctx.mollusk.sysvars.clock.slot;
    h.ctx.mollusk.warp_to_slot(slot + CHALLENGE_WINDOW_SLOTS + 1);
    assert!(h.referee_finalize(h.session_a).raw_result.is_ok());

    // a state that doesn't hash to the proven claim is rejected
    let mut wrong = final_state.clone();
    wrong[8] ^= 1;
    assert!(h.settle(h.session_a, &wrong).raw_result.is_err());

    let expected = arena::verdict(final_state).unwrap();
    assert_ne!(expected, side::DRAW, "seed produced a draw, pick another");
    let winner_key = if expected == side::FIRST { h.player_a } else { h.player_b };
    let before = h.lamports(&winner_key);

    let r = h.settle(h.session_a, final_state);
    assert!(r.raw_result.is_ok(), "settle failed: {:?}", r.raw_result);
    println!("settle instruction CU: {}", r.compute_units_consumed);

    let m = h.game_match();
    assert_eq!(m.phase, phase::SETTLED);
    assert_eq!(m.winner, expected);
    assert_eq!(h.lamports(&winner_key), before + 2 * STAKE);
}

// The headline scenario: A lies about the result, B catches it through
// the referee, the chain replays the disputed tick natively, and B still
// collects the pot through their own honest session. At no point does
// anything trust a reporter.
#[test]
fn cheating_reporter_loses_dispute_and_match_settles_honestly() {
    let trace = honest_trace(0xC4EA7);
    let lie_at = 17usize;
    let asserted = corrupt_from(&trace, lie_at);

    let mut h = harness();
    h.create(&trace.claims[0]);
    h.join();

    // A binds a session and asserts a corrupted final claim
    h.referee_init(h.session_a, h.player_a, &trace.claims[0]);
    assert!(h.bind(h.player_a, h.session_a).raw_result.is_ok());
    h.referee_checkpoint(h.session_a, h.player_a, N_TICKS, &asserted[N_TICKS as usize]);

    // B challenges and corners the lie; the chain replays the tick
    h.referee_challenge(h.session_a, h.player_b);
    h.referee_bisection(h.session_a, h.player_a, h.player_b, &asserted, &trace.claims);
    let s = h.session(&h.session_a);
    assert_eq!(s.status, referee_status::AWAITING_REPLAY);
    assert_eq!(s.lo_tick, lie_at as u64 - 1, "bisection missed the lie");
    h.referee_replay(
        h.session_a,
        h.player_a,
        h.player_b,
        &trace.states[s.lo_tick as usize],
        &trace.log[s.lo_tick as usize],
    );
    let s = h.session(&h.session_a);
    assert_eq!(s.winner, party::CHALLENGER, "cheater survived replay");

    // the burned session can no longer settle anything
    assert!(h.settle(h.session_a, &trace.states[N_TICKS as usize]).raw_result.is_err());

    // B proves the honest result through their own slot and settles
    h.referee_init(h.session_b, h.player_b, &trace.claims[0]);
    assert!(h.bind(h.player_b, h.session_b).raw_result.is_ok());
    h.prove_unchallenged(h.session_b, h.player_b, &trace);

    let final_state = &trace.states[N_TICKS as usize];
    let expected = arena::verdict(final_state).unwrap();
    assert_ne!(expected, side::DRAW, "seed produced a draw, pick another");
    let winner_key = if expected == side::FIRST { h.player_a } else { h.player_b };
    let before = h.lamports(&winner_key);

    let r = h.settle(h.session_b, final_state);
    assert!(r.raw_result.is_ok(), "settle failed: {:?}", r.raw_result);

    // the pot goes to whoever actually won the game - punishing the lie
    // (the lost bond) and paying out the match are separate concerns
    assert_eq!(h.game_match().winner, expected);
    assert_eq!(h.lamports(&winner_key), before + 2 * STAKE);
}

#[test]
fn settle_rejects_foreign_and_unbound_sessions() {
    let trace = honest_trace(0xB0B);
    let mut h = harness();
    h.create(&trace.claims[0]);
    h.join();

    // session is fully proven but was never bound to the match
    h.referee_init(h.session_a, h.player_a, &trace.claims[0]);
    h.prove_unchallenged(h.session_a, h.player_a, &trace);
    let r = h.settle(h.session_a, &trace.states[N_TICKS as usize]);
    assert!(r.raw_result.is_err(), "unbound session settled");
}

#[test]
fn bind_rejects_a_session_with_history() {
    let trace = honest_trace(0xB0B);
    let mut h = harness();
    h.create(&trace.claims[0]);
    h.join();

    h.referee_init(h.session_a, h.player_a, &trace.claims[0]);
    h.prove_unchallenged(h.session_a, h.player_a, &trace);
    // lo_tick advanced past genesis, so the bind must fail
    assert!(h.bind(h.player_a, h.session_a).raw_result.is_err());
}

#[test]
fn bind_rejects_the_opponents_slot() {
    let trace = honest_trace(0xB0B);
    let h = harness();
    h.create(&trace.claims[0]);
    h.join();

    // session belongs to A; B cannot bind it anywhere
    h.referee_init(h.session_a, h.player_a, &trace.claims[0]);
    assert!(h.bind(h.player_b, h.session_a).raw_result.is_err());
}

#[test]
fn cancel_refunds_an_unjoined_match() {
    let trace = honest_trace(0xA11CE);
    let h = harness();
    h.create(&trace.claims[0]);

    let before = h.lamports(&h.player_a);
    assert!(h.cancel().raw_result.is_ok());
    assert_eq!(h.lamports(&h.player_a), before + STAKE);
    assert_eq!(h.game_match().phase, phase::SETTLED);

    // and nothing works on a settled match
    assert!(h.join().raw_result.is_err());
}

#[test]
fn expire_refunds_both_after_the_deadline() {
    let trace = honest_trace(0xA11CE);
    let mut h = harness();
    h.create(&trace.claims[0]);
    h.join();

    let early = h.expire();
    assert!(early.raw_result.is_err(), "expire landed before deadline");

    let deadline = h.game_match().deadline;
    h.ctx.mollusk.warp_to_slot(deadline + 1);

    let a_before = h.lamports(&h.player_a);
    let b_before = h.lamports(&h.player_b);
    assert!(h.expire().raw_result.is_ok());
    assert_eq!(h.lamports(&h.player_a), a_before + STAKE);
    assert_eq!(h.lamports(&h.player_b), b_before + STAKE);
    assert_eq!(h.game_match().winner, wager::side::DRAW);
}
