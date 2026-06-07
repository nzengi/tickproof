//! Thin on-chain wrapper: one instruction = one arena tick.
//!
//! Instruction data: tick index (u64 LE) followed by the raw input log
//! entry for that tick. Account 0 is the state account, owned by this
//! program. The exact same `arena` crate runs off-chain; this wrapper is
//! what makes a disputed tick replayable by the chain itself.

use arena::Arena;
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};
use tick_core::{TickError, TickLogic};

entrypoint!(process_instruction);

fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() < 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let tick_index = u64::from_le_bytes(data[..8].try_into().unwrap());
    let inputs = &data[8..];

    let state = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    if state.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }
    let mut state_data = state.try_borrow_mut_data()?;

    Arena::tick(&mut state_data, inputs, tick_index).map_err(|e| match e {
        TickError::BadStateSize => ProgramError::InvalidAccountData,
        TickError::BadInput => ProgramError::InvalidInstructionData,
    })
}
