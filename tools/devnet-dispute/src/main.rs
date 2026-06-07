//! Plays a complete dispute against the deployed programs on devnet and
//! prints what the measurements need: transaction count, slots, fees and
//! compute units, straight from confirmed transactions.
//!
//! The script is the cheating-operator scenario from the integration
//! tests, but for real: the operator asserts a checkpoint with a wrong
//! state root at tick 11, the challenger bisects them into a corner, and
//! the disputed tick is replayed natively by the cluster. The ground
//! truth is computed locally through mollusk - if the local trace and
//! the cluster disagree on the replay outcome, that's a determinism bug
//! and the whole run fails loudly.
//!
//! Program ids are read from target/deploy/*-keypair.json, so deploy
//! first:
//!   solana program deploy target/deploy/arena_program.so
//!   solana program deploy target/deploy/referee.so

use std::time::Instant;

use arena::{Arena, INPUT_ENTRY_SIZE, N_BALLS, STATE_SIZE};
use referee::{party, status, Claim, Session, SESSION_LEN};
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

const BOND: u64 = 1_000_000;

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

struct Trace {
    claims: Vec<Claim>,
    states: Vec<Vec<u8>>,
    log: Vec<Vec<u8>>,
}

fn honest_trace(elf: &[u8], n_ticks: u64) -> Trace {
    let mut genesis = [0u8; STATE_SIZE];
    Arena::init(&mut genesis).unwrap();
    let mut engine = Engine::new(elf, &genesis);
    let mut rng = Rng::new(0xBEEF);

    let mut claims = vec![claim_bytes(&engine.checkpoint())];
    let mut states = vec![genesis.to_vec()];
    for _ in 0..n_ticks {
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
    referee_id: Pubkey,
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

        // get_transaction defaults to finalized; ask at confirmed and give
        // the rpc node a moment to index the transaction
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
            "{:<12} slot {:>9}  fee {:>5}  cu {:>6}  {}",
            rec.label, rec.slot, rec.fee, rec.cu, rec.sig
        );
        self.sent.push(rec);
    }

    fn referee_ix(&self, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Instruction {
        Instruction {
            program_id: self.referee_id,
            accounts,
            data,
        }
    }

    fn session_state(&self, session: &Pubkey) -> Session {
        let data = self.client.get_account_data(session).expect("session");
        Session::read(&data).expect("session layout")
    }
}

fn keypair_pubkey(path: &str) -> Pubkey {
    read_keypair_file(path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .pubkey()
}

fn main() {
    // args: [tick range] [lie position], default a 16-tick range with the
    // lie injected at tick 11
    let args: Vec<String> = std::env::args().collect();
    let n_ticks: u64 = args.get(1).map_or(16, |s| s.parse().expect("tick range"));
    let lie_at: usize = args
        .get(2)
        .map_or(n_ticks as usize * 2 / 3, |s| s.parse().expect("lie tick"));
    assert!(lie_at > 0 && (lie_at as u64) <= n_ticks);

    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let elf = std::fs::read(format!("{root}/target/deploy/arena_program.so"))
        .expect("run cargo build-sbf in programs/arena-program");
    let arena_id = keypair_pubkey(&format!("{root}/target/deploy/arena_program-keypair.json"));
    let referee_id = keypair_pubkey(&format!("{root}/target/deploy/referee-keypair.json"));

    let home = std::env::var("HOME").unwrap();
    let payer = read_keypair_file(format!("{home}/.config/solana/id.json")).expect("wallet");

    let client = RpcClient::new_with_commitment(
        "https://api.devnet.solana.com".to_string(),
        CommitmentConfig::confirmed(),
    );
    let start_balance = client.get_balance(&payer.pubkey()).expect("balance");

    println!("payer    {}", payer.pubkey());
    println!("arena    {arena_id}");
    println!("referee  {referee_id}");

    // local ground truth and the operator's lie
    let trace = honest_trace(&elf, n_ticks);
    let mut asserted = trace.claims.clone();
    for c in asserted.iter_mut().skip(lie_at) {
        c[0] ^= 0xFF;
    }

    let operator = Keypair::new();
    let challenger = Keypair::new();
    let session = Keypair::new();
    let scratch = Keypair::new();

    let mut d = Driver {
        client,
        payer,
        referee_id,
        sent: Vec::new(),
    };
    let wall = Instant::now();

    // session funded with rent plus both bonds in one go; the operator
    // and challenger sides are both played from this wallet
    let session_rent = d
        .client
        .get_minimum_balance_for_rent_exemption(SESSION_LEN)
        .unwrap();
    let mut init_data = vec![0u8];
    init_data.extend_from_slice(&BOND.to_le_bytes());
    init_data.extend_from_slice(&trace.claims[0]);
    d.send(
        "init",
        &[
            system_instruction::create_account(
                &d.payer.pubkey(),
                &session.pubkey(),
                session_rent + 2 * BOND,
                SESSION_LEN as u64,
                &referee_id,
            ),
            d.referee_ix(
                vec![
                    AccountMeta::new(session.pubkey(), false),
                    AccountMeta::new_readonly(operator.pubkey(), true),
                    AccountMeta::new_readonly(arena_id, false),
                ],
                init_data,
            ),
        ],
        &[&session, &operator],
    );

    let mut cp_data = vec![1u8];
    cp_data.extend_from_slice(&n_ticks.to_le_bytes());
    cp_data.extend_from_slice(&asserted[n_ticks as usize]);
    d.send(
        "checkpoint",
        &[d.referee_ix(
            vec![
                AccountMeta::new(session.pubkey(), false),
                AccountMeta::new_readonly(operator.pubkey(), true),
            ],
            cp_data,
        )],
        &[&operator],
    );

    d.send(
        "challenge",
        &[d.referee_ix(
            vec![
                AccountMeta::new(session.pubkey(), false),
                AccountMeta::new_readonly(challenger.pubkey(), true),
            ],
            vec![3u8],
        )],
        &[&challenger],
    );

    // bisection: operator answers from the corrupt trace, challenger
    // agrees wherever the asserted claim matches the honest one
    let mut round = 0;
    loop {
        let s = d.session_state(&session.pubkey());
        match (s.status, s.turn) {
            (status::BISECTING, party::OPERATOR) => {
                round += 1;
                let mid = s.lo_tick + (s.hi_tick - s.lo_tick) / 2;
                let mut data = vec![4u8];
                data.extend_from_slice(&asserted[mid as usize]);
                d.send(
                    &format!("bisect {round}"),
                    &[d.referee_ix(
                        vec![
                            AccountMeta::new(session.pubkey(), false),
                            AccountMeta::new_readonly(operator.pubkey(), true),
                        ],
                        data,
                    )],
                    &[&operator],
                );
            }
            (status::BISECTING, party::CHALLENGER) => {
                let agree = s.mid_claim == trace.claims[s.mid_tick as usize];
                d.send(
                    &format!("pick {round}"),
                    &[d.referee_ix(
                        vec![
                            AccountMeta::new(session.pubkey(), false),
                            AccountMeta::new_readonly(challenger.pubkey(), true),
                        ],
                        vec![5u8, agree as u8],
                    )],
                    &[&challenger],
                );
            }
            _ => break,
        }
    }

    let s = d.session_state(&session.pubkey());
    assert_eq!(s.status, status::AWAITING_REPLAY, "bisection didn't corner");
    println!(
        "cornered: tick {} -> {} (lie was injected at {lie_at})",
        s.lo_tick, s.hi_tick
    );

    let scratch_rent = d
        .client
        .get_minimum_balance_for_rent_exemption(STATE_SIZE)
        .unwrap();
    let mut replay_data = vec![6u8];
    let inputs = &trace.log[s.lo_tick as usize];
    replay_data.extend_from_slice(&(inputs.len() as u32).to_le_bytes());
    replay_data.extend_from_slice(inputs);
    replay_data.extend_from_slice(&trace.states[s.lo_tick as usize]);
    d.send(
        "replay",
        &[
            system_instruction::create_account(
                &d.payer.pubkey(),
                &scratch.pubkey(),
                scratch_rent,
                STATE_SIZE as u64,
                &arena_id,
            ),
            d.referee_ix(
                vec![
                    AccountMeta::new(session.pubkey(), false),
                    AccountMeta::new(scratch.pubkey(), true),
                    AccountMeta::new_readonly(arena_id, false),
                    AccountMeta::new(operator.pubkey(), false),
                    AccountMeta::new(challenger.pubkey(), false),
                ],
                replay_data,
            ),
        ],
        &[&scratch],
    );

    let s = d.session_state(&session.pubkey());
    assert_eq!(s.status, status::RESOLVED, "dispute not resolved");
    assert_eq!(s.winner, party::CHALLENGER, "wrong winner");
    let challenger_balance = d.client.get_balance(&challenger.pubkey()).unwrap();
    assert_eq!(challenger_balance, 2 * BOND, "pot not paid out");

    let elapsed = wall.elapsed();
    let end_balance = d.client.get_balance(&d.payer.pubkey()).unwrap();
    let first_slot = d.sent.first().unwrap().slot;
    let last_slot = d.sent.last().unwrap().slot;
    let total_fees: u64 = d.sent.iter().map(|r| r.fee).sum();

    println!();
    println!("dispute settled: challenger wins, pot {} lamports", 2 * BOND);
    println!(
        "{} transactions over {} slots, {elapsed:.1?} wall clock",
        d.sent.len(),
        last_slot - first_slot,
    );
    println!("total tx fees: {total_fees} lamports");
    println!(
        "payer spent {} lamports incl. rent deposits",
        start_balance - end_balance
    );
}
