// Pins the TS merkle implementation to the Rust one. The golden root is
// the frozen constant from crates/merkle's `golden_root` test - if the
// two implementations ever disagree, match creation from the browser
// would write genesis claims the engine rejects.

import { sha256 } from "@noble/hashes/sha2";

const CHUNK = 32;

function hashv(parts) {
  const h = sha256.create();
  for (const p of parts) h.update(p);
  return h.digest();
}

function leafCount(len) {
  const chunks = Math.max(Math.ceil(len / CHUNK), 1);
  return 1 << Math.ceil(Math.log2(chunks));
}

function chunkAt(state, index) {
  const chunk = new Uint8Array(CHUNK);
  const start = index * CHUNK;
  if (start < state.length) {
    const end = Math.min(start + CHUNK, state.length);
    chunk.set(state.subarray(start, end));
  }
  return chunk;
}

function stateRoot(state) {
  const n = leafCount(state.length);
  const level = [];
  for (let i = 0; i < n; i++) {
    level.push(hashv([new Uint8Array([0]), chunkAt(state, i)]));
  }
  let width = n;
  while (width > 1) {
    for (let i = 0; i < width / 2; i++) {
      level[i] = hashv([new Uint8Array([1]), level[2 * i], level[2 * i + 1]]);
    }
    width /= 2;
  }
  const lenLe = new Uint8Array(8);
  new DataView(lenLe.buffer).setBigUint64(0, BigInt(state.length), true);
  return hashv([new Uint8Array([2]), lenLe, level[0]]);
}

const hex = (b) =>
  Array.from(b)
    .map((x) => x.toString(16).padStart(2, "0"))
    .join("");

// golden vector: state = (0..=255), root frozen in crates/merkle tests
const state = new Uint8Array(256);
for (let i = 0; i < 256; i++) state[i] = i;
const got = hex(stateRoot(state));
const want =
  "5e74a66a9c4a5966e57660355611e1223aa675827fd78598b5db5f4ea091c9b8";

if (got !== want) {
  console.error(`MISMATCH\n got  ${got}\n want ${want}`);
  process.exit(1);
}
console.log("golden root matches Rust:", got);

// print the arena genesis claim for reference
const STATE_SIZE = 8 + 8 * 32;
const genesis = new Uint8Array(STATE_SIZE);
const view = new DataView(genesis.buffer);
for (let i = 0; i < 8; i++) {
  const base = 8 + i * 32;
  const p = BigInt(32 + i * 28) << 32n;
  view.setBigInt64(base, p, true);
  view.setBigInt64(base + 8, p, true);
}
console.log("arena genesis root:", hex(stateRoot(genesis)));
