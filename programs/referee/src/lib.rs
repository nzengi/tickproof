//! Dispute referee. An operator runs the game off-chain and periodically
//! asserts checkpoints here. Anyone who disagrees posts a bond and the two
//! sides bisect over the tick range until they pin down the single tick
//! where they diverge. That tick is then replayed on-chain - a CPI into
//! the actual game program, the same SBF the operator ran off-chain - and
//! whoever's claim matches the replayed result takes both bonds.
//!
//! A claim is 64 bytes: the state merkle root followed by the input chain
//! value at that tick. Committing the inputs alongside the state is what
//! keeps the operator from inventing a convenient input log once a
//! dispute is underway.
//!
//! One deliberate asymmetry: if the disputed tick won't execute at all
//! (the operator committed to inputs the game program rejects), the
//! replay transaction can never land, the deadline runs out, and the
//! challenger wins by timeout. Burden of proof sits with the asserter.

#[cfg(not(feature = "no-entrypoint"))]
use solana_program::entrypoint;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

pub const SESSION_LEN: usize = 344;

/// Slots the operator's assertion must sit unchallenged before it can
/// be finalized, and slots each side gets per dispute move. Prototype
/// numbers, loose enough to drive a dispute over devnet RPC by hand -
/// mainnet values would be hours, not minutes.
pub const CHALLENGE_WINDOW_SLOTS: u64 = 64;
pub const TURN_SLOTS: u64 = 150;

pub mod status {
    pub const IDLE: u8 = 0;
    pub const BISECTING: u8 = 1;
    pub const AWAITING_REPLAY: u8 = 2;
    pub const RESOLVED: u8 = 3;
}

pub mod party {
    pub const NONE: u8 = 0;
    pub const OPERATOR: u8 = 1;
    pub const CHALLENGER: u8 = 2;
}

/// state merkle root (32) followed by input chain (32)
pub type Claim = [u8; 64];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub status: u8,
    pub winner: u8,
    pub turn: u8,
    pub operator: Pubkey,
    pub challenger: Pubkey,
    pub game_program: Pubkey,
    pub bond: u64,
    pub posted_slot: u64,
    pub deadline: u64,
    pub lo_tick: u64,
    pub lo_claim: Claim,
    pub hi_tick: u64,
    pub hi_claim: Claim,
    pub mid_tick: u64,
    pub mid_claim: Claim,
}

impl Session {
    pub fn read(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != SESSION_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let pk = |o: usize| Pubkey::new_from_array(data[o..o + 32].try_into().unwrap());
        let u64le = |o: usize| u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
        let claim = |o: usize| -> Claim { data[o..o + 64].try_into().unwrap() };
        Ok(Self {
            status: data[0],
            winner: data[1],
            turn: data[2],
            operator: pk(8),
            challenger: pk(40),
            game_program: pk(72),
            bond: u64le(104),
            posted_slot: u64le(112),
            deadline: u64le(120),
            lo_tick: u64le(128),
            lo_claim: claim(136),
            hi_tick: u64le(200),
            hi_claim: claim(208),
            mid_tick: u64le(272),
            mid_claim: claim(280),
        })
    }

    pub fn write(&self, data: &mut [u8]) {
        data[0] = self.status;
        data[1] = self.winner;
        data[2] = self.turn;
        data[8..40].copy_from_slice(self.operator.as_ref());
        data[40..72].copy_from_slice(self.challenger.as_ref());
        data[72..104].copy_from_slice(self.game_program.as_ref());
        data[104..112].copy_from_slice(&self.bond.to_le_bytes());
        data[112..120].copy_from_slice(&self.posted_slot.to_le_bytes());
        data[120..128].copy_from_slice(&self.deadline.to_le_bytes());
        data[128..136].copy_from_slice(&self.lo_tick.to_le_bytes());
        data[136..200].copy_from_slice(&self.lo_claim);
        data[200..208].copy_from_slice(&self.hi_tick.to_le_bytes());
        data[208..272].copy_from_slice(&self.hi_claim);
        data[272..280].copy_from_slice(&self.mid_tick.to_le_bytes());
        data[280..344].copy_from_slice(&self.mid_claim);
    }
}

// protocol violations; everything else maps to builtin ProgramErrors
const ERR_STATUS: u32 = 0; // instruction not valid in current status
const ERR_TURN: u32 = 1; // not this party's move
const ERR_PENDING: u32 = 2; // an assertion is already pending
const ERR_WINDOW: u32 = 3; // challenge window still open
const ERR_DEADLINE: u32 = 4; // move arrived after the deadline
const ERR_NOT_EXPIRED: u32 = 5; // timeout called before the deadline
const ERR_CLAIM: u32 = 6; // supplied data doesn't match a committed claim

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
    let session_info = next_account_info(iter)?;
    if session_info.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }
    let mut session = Session::read(&session_info.try_borrow_data()?)?;

    match tag {
        0 => init(&mut session, iter, rest)?,
        1 => checkpoint(&mut session, iter, rest)?,
        2 => finalize(&mut session)?,
        3 => challenge(&mut session, session_info, iter)?,
        4 => bisect(&mut session, iter, rest)?,
        5 => pick(&mut session, iter, rest)?,
        6 => replay(&mut session, session_info, iter, rest)?,
        7 => timeout(&mut session, session_info, iter)?,
        _ => return Err(ProgramError::InvalidInstructionData),
    }

    session.write(&mut session_info.try_borrow_mut_data()?);
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

fn parse_claim(data: &[u8]) -> Result<Claim, ProgramError> {
    data.try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)
}

/// data: bond u64, genesis claim. accounts: operator (signer), game program.
/// The session account is created and funded with rent + bond beforehand;
/// all-zero data is what marks it as uninitialized.
fn init(
    session: &mut Session,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if data.len() != 8 + 64 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if session.operator != Pubkey::default() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let operator = signer(iter)?;
    let game_program = next_account_info(iter)?;
    if !game_program.executable {
        return Err(ProgramError::InvalidAccountData);
    }

    session.operator = *operator.key;
    session.game_program = *game_program.key;
    session.bond = u64::from_le_bytes(data[..8].try_into().unwrap());
    session.lo_claim = parse_claim(&data[8..])?;
    session.hi_claim = session.lo_claim;
    Ok(())
}

fn require_bonded(session: &Session, info: &AccountInfo, sides: u64) -> ProgramResult {
    let rent = Rent::get()?.minimum_balance(SESSION_LEN);
    let needed = rent + session.bond * sides;
    if info.lamports() < needed {
        return Err(ProgramError::InsufficientFunds);
    }
    Ok(())
}

/// data: tick u64, claim. Only one assertion can be in flight; finalize
/// the previous one first.
fn checkpoint(
    session: &mut Session,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if data.len() != 8 + 64 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if session.status != status::IDLE {
        return Err(err(ERR_STATUS));
    }
    if session.hi_tick != session.lo_tick {
        return Err(err(ERR_PENDING));
    }
    let operator = signer(iter)?;
    if *operator.key != session.operator {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let tick = u64::from_le_bytes(data[..8].try_into().unwrap());
    if tick <= session.lo_tick {
        return Err(ProgramError::InvalidArgument);
    }

    session.hi_tick = tick;
    session.hi_claim = parse_claim(&data[8..])?;
    session.posted_slot = Clock::get()?.slot;
    Ok(())
}

fn finalize(session: &mut Session) -> ProgramResult {
    if session.status != status::IDLE || session.hi_tick == session.lo_tick {
        return Err(err(ERR_STATUS));
    }
    if Clock::get()?.slot < session.posted_slot + CHALLENGE_WINDOW_SLOTS {
        return Err(err(ERR_WINDOW));
    }
    session.lo_tick = session.hi_tick;
    session.lo_claim = session.hi_claim;
    Ok(())
}

/// accounts: challenger (signer). The challenger tops the session up with
/// their bond beforehand; the balance check is the enforcement.
fn challenge(
    session: &mut Session,
    session_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
) -> ProgramResult {
    if session.status != status::IDLE || session.hi_tick == session.lo_tick {
        return Err(err(ERR_STATUS));
    }
    let challenger = signer(iter)?;
    require_bonded(session, session_info, 2)?;

    session.challenger = *challenger.key;
    session.deadline = Clock::get()?.slot + TURN_SLOTS;
    if session.hi_tick - session.lo_tick == 1 {
        session.status = status::AWAITING_REPLAY;
    } else {
        session.status = status::BISECTING;
        session.turn = party::OPERATOR;
    }
    Ok(())
}

/// data: claim at the midpoint of [lo, hi]. Operator's move.
fn bisect(
    session: &mut Session,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if session.status != status::BISECTING {
        return Err(err(ERR_STATUS));
    }
    if session.turn != party::OPERATOR {
        return Err(err(ERR_TURN));
    }
    let operator = signer(iter)?;
    if *operator.key != session.operator {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let now = Clock::get()?.slot;
    if now > session.deadline {
        return Err(err(ERR_DEADLINE));
    }

    session.mid_tick = session.lo_tick + (session.hi_tick - session.lo_tick) / 2;
    session.mid_claim = parse_claim(data)?;
    session.turn = party::CHALLENGER;
    session.deadline = now + TURN_SLOTS;
    Ok(())
}

/// data: one byte, nonzero = the challenger agrees with the midpoint
/// claim. Each pick halves the interval; at width one the dispute moves
/// to replay.
fn pick(
    session: &mut Session,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if session.status != status::BISECTING {
        return Err(err(ERR_STATUS));
    }
    if session.turn != party::CHALLENGER {
        return Err(err(ERR_TURN));
    }
    let challenger = signer(iter)?;
    if *challenger.key != session.challenger {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let now = Clock::get()?.slot;
    if now > session.deadline {
        return Err(err(ERR_DEADLINE));
    }
    let agree = *data.first().ok_or(ProgramError::InvalidInstructionData)? != 0;

    if agree {
        session.lo_tick = session.mid_tick;
        session.lo_claim = session.mid_claim;
    } else {
        session.hi_tick = session.mid_tick;
        session.hi_claim = session.mid_claim;
    }
    session.deadline = now + TURN_SLOTS;
    if session.hi_tick - session.lo_tick == 1 {
        session.status = status::AWAITING_REPLAY;
    } else {
        session.turn = party::OPERATOR;
    }
    Ok(())
}

/// The native one-step proof. data: inputs len u32, inputs, then the full
/// pre-state. accounts: scratch (writable signer, owned by the game
/// program), game program, operator, challenger.
///
/// Anyone may submit this - the outcome is decided by execution, not by
/// who called it. The pre-state must hash to the agreed lo claim and the
/// inputs must extend the lo input chain to the asserted hi chain; the
/// game program then runs the tick for real and the resulting root either
/// matches the asserted hi root or it doesn't.
fn replay(
    session: &mut Session,
    session_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
    data: &[u8],
) -> ProgramResult {
    if session.status != status::AWAITING_REPLAY {
        return Err(err(ERR_STATUS));
    }
    if data.len() < 4 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let inputs_len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    let inputs = data
        .get(4..4 + inputs_len)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let pre_state = &data[4 + inputs_len..];

    let scratch = next_account_info(iter)?;
    let game_program = next_account_info(iter)?;
    if *game_program.key != session.game_program || scratch.owner != game_program.key {
        return Err(ProgramError::InvalidAccountData);
    }

    if tick_merkle::state_root(pre_state) != session.lo_claim[..32] {
        return Err(err(ERR_CLAIM));
    }
    let lo_chain: [u8; 32] = session.lo_claim[32..].try_into().unwrap();
    if tick_merkle::extend_input_chain(&lo_chain, inputs) != session.hi_claim[32..] {
        return Err(err(ERR_CLAIM));
    }

    // seed the scratch with the agreed pre-state, then run the tick
    let mut load = Vec::with_capacity(1 + pre_state.len());
    load.push(1);
    load.extend_from_slice(pre_state);
    invoke(
        &Instruction {
            program_id: *game_program.key,
            accounts: vec![AccountMeta::new(*scratch.key, true)],
            data: load,
        },
        &[scratch.clone(), game_program.clone()],
    )?;

    let mut tick = Vec::with_capacity(9 + inputs.len());
    tick.push(0);
    tick.extend_from_slice(&session.lo_tick.to_le_bytes());
    tick.extend_from_slice(inputs);
    invoke(
        &Instruction {
            program_id: *game_program.key,
            accounts: vec![AccountMeta::new(*scratch.key, false)],
            data: tick,
        },
        &[scratch.clone(), game_program.clone()],
    )?;

    let post_root = tick_merkle::state_root(&scratch.try_borrow_data()?);
    let winner = if post_root == session.hi_claim[..32] {
        party::OPERATOR
    } else {
        party::CHALLENGER
    };
    resolve(session, session_info, iter, winner)
}

/// Whoever was supposed to move and didn't loses; a stalled replay phase
/// counts against the operator, who is the one defending an assertion.
fn timeout(
    session: &mut Session,
    session_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
) -> ProgramResult {
    let loser = match session.status {
        status::BISECTING => session.turn,
        status::AWAITING_REPLAY => party::OPERATOR,
        _ => return Err(err(ERR_STATUS)),
    };
    if Clock::get()?.slot <= session.deadline {
        return Err(err(ERR_NOT_EXPIRED));
    }
    let winner = if loser == party::OPERATOR {
        party::CHALLENGER
    } else {
        party::OPERATOR
    };
    resolve(session, session_info, iter, winner)
}

/// Remaining accounts: operator, challenger (both writable). The winner
/// takes both bonds; rent stays with the session.
fn resolve(
    session: &mut Session,
    session_info: &AccountInfo,
    iter: &mut core::slice::Iter<AccountInfo>,
    winner: u8,
) -> ProgramResult {
    let operator = next_account_info(iter)?;
    let challenger = next_account_info(iter)?;
    if *operator.key != session.operator || *challenger.key != session.challenger {
        return Err(ProgramError::InvalidAccountData);
    }
    let payee = if winner == party::OPERATOR {
        operator
    } else {
        challenger
    };

    let pot = session.bond * 2;
    **session_info.try_borrow_mut_lamports()? -= pot;
    **payee.try_borrow_mut_lamports()? += pot;

    session.status = status::RESOLVED;
    session.winner = winner;
    Ok(())
}
