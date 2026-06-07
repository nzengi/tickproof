//! Measures the per-instruction cost of interpreting sbpf inside a
//! contract, using the interp-bench program. The synthetic stream leans
//! on the same mix as a fixed-point physics tick: loads, multiplies,
//! shifts, adds, stores, a loop branch. Two run lengths and a difference
//! quotient strip out the fixed entrypoint overhead.

use mollusk_svm::program::loader_keys::LOADER_V3;
use mollusk_svm::Mollusk;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

fn insn(op: u8, dst: u8, src: u8, off: i16, imm: i32) -> [u8; 8] {
    let mut e = [0u8; 8];
    e[0] = op;
    e[1] = (src << 4) | dst;
    e[2..4].copy_from_slice(&off.to_le_bytes());
    e[4..8].copy_from_slice(&imm.to_le_bytes());
    e
}

/// A loop doing fixed-point-flavored work: per iteration 2 loads, 2
/// stores, 7 alu ops (mul and shift included), 1 back branch.
fn workload(iterations: i32) -> Vec<u8> {
    let prog = [
        insn(0xb7, 1, 0, 0, iterations), // r1 = counter
        insn(0xb7, 2, 0, 0, 0),          // r2 = pointer
        // body
        insn(0x79, 3, 2, 0, 0),          // r3 = mem[r2]
        insn(0x79, 4, 2, 8, 0),          // r4 = mem[r2+8]
        insn(0x2f, 3, 4, 0, 0),          // r3 *= r4
        insn(0x77, 3, 0, 0, 32),         // r3 >>= 32
        insn(0x0f, 3, 4, 0, 0),          // r3 += r4
        insn(0xa7, 4, 0, 0, 0x5DEECE6),  // r4 ^= const
        insn(0x7b, 2, 3, 0, 0),          // mem[r2] = r3
        insn(0x7b, 2, 4, 8, 0),          // mem[r2+8] = r4
        insn(0x07, 2, 0, 0, 16),         // r2 += 16
        insn(0x57, 2, 0, 0, 0x3F0),      // r2 &= 0x3f0 (wrap, aligned)
        insn(0x17, 1, 0, 0, 1),          // r1 -= 1
        insn(0x55, 1, 0, -12, 0),        // if r1 != 0 goto body
        insn(0x95, 0, 0, 0, 0),          // exit
    ];
    prog.concat()
}

/// Returns (executed instruction count, CU consumed).
fn run(mollusk: &Mollusk, program_id: Pubkey, code: &[u8]) -> (u64, u64) {
    let out_key = Pubkey::new_unique();
    let out = Account {
        lamports: 1_000_000_000,
        data: vec![0u8; 16],
        owner: program_id,
        ..Account::default()
    };

    let mut data = u32::MAX.to_le_bytes().to_vec();
    data.extend_from_slice(code);
    let ix = Instruction {
        program_id,
        accounts: vec![AccountMeta::new(out_key, false)],
        data,
    };
    let result = mollusk.process_instruction(&ix, &[(out_key, out)]);
    assert!(result.raw_result.is_ok(), "{:?}", result.raw_result);

    let (_, acc) = result
        .resulting_accounts
        .iter()
        .find(|(k, _)| *k == out_key)
        .unwrap();
    let executed = u64::from_le_bytes(acc.data[..8].try_into().unwrap());
    (executed, result.compute_units_consumed)
}

#[test]
fn cu_per_emulated_instruction() {
    let elf_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/interp_bench.so"
    );
    let elf = std::fs::read(elf_path)
        .expect("interp_bench.so missing - run cargo build-sbf in programs/interp-bench");

    let program_id = Pubkey::new_unique();
    let mut mollusk = Mollusk::default();
    mollusk.add_program_with_loader_and_elf(&program_id, &LOADER_V3, &elf);

    // sized to stay under the 1.4M CU budget - interpretation runs at
    // roughly 80 CU per emulated instruction
    let (n_short, cu_short) = run(&mollusk, program_id, &workload(200));
    let (n_long, cu_long) = run(&mollusk, program_id, &workload(1200));
    assert_eq!(n_short, 2 + 200 * 12 + 1);
    assert_eq!(n_long, 2 + 1200 * 12 + 1);

    let per_insn = (cu_long - cu_short) as f64 / (n_long - n_short) as f64;
    // a tick of the arena game executes roughly its CU count in
    // instructions (no syscalls), measured at ~1958 CU
    let tick_native_cu = 1958.0;
    println!("emulated {n_long} instructions for {cu_long} CU");
    println!("marginal cost: {per_insn:.1} CU per emulated instruction");
    println!(
        "interpreting one arena tick would cost ~{:.0} CU ({}x native), \
         and that is before memory proofs",
        per_insn * tick_native_cu,
        (per_insn * tick_native_cu / tick_native_cu) as u64,
    );

    // if interpretation ever gets this close to native, the comparison
    // table needs a rewrite
    assert!(per_insn > 2.0);
}
