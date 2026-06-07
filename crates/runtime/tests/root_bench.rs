//! On-chain state root cost against state size. The replay instruction
//! pays this twice (pre- and post-state), so the curve bounds how big a
//! game state fits a native one-step proof.

use mollusk_svm::program::loader_keys::LOADER_V3;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

#[test]
fn root_cost_curve() {
    let elf_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/root_bench.so"
    );
    let elf = std::fs::read(elf_path)
        .expect("root_bench.so missing - run cargo build-sbf in programs/root-bench");

    let program_id = Pubkey::new_unique();
    let mut mollusk = Mollusk::default();
    mollusk.add_program_with_loader_and_elf(&program_id, &LOADER_V3, &elf);

    let mut last = 0u64;
    for size in [264usize, 1024, 2048, 4096, 8192] {
        let out_key = Pubkey::new_unique();
        let out = Account {
            lamports: 1_000_000_000,
            data: vec![0u8; 32],
            owner: program_id,
            ..Account::default()
        };
        let state: Vec<u8> = (0..size).map(|i| i as u8).collect();
        let ix = Instruction {
            program_id,
            accounts: vec![AccountMeta::new(out_key, false)],
            data: state.clone(),
        };
        let result = mollusk.process_instruction(&ix, &[(out_key, out)]);
        assert!(result.raw_result.is_ok(), "{size}B: {:?}", result.raw_result);

        // double-check the on-chain root against the host build
        let (_, acc) = result
            .resulting_accounts
            .iter()
            .find(|(k, _)| *k == out_key)
            .unwrap();
        assert_eq!(acc.data[..32], tick_merkle::state_root(&state));

        let cu = result.compute_units_consumed;
        let chunks = size.div_ceil(32);
        println!(
            "{size:>5} B ({chunks:>3} chunks): {cu:>6} CU{}",
            if last > 0 {
                format!("  (+{} over previous)", cu - last)
            } else {
                String::new()
            }
        );
        last = cu;
    }
}
