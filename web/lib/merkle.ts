// TypeScript twin of crates/merkle. Same chunking, same domain tags,
// same length commitment - it has to be, the genesis claim the dApp
// writes into a match account must equal what the Rust engine computes.
// `scripts/check-merkle.mjs` pins this against the Rust golden vector.

import { sha256 } from "@noble/hashes/sha2";
import { STATE_SIZE } from "./constants";

export const CHUNK = 32;

const LEAF_TAG = 0;
const NODE_TAG = 1;
const ROOT_TAG = 2;

function hashv(parts: Uint8Array[]): Uint8Array {
  const h = sha256.create();
  for (const p of parts) h.update(p);
  return h.digest();
}

export function leafCount(len: number): number {
  const chunks = Math.max(Math.ceil(len / CHUNK), 1);
  return 1 << Math.ceil(Math.log2(chunks));
}

export function chunkAt(state: Uint8Array, index: number): Uint8Array {
  const chunk = new Uint8Array(CHUNK);
  const start = index * CHUNK;
  if (start < state.length) {
    const end = Math.min(start + CHUNK, state.length);
    chunk.set(state.subarray(start, end));
  }
  return chunk;
}

export function stateRoot(state: Uint8Array): Uint8Array {
  const n = leafCount(state.length);
  const level: Uint8Array[] = [];
  for (let i = 0; i < n; i++) {
    level.push(hashv([new Uint8Array([LEAF_TAG]), chunkAt(state, i)]));
  }
  let width = n;
  while (width > 1) {
    for (let i = 0; i < width / 2; i++) {
      level[i] = hashv([new Uint8Array([NODE_TAG]), level[2 * i], level[2 * i + 1]]);
    }
    width /= 2;
  }
  const lenLe = new Uint8Array(8);
  new DataView(lenLe.buffer).setBigUint64(0, BigInt(state.length), true);
  return hashv([new Uint8Array([ROOT_TAG]), lenLe, level[0]]);
}

/// Arena genesis, mirroring `Arena::init`: balls spread along the
/// diagonal at (32 + i*28, ...) in Q32.32, everything at rest.
export function arenaGenesisState(): Uint8Array {
  const state = new Uint8Array(STATE_SIZE);
  const view = new DataView(state.buffer);
  for (let i = 0; i < 8; i++) {
    const base = 8 + i * 32;
    const p = BigInt(32 + i * 28) << 32n;
    view.setBigInt64(base, p, true);
    view.setBigInt64(base + 8, p, true);
  }
  return state;
}

/// Genesis claim: state root of the genesis state followed by the
/// all-zero input chain.
export function genesisClaim(): Uint8Array {
  const claim = new Uint8Array(64);
  claim.set(stateRoot(arenaGenesisState()), 0);
  return claim;
}
