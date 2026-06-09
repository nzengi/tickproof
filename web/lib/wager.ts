// Match account layout and instruction builders for programs/wager.
// Byte-for-byte the same layout as `Match::read` / `Match::write`.

import { Buffer } from "buffer";
import {
  AccountMeta,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";
import {
  ARENA_PROGRAM_ID,
  MATCH_LEN,
  REFEREE_PROGRAM_ID,
  WAGER_PROGRAM_ID,
} from "./constants";
import { genesisClaim } from "./merkle";

export const PHASE = { OPEN: 0, LIVE: 1, SETTLED: 2 } as const;
export const PHASE_NAME = ["open", "live", "settled"] as const;
export const SIDE_NAME = ["draw", "player A", "player B"] as const;

export interface MatchAccount {
  pubkey: PublicKey;
  lamports: number;
  phase: number;
  winner: number;
  playerA: PublicKey;
  playerB: PublicKey;
  gameProgram: PublicKey;
  refereeProgram: PublicKey;
  sessionA: PublicKey;
  sessionB: PublicKey;
  stake: bigint;
  finalTick: bigint;
  deadlineSlots: bigint;
  deadline: bigint;
  genesisClaim: Uint8Array;
}

export function decodeMatch(
  pubkey: PublicKey,
  lamports: number,
  data: Uint8Array,
): MatchAccount | null {
  if (data.length !== MATCH_LEN) return null;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const pk = (o: number) => new PublicKey(data.subarray(o, o + 32));
  return {
    pubkey,
    lamports,
    phase: data[0],
    winner: data[1],
    playerA: pk(8),
    playerB: pk(40),
    gameProgram: pk(72),
    refereeProgram: pk(104),
    sessionA: pk(136),
    sessionB: pk(168),
    stake: view.getBigUint64(200, true),
    finalTick: view.getBigUint64(208, true),
    deadlineSlots: view.getBigUint64(216, true),
    deadline: view.getBigUint64(224, true),
    genesisClaim: data.subarray(232, 296),
  };
}

function ix(accounts: AccountMeta[], data: Uint8Array): TransactionInstruction {
  return new TransactionInstruction({
    programId: WAGER_PROGRAM_ID,
    keys: accounts,
    data: Buffer.from(data),
  });
}

function u64le(v: bigint): Uint8Array {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, v, true);
  return b;
}

/// System create (rent + stake, owned by the wager program) followed by
/// the create instruction. The match account is a fresh throwaway keypair.
export function createMatchIxs(opts: {
  payer: PublicKey;
  matchPubkey: PublicKey;
  playerA: PublicKey;
  playerB: PublicKey;
  stake: bigint;
  finalTick: bigint;
  deadlineSlots: bigint;
  rentLamports: number;
}): TransactionInstruction[] {
  const data = new Uint8Array(1 + 8 + 8 + 8 + 64);
  data[0] = 0;
  data.set(u64le(opts.stake), 1);
  data.set(u64le(opts.finalTick), 9);
  data.set(u64le(opts.deadlineSlots), 17);
  data.set(genesisClaim(), 25);
  return [
    SystemProgram.createAccount({
      fromPubkey: opts.payer,
      newAccountPubkey: opts.matchPubkey,
      lamports: opts.rentLamports + Number(opts.stake),
      space: MATCH_LEN,
      programId: WAGER_PROGRAM_ID,
    }),
    ix(
      [
        { pubkey: opts.matchPubkey, isSigner: false, isWritable: true },
        { pubkey: opts.playerA, isSigner: true, isWritable: false },
        { pubkey: opts.playerB, isSigner: false, isWritable: false },
        { pubkey: ARENA_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: REFEREE_PROGRAM_ID, isSigner: false, isWritable: false },
      ],
      data,
    ),
  ];
}

/// Player B tops the match up with their stake and joins in one tx.
export function joinIxs(m: MatchAccount): TransactionInstruction[] {
  return [
    SystemProgram.transfer({
      fromPubkey: m.playerB,
      toPubkey: m.pubkey,
      lamports: Number(m.stake),
    }),
    ix(
      [
        { pubkey: m.pubkey, isSigner: false, isWritable: true },
        { pubkey: m.playerB, isSigner: true, isWritable: false },
      ],
      new Uint8Array([1]),
    ),
  ];
}

export function cancelIx(m: MatchAccount): TransactionInstruction {
  return ix(
    [
      { pubkey: m.pubkey, isSigner: false, isWritable: true },
      { pubkey: m.playerA, isSigner: true, isWritable: true },
    ],
    new Uint8Array([2]),
  );
}

/// Both players sign the result byte; either side can submit.
export function coopSettleIx(
  m: MatchAccount,
  winner: number,
): TransactionInstruction {
  return ix(
    [
      { pubkey: m.pubkey, isSigner: false, isWritable: true },
      { pubkey: m.playerA, isSigner: true, isWritable: true },
      { pubkey: m.playerB, isSigner: true, isWritable: true },
    ],
    new Uint8Array([4, winner]),
  );
}

/// Anyone can unwind a live match after the deadline.
export function expireIx(m: MatchAccount): TransactionInstruction {
  return ix(
    [
      { pubkey: m.pubkey, isSigner: false, isWritable: true },
      { pubkey: m.playerA, isSigner: false, isWritable: true },
      { pubkey: m.playerB, isSigner: false, isWritable: true },
    ],
    new Uint8Array([6]),
  );
}

export function short(pk: PublicKey): string {
  const s = pk.toBase58();
  return `${s.slice(0, 4)}..${s.slice(-4)}`;
}

export function lamportsToSol(l: bigint | number): string {
  return (Number(l) / 1e9).toLocaleString("en-US", {
    maximumFractionDigits: 4,
  });
}
