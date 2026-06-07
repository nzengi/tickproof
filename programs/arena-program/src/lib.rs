//! Thin on-chain wrapper around the arena tick function.
//!
//! Two instructions, selected by the first byte:
//!
//! 0 = Tick: tick index (u64 LE) then the raw input log entry for that
//!     tick. Account 0 is the state account, owned by this program. The
//!     exact same `arena` crate runs off-chain; this instruction is what
//!     makes a disputed tick replayable by the chain itself.
//!
//! 1 = LoadState: raw state bytes, copied verbatim into account 0. Only
//!     the runtime can write program-owned accounts, so the referee uses
//!     this to seed a replay scratch account with the agreed pre-state.
//!     The account has to sign, which keeps anyone from overwriting a
//!     live session's state - scratch accounts are throwaway keypairs,
//!     real state accounts never sign anything.

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
    let state = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    if state.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }

    match data.split_first() {
        Some((0, rest)) => tick(state, rest),
        Some((1, rest)) => load_state(state, rest),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn tick(state: &AccountInfo, data: &[u8]) -> ProgramResult {
    if data.len() < 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let tick_index = u64::from_le_bytes(data[..8].try_into().unwrap());
    let inputs = &data[8..];

    let mut state_data = state.try_borrow_mut_data()?;
    Arena::tick(&mut state_data, inputs, tick_index).map_err(|e| match e {
        TickError::BadStateSize => ProgramError::InvalidAccountData,
        TickError::BadInput => ProgramError::InvalidInstructionData,
    })
}

fn load_state(state: &AccountInfo, data: &[u8]) -> ProgramResult {
    if !state.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut state_data = state.try_borrow_mut_data()?;
    if state_data.len() != data.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    state_data.copy_from_slice(data);
    Ok(())
}
