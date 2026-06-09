//! Plays a real staked match on devnet, end to end: two players escrow
//! lamports in the wager program, the match runs off-chain through the
//! engine, and the result settles with no trusted reporter - the final
//! checkpoint is proven through the referee and the game program itself
//! names the winner over the proven state.
//!
//! Default is the honest run: assert, sit out the challenge window,
//! finalize, settle. `--cheat` plays the full adversarial story instead:
//! player A asserts a corrupted result, player B challenges, bisects the
//! lie into a corner, the cluster replays the disputed tick natively, A's
//! session burns, and B proves the honest result through their own slot
//! and settles the pot.
//!
//! Program ids are read from target/deploy/*-keypair.json, so deploy
//! first:
//!   solana program deploy target/deploy/arena_program.so
//!   solana program deploy target/deploy/referee.so
//!   solana program deploy target/deploy/wager.so

use std::time::Instant;

use arena::{side, Arena, INPUT_ENTRY_SIZE, STATE_SIZE};
use referee::{party, status as referee_status, Claim, Session, CHALLENGE_WINDOW_SLOTS, SESSION_LEN};
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_system_interface::instruction as system_instruction;
use solana_transaction_status::UiTransactionEncoding;
use tick_core::rng::Rng;
use tickproof_runtime::{Checkpoint, Engine};
use wager::{phase, Match, MATCH_LEN};

const STAKE: u64 = 5_000_000;
const BOND: u64 = 1_000_000;
const DEADLINE_SLOTS: u64 = 5_000;
const N_TICKS: u64 = 32;

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

fn honest_trace(elf: &[u8]) -> Trace {
    let mut genesis = [0u8; STATE_SIZE];
    Arena::init(&mut genesis).unwrap();
    let mut engine = Engine::new(elf, &genesis);
    let mut rng = Rng::new(0xFA7E);

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

struct Record {
    label: String,
    sig: Signature,
    slot: u64,
    fee: u64,
    cu: u64,
}

struct Driver {
    client: RpcClient,
    payer: Keypair,
    sent: Vec<Record>,
}

impl Driver {
    fn send(&mut self, label: &str, ixs: &[Instruction], extra_signers: &[&Keypair]) {
        let blockhash = self.client.get_latest_blockhash().expect("blockhash");
        let mut signers: Vec<&Keypair> = vec![&self.payer];
        signers.extend_from_slice(extra_signers);
        let tx = Transaction::new_signed_with_payer(
            ixs,
            Some(&self.payer.pubkey()),
            &signers,
            blockhash,
        );
        let sig = self
            .client
            .send_and_confirm_transaction(&tx)
            .unwrap_or_else(|e| panic!("{label} failed: {e}"));

        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };
        let mut attempts = 0;
        let confirmed = loop {
            match self.client.get_transaction_with_config(&sig, config) {
                Ok(tx) => break tx,
                Err(e) if attempts < 10 => {
                    attempts += 1;
                    let _ = e;
                    std::thread::sleep(std::time::Duration::from_millis(800));
                }
                Err(e) => panic!("fetch {label} tx: {e}"),
            }
        };
        let meta = confirmed.transaction.meta.expect("tx meta");
        let cu: Option<u64> = meta.compute_units_consumed.into();
        let rec = Record {
            label: label.to_string(),
            sig,
            slot: confirmed.slot,
            fee: meta.fee,
            cu: cu.unwrap_or(0),
        };
        println!(
            "{:<14} slot {:>9}  fee {:>5}  cu {:>6}  {}",
            rec.label, rec.slot, rec.fee, rec.cu, rec.sig
        );
        self.sent.push(rec);
    }

    fn session_state(&self, session: &Pubkey) -> Session {
        let data = self.client.get_account_data(session).expect("session");
        Session::read(&data).expect("session layout")
    }

    fn match_state(&self, game_match: &Pubkey) -> Match {
        let data = self.client.get_account_data(game_match).expect("match");
        Match::read(&data).expect("match layout")
    }

    /// Block until the cluster moves past `slot`.
    fn wait_past_slot(&self, slot: u64, label: &str) {
        println!("waiting out {label} (until slot {slot})...");
        loop {
            let now = self.client.get_slot().expect("slot");
            if now > slot {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

fn keypair_pubkey(path: &str) -> Pubkey {
    read_keypair_file(path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .pubkey()
}

struct Setup {
    d: Driver,
    arena_id: Pubkey,
    referee_id: Pubkey,
    wager_id: Pubkey,
    trace: Trace,
    player_a: Keypair,
    player_b: Keypair,
    game_match: Keypair,
}

fn setup() -> Setup {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let elf = std::fs::read(format!("{root}/target/deploy/arena_program.so"))
        .expect("run cargo build-sbf in programs/arena-program");
    let arena_id = keypair_pubkey(&format!("{root}/target/deploy/arena_program-keypair.json"));
    let referee_id = keypair_pubkey(&format!("{root}/target/deploy/referee-keypair.json"));
    let wager_id = keypair_pubkey(&format!("{root}/target/deploy/wager-keypair.json"));

    let home = std::env::var("HOME").unwrap();
    let payer = read_keypair_file(format!("{home}/.config/solana/id.json")).expect("wallet");

    let client = RpcClient::new_with_commitment(
        "https://api.devnet.solana.com".to_string(),
        CommitmentConfig::confirmed(),
    );

    let trace = honest_trace(&elf);
    Setup {
        d: Driver {
            client,
            payer,
            sent: Vec::new(),
        },
        arena_id,
        referee_id,
        wager_id,
        trace,
        player_a: Keypair::new(),
        player_b: Keypair::new(),
        game_match: Keypair::new(),
    }
}

impl Setup {
    fn wager_ix(&self, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Instruction {
        Instruction {
            program_id: self.wager_id,
            accounts,
            data,
        }
    }

    fn referee_ix(&self, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Instruction {
        Instruction {
            program_id: self.referee_id,
            accounts,
            data,
        }
    }

    /// Escrow both stakes: create + fund the match account, A creates,
    /// B tops up their stake and joins.
    fn open_match(&mut self) {
        let match_rent = self
            .d
            .client
            .get_minimum_balance_for_rent_exemption(MATCH_LEN)
            .unwrap();
        let mut create_data = vec![0u8];
        create_data.extend_from_slice(&STAKE.to_le_bytes());
        create_data.extend_from_slice(&N_TICKS.to_le_bytes());
        create_data.extend_from_slice(&DEADLINE_SLOTS.to_le_bytes());
        create_data.extend_from_slice(&self.trace.claims[0]);
        let create = [
            system_instruction::create_account(
                &self.d.payer.pubkey(),
                &self.game_match.pubkey(),
                match_rent + STAKE,
                MATCH_LEN as u64,
                &self.wager_id,
            ),
            self.wager_ix(
                vec![
                    AccountMeta::new(self.game_match.pubkey(), false),
                    AccountMeta::new_readonly(self.player_a.pubkey(), true),
                    AccountMeta::new_readonly(self.player_b.pubkey(), false),
                    AccountMeta::new_readonly(self.arena_id, false),
                    AccountMeta::new_readonly(self.referee_id, false),
                ],
                create_data,
            ),
        ];
        let (game_match, player_a) = (&self.game_match, &self.player_a);
        self.d.send("create", &create, &[game_match, player_a]);

        let join = [
            system_instruction::transfer(
                &self.d.payer.pubkey(),
                &self.game_match.pubkey(),
                STAKE,
            ),
            self.wager_ix(
                vec![
                    AccountMeta::new(self.game_match.pubkey(), false),
                    AccountMeta::new_readonly(self.player_b.pubkey(), true),
                ],
                vec![1u8],
            ),
        ];
        let player_b = &self.player_b;
        self.d.send("join", &join, &[player_b]);
    }

    /// Open a referee session for `operator` and bind it to the match.
    fn open_session(&mut self, label: &str, operator_is_a: bool, lamports: u64) -> Keypair {
        let session = Keypair::new();
        let operator = if operator_is_a { &self.player_a } else { &self.player_b };
        let session_rent = self
            .d
            .client
            .get_minimum_balance_for_rent_exemption(SESSION_LEN)
            .unwrap();
        let mut init_data = vec![0u8];
        init_data.extend_from_slice(&BOND.to_le_bytes());
        init_data.extend_from_slice(&self.trace.claims[0]);
        let ixs = [
            system_instruction::create_account(
                &self.d.payer.pubkey(),
                &session.pubkey(),
                session_rent + lamports,
                SESSION_LEN as u64,
                &self.referee_id,
            ),
            self.referee_ix(
                vec![
                    AccountMeta::new(session.pubkey(), false),
                    AccountMeta::new_readonly(operator.pubkey(), true),
                    AccountMeta::new_readonly(self.arena_id, false),
                ],
                init_data,
            ),
            self.wager_ix(
                vec![
                    AccountMeta::new(self.game_match.pubkey(), false),
                    AccountMeta::new_readonly(operator.pubkey(), true),
                    AccountMeta::new_readonly(session.pubkey(), false),
                ],
                vec![3u8],
            ),
        ];
        let signers: [&Keypair; 2] = [&session, operator];
        self.d.send(label, &ixs, &signers);
        session
    }

    fn checkpoint(&mut self, label: &str, session: &Pubkey, operator_is_a: bool, claim: &Claim) {
        let operator = if operator_is_a { &self.player_a } else { &self.player_b };
        let mut data = vec![1u8];
        data.extend_from_slice(&N_TICKS.to_le_bytes());
        data.extend_from_slice(claim);
        let ix = self.referee_ix(
            vec![
                AccountMeta::new(*session, false),
                AccountMeta::new_readonly(operator.pubkey(), true),
            ],
            data,
        );
        let signer = if operator_is_a { &self.player_a } else { &self.player_b };
        self.d.send(label, &[ix], &[signer]);
    }

    fn finalize_after_window(&mut self, session: &Pubkey) {
        let s = self.d.session_state(session);
        self.d
            .wait_past_slot(s.posted_slot + CHALLENGE_WINDOW_SLOTS, "challenge window");
        let ix = self.referee_ix(vec![AccountMeta::new(*session, false)], vec![2u8]);
        self.d.send("finalize", &[ix], &[]);
    }

    /// Prove the final state through `session` and settle the pot.
    fn settle(&mut self, session: &Pubkey) {
        let scratch = Keypair::new();
        let scratch_rent = self
            .d
            .client
            .get_minimum_balance_for_rent_exemption(STATE_SIZE)
            .unwrap();
        let mut data = vec![5u8];
        data.extend_from_slice(&self.trace.states[N_TICKS as usize]);
        let ixs = [
            system_instruction::create_account(
                &self.d.payer.pubkey(),
                &scratch.pubkey(),
                scratch_rent,
                STATE_SIZE as u64,
                &self.arena_id,
            ),
            self.wager_ix(
                vec![
                    AccountMeta::new(self.game_match.pubkey(), false),
                    AccountMeta::new_readonly(*session, false),
                    AccountMeta::new(scratch.pubkey(), true),
                    AccountMeta::new_readonly(self.arena_id, false),
                    AccountMeta::new(self.player_a.pubkey(), false),
                    AccountMeta::new(self.player_b.pubkey(), false),
                ],
                data,
            ),
        ];
        self.d.send("settle", &ixs, &[&scratch]);
    }

    /// B challenges A's session and bisects the lie into a corner; the
    /// cluster then replays the disputed tick natively.
    fn dispute(&mut self, session: &Pubkey, asserted: &[Claim]) {
        let challenge = [
            system_instruction::transfer(&self.d.payer.pubkey(), session, BOND),
            self.referee_ix(
                vec![
                    AccountMeta::new(*session, false),
                    AccountMeta::new_readonly(self.player_b.pubkey(), true),
                ],
                vec![3u8],
            ),
        ];
        let player_b = &self.player_b;
        self.d.send("challenge", &challenge, &[player_b]);

        let mut round = 0;
        loop {
            let s = self.d.session_state(session);
            match (s.status, s.turn) {
                (referee_status::BISECTING, party::OPERATOR) => {
                    round += 1;
                    let mid = s.lo_tick + (s.hi_tick - s.lo_tick) / 2;
                    let mut data = vec![4u8];
                    data.extend_from_slice(&asserted[mid as usize]);
                    let ix = self.referee_ix(
                        vec![
                            AccountMeta::new(*session, false),
                            AccountMeta::new_readonly(self.player_a.pubkey(), true),
                        ],
                        data,
                    );
                    let player_a = &self.player_a;
                    self.d.send(&format!("bisect {round}"), &[ix], &[player_a]);
                }
                (referee_status::BISECTING, party::CHALLENGER) => {
                    let agree = s.mid_claim == self.trace.claims[s.mid_tick as usize];
                    let ix = self.referee_ix(
                        vec![
                            AccountMeta::new(*session, false),
                            AccountMeta::new_readonly(self.player_b.pubkey(), true),
                        ],
                        vec![5u8, agree as u8],
                    );
                    let player_b = &self.player_b;
                    self.d.send(&format!("pick {round}"), &[ix], &[player_b]);
                }
                _ => break,
            }
        }

        let s = self.d.session_state(session);
        assert_eq!(s.status, referee_status::AWAITING_REPLAY, "bisection didn't corner");
        println!("cornered: tick {} -> {}", s.lo_tick, s.hi_tick);

        let scratch = Keypair::new();
        let scratch_rent = self
            .d
            .client
            .get_minimum_balance_for_rent_exemption(STATE_SIZE)
            .unwrap();
        let inputs = &self.trace.log[s.lo_tick as usize];
        let mut data = vec![6u8];
        data.extend_from_slice(&(inputs.len() as u32).to_le_bytes());
        data.extend_from_slice(inputs);
        data.extend_from_slice(&self.trace.states[s.lo_tick as usize]);
        let ixs = [
            system_instruction::create_account(
                &self.d.payer.pubkey(),
                &scratch.pubkey(),
                scratch_rent,
                STATE_SIZE as u64,
                &self.arena_id,
            ),
            self.referee_ix(
                vec![
                    AccountMeta::new(*session, false),
                    AccountMeta::new(scratch.pubkey(), true),
                    AccountMeta::new_readonly(self.arena_id, false),
                    AccountMeta::new(self.player_a.pubkey(), false),
                    AccountMeta::new(self.player_b.pubkey(), false),
                ],
                data,
            ),
        ];
        self.d.send("replay", &ixs, &[&scratch]);

        let s = self.d.session_state(session);
        assert_eq!(s.status, referee_status::RESOLVED, "dispute not resolved");
        assert_eq!(s.winner, party::CHALLENGER, "cheater survived the replay");
        println!("dispute settled: challenger takes both bonds");
    }
}

fn main() {
    let cheat = std::env::args().any(|a| a == "--cheat");

    let mut s = setup();
    let start_balance = s.d.client.get_balance(&s.d.payer.pubkey()).expect("balance");
    println!("payer    {}", s.d.payer.pubkey());
    println!("arena    {}", s.arena_id);
    println!("referee  {}", s.referee_id);
    println!("wager    {}", s.wager_id);
    println!("match    {}", s.game_match.pubkey());
    println!("player A {}", s.player_a.pubkey());
    println!("player B {}", s.player_b.pubkey());

    // the match was already played off-chain; this is what settlement
    // has to agree with
    let final_state = s.trace.states[N_TICKS as usize].clone();
    let expected = arena::verdict(&final_state).unwrap();
    let expected_name = match expected {
        side::FIRST => "player A",
        side::SECOND => "player B",
        _ => "draw",
    };
    println!("local verdict after {N_TICKS} ticks: {expected_name}");

    let wall = Instant::now();
    s.open_match();

    let settle_session = if cheat {
        // A asserts a corrupted final claim through their bound session
        let lie_at = N_TICKS as usize * 2 / 3;
        let mut asserted = s.trace.claims.clone();
        for c in asserted.iter_mut().skip(lie_at) {
            c[0] ^= 0xFF;
        }
        println!("player A will lie from tick {lie_at}");

        let session_a = s.open_session("session A", true, BOND);
        s.checkpoint("checkpoint A", &session_a.pubkey(), true, &asserted[N_TICKS as usize]);
        s.dispute(&session_a.pubkey(), &asserted);

        // the burned session can't settle; B proves the honest result
        let session_b = s.open_session("session B", false, BOND);
        s.checkpoint("checkpoint B", &session_b.pubkey(), false, &s.trace.claims[N_TICKS as usize].clone());
        s.finalize_after_window(&session_b.pubkey());
        session_b
    } else {
        let session_a = s.open_session("session A", true, BOND);
        s.checkpoint("checkpoint A", &session_a.pubkey(), true, &s.trace.claims[N_TICKS as usize].clone());
        s.finalize_after_window(&session_a.pubkey());
        session_a
    };

    s.settle(&settle_session.pubkey());

    let m = s.d.match_state(&s.game_match.pubkey());
    assert_eq!(m.phase, phase::SETTLED, "match not settled");
    assert_eq!(m.winner, expected, "on-chain winner disagrees with local verdict");
    let winner_key = if expected == side::FIRST {
        s.player_a.pubkey()
    } else {
        s.player_b.pubkey()
    };
    let winner_balance = s.d.client.get_balance(&winner_key).unwrap();
    assert!(winner_balance >= 2 * STAKE, "pot not paid out");

    let elapsed = wall.elapsed();
    let end_balance = s.d.client.get_balance(&s.d.payer.pubkey()).unwrap();
    let first_slot = s.d.sent.first().unwrap().slot;
    let last_slot = s.d.sent.last().unwrap().slot;
    let total_fees: u64 = s.d.sent.iter().map(|r| r.fee).sum();

    println!();
    println!(
        "match settled trustlessly: {expected_name} takes the pot ({} lamports)",
        2 * STAKE
    );
    if cheat {
        let b_balance = s.d.client.get_balance(&s.player_b.pubkey()).unwrap();
        println!("player B also holds both dispute bonds (balance {b_balance})");
    }
    println!(
        "{} transactions over {} slots, {elapsed:.1?} wall clock",
        s.d.sent.len(),
        last_slot - first_slot,
    );
    println!("total tx fees: {total_fees} lamports");
    println!(
        "payer spent {} lamports incl. rent deposits and stakes",
        start_balance - end_balance
    );
}
