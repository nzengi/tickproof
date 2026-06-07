//! How much does it cost to interpret sbpf inside a contract, per emulated
//! instruction? This is the number the replay comparison needs: the
//! interpreter-in-contract approach (Lollipop-style SVM-in-SVM) pays this
//! on every instruction of a disputed re-execution, while native replay
//! pays the runtime's ~1 CU.
//!
//! The interpreter below is deliberately minimal - a 64-bit ALU, loads and
//! stores against a flat scratch memory, two branches, exit. No verifier,
//! no call handling, no memory translation, no syscalls. Every cut corner
//! makes interpretation look cheaper than a faithful emulator would be, so
//! the measured figure is a lower bound and the comparison stays
//! conservative.
//!
//! Instruction data: max step count (u32 LE), then standard 8-byte BPF
//! encoded instructions. Account 0 receives the executed-instruction count
//! and final r0, which also keeps the work observable.

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};

const MEM_SIZE: usize = 4096;

entrypoint!(process_instruction);

fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let out = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    if out.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }
    if data.len() < 4 || (data.len() - 4) % 8 != 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let max_steps = u32::from_le_bytes(data[..4].try_into().unwrap()) as u64;
    let code = &data[4..];
    let n_insns = code.len() / 8;

    let mut reg = [0u64; 11];
    let mut mem = vec![0u8; MEM_SIZE];
    let mut pc = 0usize;
    let mut executed = 0u64;

    while executed < max_steps {
        if pc >= n_insns {
            return Err(ProgramError::InvalidInstructionData);
        }
        let i = pc * 8;
        let op = code[i];
        let dst = (code[i + 1] & 0x0f) as usize;
        let src = (code[i + 1] >> 4) as usize;
        let off = i16::from_le_bytes([code[i + 2], code[i + 3]]) as i64;
        let imm = i32::from_le_bytes(code[i + 4..i + 8].try_into().unwrap()) as i64 as u64;
        if dst > 10 || src > 10 {
            return Err(ProgramError::InvalidInstructionData);
        }
        executed += 1;
        pc += 1;

        match op {
            0xb7 => reg[dst] = imm,
            0xbf => reg[dst] = reg[src],
            0x07 => reg[dst] = reg[dst].wrapping_add(imm),
            0x0f => reg[dst] = reg[dst].wrapping_add(reg[src]),
            0x17 => reg[dst] = reg[dst].wrapping_sub(imm),
            0x1f => reg[dst] = reg[dst].wrapping_sub(reg[src]),
            0x27 => reg[dst] = reg[dst].wrapping_mul(imm),
            0x2f => reg[dst] = reg[dst].wrapping_mul(reg[src]),
            0x47 => reg[dst] |= imm,
            0x4f => reg[dst] |= reg[src],
            0x57 => reg[dst] &= imm,
            0x5f => reg[dst] &= reg[src],
            0x67 => reg[dst] = reg[dst].wrapping_shl(imm as u32),
            0x77 => reg[dst] = reg[dst].wrapping_shr(imm as u32),
            0xa7 => reg[dst] ^= imm,
            0xaf => reg[dst] ^= reg[src],
            // ldxdw / stxdw against scratch memory, addresses are plain
            // offsets - a real emulator also proves these against a
            // memory commitment, which costs far more than the read
            0x79 => {
                let addr = checked_addr(reg[src], off)?;
                reg[dst] = u64::from_le_bytes(mem[addr..addr + 8].try_into().unwrap());
            }
            0x7b => {
                let addr = checked_addr(reg[dst], off)?;
                mem[addr..addr + 8].copy_from_slice(&reg[src].to_le_bytes());
            }
            0x05 => pc = branch(pc, off, n_insns)?,
            0x15 => {
                if reg[dst] == imm {
                    pc = branch(pc, off, n_insns)?;
                }
            }
            0x55 => {
                if reg[dst] != imm {
                    pc = branch(pc, off, n_insns)?;
                }
            }
            0x95 => break,
            _ => return Err(ProgramError::InvalidInstructionData),
        }
    }

    let mut out_data = out.try_borrow_mut_data()?;
    if out_data.len() < 16 {
        return Err(ProgramError::InvalidAccountData);
    }
    out_data[..8].copy_from_slice(&executed.to_le_bytes());
    out_data[8..16].copy_from_slice(&reg[0].to_le_bytes());
    Ok(())
}

fn checked_addr(base: u64, off: i64) -> Result<usize, ProgramError> {
    let addr = (base as i64).wrapping_add(off);
    if addr < 0 || addr as usize + 8 > MEM_SIZE {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(addr as usize)
}

fn branch(pc: usize, off: i64, n_insns: usize) -> Result<usize, ProgramError> {
    let target = pc as i64 + off;
    if target < 0 || target as usize >= n_insns {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(target as usize)
}
