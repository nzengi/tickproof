//! Hashes the instruction data into a state root and writes it out. The
//! point is the CU meter: the replay instruction computes exactly this
//! over the pre- and post-state, so the curve of root cost against state
//! size says how big a game state native replay can carry.

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};

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
    let root = tick_merkle::state_root(data);
    let mut out_data = out.try_borrow_mut_data()?;
    if out_data.len() < 32 {
        return Err(ProgramError::InvalidAccountData);
    }
    out_data[..32].copy_from_slice(&root);
    Ok(())
}
