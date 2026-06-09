//! Stake escrow for two-player matches with no trusted result reporter.
//! Both players lock the stake, play off-chain at full speed, and settle
//! one of two ways: both sign the result (the normal case), or either
//! player proves the result through the referee - assert the final
//! checkpoint, survive the challenge window (cheating assertions get
//! bisected and replayed natively by the chain), then hand the finalized
//! state here. The match account never has to trust a server, an oracle,
//! or the other player.
//!
//! Who won is the game's business, not the escrow's: settlement CPIs the
//! game program's Verdict instruction over the proven final state and
//! pays out whatever side it returns. Any game that exposes LoadState /
//! Verdict settles through this same program.
//!
//! Each player gets their own referee-session slot, bound while the
//! match is live. A player can only ever rebind their *own* slot, so a
//! cheater whose session got burned in a dispute cannot touch the
//! opponent's path to settlement. Funds can't deadlock: a live match
//! that nobody manages to settle refunds both sides after the deadline.
//!
//! Known prototype gap, same one the referee has: the input log is
//! whatever the asserting operator committed to. Binding opponent inputs
//! cryptographically (signed input entries checked inside the tick) is
//! the next layer, not this one.

use referee::{status as referee_status, Session};
#[cfg(not(feature = "no-entrypoint"))]
use solana_program::entrypoint;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::{get_return_data, invoke},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

pub const MATCH_LEN: usize = 296;

pub mod phase {
    pub const OPEN: u8 = 0;
    pub const LIVE: u8 = 1;
    pub const SETTLED: u8 = 2;
}

/// Verdict encoding, shared with the game programs.
pub mod side {
    pub const DRAW: u8 = 0;
    pub const A: u8 = 1;
    pub const B: u8 = 2;
}

/// state merkle root (32) followed by input chain (32), as in the referee
pub type Claim = [u8; 64];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub phase: u8,
    pub winner: u8,
    pub player_a: Pubkey,
    pub player_b: Pubkey,
    pub game_program: Pubkey,
    pub referee_program: Pubkey,
    pub session_a: Pubkey,
    pub session_b: Pubkey,
    pub stake: u64,
    pub final_tick: u64,
    pub deadline_slots: u64,
    pub deadline: u64,
    pub genesis_claim: Claim,
}

impl Match {
    pub fn read(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != MATCH_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let pk = |o: usize| Pubkey::new_from_array(data[o..o + 32].try_into().unwrap());
        let u64le = |o: usize| u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
        Ok(Self {
            phase: data[0],
            winner: data[1],
            player_a: pk(8),
            player_b: pk(40),
            game_program: pk(72),
            referee_program: pk(104),
            session_a: pk(136),
            session_b: pk(168),
            stake: u64le(200),
            final_tick: u64le(208),
            deadline_slots: u64le(216),
            deadline: u64le(224),
            genesis_claim: data[232..296].try_into().unwrap(),
        })
    }

    pub fn write(&self, data: &mut [u8]) {
        data[0] = self.phase;
        data[1] = self.winner;
        data[8..40].copy_from_slice(self.player_a.as_ref());
        data[40..72].copy_from_slice(self.player_b.as_ref());
        data[72..104].copy_from_slice(self.game_program.as_ref());
        data[104..136].copy_from_slice(self.referee_program.as_ref());
        data[136..168].copy_from_slice(self.session_a.as_ref());
        data[168..200].copy_from_slice(self.session_b.as_ref());
        data[200..208].copy_from_slice(&self.stake.to_le_bytes());
        data[208..216].copy_from_slice(&self.final_tick.to_le_bytes());
        data[216..224].copy_from_slice(&self.deadline_slots.to_le_bytes());
        data[224..232].copy_from_slice(&self.deadline.to_le_bytes());
        data[232..296].copy_from_slice(&self.genesis_claim);
    }
}

// protocol violations; everything else maps to builtin ProgramErrors
const ERR_PHASE: u32 = 0; // instruction not valid in current phase
const ERR_SESSION: u32 = 1; // session doesn't qualify for this match
const ERR_UNPROVEN: u32 = 2; // session not finalized at the match's final tick
const ERR_CLAIM: u32 = 3; // supplied state doesn't hash to the proven claim
const ERR_NOT_EXPIRED: u32 = 4; // expire called before the deadline
const ERR_VERDICT: u32 = 5; // game returned no verdict or a bad one

fn err(code: u32) -> ProgramError {
    ProgramError::Custom(code)
}

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let (tag, rest) = data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    let iter = &mut accounts.iter();
    let match_info = next_account_info(iter)?;
    if match_info.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }
    let mut m = Match::read(&match_info.try_borrow_data()?)?;

    match tag {
        0 => create(&mut m, match_info, iter, rest)?,
        1 => join(&mut m, match_info, iter)?,
        2 => cancel(&mut m, match_info, iter)?,
        3 => bind(&mut m, iter)?,
        4 => settle_coop(&mut m, match_info, iter, rest)?,
        5 => settle(&mut m, match_info, iter, rest)?,
        6 => expire(&mut m, match_info, iter)?,
        _ => return Err(ProgramError::InvalidInstructionData),
    }

    m.write(&mut match_info.try_borrow_mut_data()?);
    Ok(())
}

fn signer<'a, 'b>(
    iter: &mut core::slice::Iter<'a, AccountInfo<'b>>,
) -> Result<&'a AccountInfo<'b>, ProgramError> {
    let info = next_account_info(iter)?;
    if !info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(info)
}

/// The escrow never pulls transfers; stakes arrive as plain lamport
/// top-ups of the match account and this balance check is the
/// enforcement, same scheme as the referee's bonds.
fn require_staked(m: &Match, info: &AccountInfo, sides: u64) -> ProgramResult {
    let rent = Rent::get()?.minimum_balance(MATCH_LEN);
    if info.lamports() < rent + m.stake * sides {
        return Err(ProgramError::InsufficientFunds);
    }
    Ok(())
}

/// data: stake u64, final tick u64, deadline slots u64, genesis claim.
/// accounts: player a (signer), player b, game program, referee program.
/// The match account is created and funded with rent + stake beforehand;
/// all-zero data is what marks it as uninitialized.
fn create(
    m: &mut Match,
    match_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if data.len() != 8 + 8 + 8 + 64 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if m.player_a != Pubkey::default() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let player_a = signer(iter)?;
    let player_b = next_account_info(iter)?;
    let game_program = next_account_info(iter)?;
    let referee_program = next_account_info(iter)?;
    if !game_program.executable || !referee_program.executable {
        return Err(ProgramError::InvalidAccountData);
    }
    let final_tick = u64::from_le_bytes(data[8..16].try_into().unwrap());
    if final_tick == 0 {
        return Err(ProgramError::InvalidArgument);
    }

    m.player_a = *player_a.key;
    m.player_b = *player_b.key;
    m.game_program = *game_program.key;
    m.referee_program = *referee_program.key;
    m.stake = u64::from_le_bytes(data[..8].try_into().unwrap());
    m.final_tick = final_tick;
    m.deadline_slots = u64::from_le_bytes(data[16..24].try_into().unwrap());
    m.genesis_claim = data[24..88].try_into().unwrap();
    require_staked(m, match_info, 1)
}

/// accounts: player b (signer). Their stake must already sit on the
/// match account. The settlement deadline starts counting here.
fn join(
    m: &mut Match,
    match_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
) -> ProgramResult {
    if m.phase != phase::OPEN || m.player_a == Pubkey::default() {
        return Err(err(ERR_PHASE));
    }
    let player_b = signer(iter)?;
    if *player_b.key != m.player_b {
        return Err(ProgramError::MissingRequiredSignature);
    }
    require_staked(m, match_info, 2)?;

    m.phase = phase::LIVE;
    m.deadline = Clock::get()?.slot + m.deadline_slots;
    Ok(())
}

/// accounts: player a (signer, writable). Withdraws an unjoined match.
fn cancel(
    m: &mut Match,
    match_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
) -> ProgramResult {
    if m.phase != phase::OPEN || m.player_a == Pubkey::default() {
        return Err(err(ERR_PHASE));
    }
    let player_a = signer(iter)?;
    if *player_a.key != m.player_a {
        return Err(ProgramError::MissingRequiredSignature);
    }

    pay(match_info, player_a, m.stake)?;
    m.phase = phase::SETTLED;
    m.winner = side::DRAW;
    Ok(())
}

/// accounts: player (signer), session (owned by the referee program).
/// Points the player's settlement slot at a virgin referee session for
/// this exact match: genesis claim, game program and operator all have
/// to line up, and nothing may have been asserted on it yet. A player
/// can rebind their own slot (a session burned in a lost dispute is
/// dead weight) but can never touch the opponent's.
fn bind(m: &mut Match, iter: &mut core::slice::Iter<AccountInfo>) -> ProgramResult {
    if m.phase != phase::LIVE {
        return Err(err(ERR_PHASE));
    }
    let player = signer(iter)?;
    let session_info = next_account_info(iter)?;
    if *session_info.owner != m.referee_program {
        return Err(ProgramError::IllegalOwner);
    }
    let s = Session::read(&session_info.try_borrow_data()?)?;
    if s.status != referee_status::IDLE
        || s.lo_tick != 0
        || s.hi_tick != 0
        || s.lo_claim != m.genesis_claim
        || s.operator != *player.key
        || s.game_program != m.game_program
    {
        return Err(err(ERR_SESSION));
    }

    if *player.key == m.player_a {
        m.session_a = *session_info.key;
    } else if *player.key == m.player_b {
        m.session_b = *session_info.key;
    } else {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

/// data: one verdict byte. accounts: player a, player b (both signers,
/// writable). Both signatures on the result settle instantly - the
/// referee path exists for when they don't agree.
fn settle_coop(
    m: &mut Match,
    match_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if m.phase != phase::LIVE {
        return Err(err(ERR_PHASE));
    }
    let winner = *data.first().ok_or(ProgramError::InvalidInstructionData)?;
    if winner > side::B {
        return Err(ProgramError::InvalidInstructionData);
    }
    let player_a = signer(iter)?;
    let player_b = signer(iter)?;
    if *player_a.key != m.player_a || *player_b.key != m.player_b {
        return Err(ProgramError::MissingRequiredSignature);
    }

    payout(m, match_info, player_a, player_b, winner)
}

/// The trustless path. data: the full final game state. accounts:
/// session, scratch (writable signer, owned by the game program), game
/// program, player a, player b (both writable).
///
/// Anyone may submit this. The session has to be one of the two bound
/// slots and finalized at exactly the match's final tick - which means
/// its claim survived the referee's challenge window or won its dispute
/// by native replay. The supplied state must hash to that claim; the
/// game program then reads the state and names the winner itself.
fn settle(
    m: &mut Match,
    match_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
    state: &[u8],
) -> ProgramResult {
    if m.phase != phase::LIVE {
        return Err(err(ERR_PHASE));
    }
    let session_info = next_account_info(iter)?;
    let scratch = next_account_info(iter)?;
    let game_program = next_account_info(iter)?;
    if *game_program.key != m.game_program || scratch.owner != game_program.key {
        return Err(ProgramError::InvalidAccountData);
    }
    if *session_info.owner != m.referee_program {
        return Err(ProgramError::IllegalOwner);
    }

    let bound = *session_info.key != Pubkey::default()
        && (*session_info.key == m.session_a || *session_info.key == m.session_b);
    if !bound {
        return Err(err(ERR_SESSION));
    }
    let s = Session::read(&session_info.try_borrow_data()?)?;
    if s.status != referee_status::IDLE
        || s.lo_tick != m.final_tick
        || s.hi_tick != m.final_tick
    {
        return Err(err(ERR_UNPROVEN));
    }
    if tick_merkle::state_root(state) != s.lo_claim[..32] {
        return Err(err(ERR_CLAIM));
    }

    // seed the scratch with the proven final state, then ask the game
    let mut load = Vec::with_capacity(1 + state.len());
    load.push(1);
    load.extend_from_slice(state);
    invoke(
        &Instruction {
            program_id: *game_program.key,
            accounts: vec![AccountMeta::new(*scratch.key, true)],
            data: load,
        },
        &[scratch.clone(), game_program.clone()],
    )?;
    invoke(
        &Instruction {
            program_id: *game_program.key,
            accounts: vec![AccountMeta::new_readonly(*scratch.key, false)],
            data: vec![2],
        },
        &[scratch.clone(), game_program.clone()],
    )?;
    let (returner, verdict) = get_return_data().ok_or(err(ERR_VERDICT))?;
    if returner != *game_program.key || verdict.len() != 1 || verdict[0] > side::B {
        return Err(err(ERR_VERDICT));
    }

    let player_a = next_account_info(iter)?;
    let player_b = next_account_info(iter)?;
    payout(m, match_info, player_a, player_b, verdict[0])
}

/// accounts: player a, player b (both writable). A live match nobody
/// settled in time unwinds to a refund - stakes can't get stuck behind
/// a dispute that never resolves.
fn expire(
    m: &mut Match,
    match_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
) -> ProgramResult {
    if m.phase != phase::LIVE {
        return Err(err(ERR_PHASE));
    }
    if Clock::get()?.slot <= m.deadline {
        return Err(err(ERR_NOT_EXPIRED));
    }
    let player_a = next_account_info(iter)?;
    let player_b = next_account_info(iter)?;
    payout(m, match_info, player_a, player_b, side::DRAW)
}

/// Winner takes both stakes, a draw refunds each side; rent stays with
/// the match account.
fn payout(
    m: &mut Match,
    match_info: &AccountInfo,
    player_a: &AccountInfo,
    player_b: &AccountInfo,
    winner: u8,
) -> ProgramResult {
    if *player_a.key != m.player_a || *player_b.key != m.player_b {
        return Err(ProgramError::InvalidAccountData);
    }
    match winner {
        side::A => pay(match_info, player_a, m.stake * 2)?,
        side::B => pay(match_info, player_b, m.stake * 2)?,
        _ => {
            pay(match_info, player_a, m.stake)?;
            pay(match_info, player_b, m.stake)?;
        }
    }
    m.phase = phase::SETTLED;
    m.winner = winner;
    Ok(())
}

fn pay(from: &AccountInfo, to: &AccountInfo, lamports: u64) -> ProgramResult {
    **from.try_borrow_mut_lamports()? -= lamports;
    **to.try_borrow_mut_lamports()? += lamports;
    Ok(())
}
