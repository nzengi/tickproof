//! State commitments. The state is split into fixed 32-byte chunks and
//! hashed into a binary sha256 tree; the state length is committed at the
//! top so trailing zeros can't be confused with padding. Verification has
//! to run inside the referee program too, so it stays cheap: one sha256
//! per level, depth is log2 of the chunk count.
//!
//! Hashes are domain-separated with a one-byte tag (leaf / node / root)
//! to rule out second-preimage games between levels.

use solana_sha256_hasher::hashv;

pub const CHUNK: usize = 32;

const LEAF_TAG: &[u8] = &[0];
const NODE_TAG: &[u8] = &[1];
const ROOT_TAG: &[u8] = &[2];
const INPUT_TAG: &[u8] = &[3];

fn leaf_hash(chunk: &[u8; CHUNK]) -> [u8; 32] {
    hashv(&[LEAF_TAG, chunk]).to_bytes()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    hashv(&[NODE_TAG, left, right]).to_bytes()
}

fn root_hash(state_len: u64, tree: &[u8; 32]) -> [u8; 32] {
    hashv(&[ROOT_TAG, &state_len.to_le_bytes(), tree]).to_bytes()
}

/// Number of leaves for a state of `len` bytes: chunk count rounded up
/// to the next power of two, never less than one.
pub fn leaf_count(len: usize) -> usize {
    len.div_ceil(CHUNK).max(1).next_power_of_two()
}

/// The chunk at `index`, zero-padded. Chunks past the end of the state
/// read as all zero; the length commitment in the root is what keeps
/// that unambiguous.
pub fn chunk_at(state: &[u8], index: usize) -> [u8; CHUNK] {
    let mut chunk = [0u8; CHUNK];
    let start = index * CHUNK;
    if start < state.len() {
        let end = (start + CHUNK).min(state.len());
        chunk[..end - start].copy_from_slice(&state[start..end]);
    }
    chunk
}

fn leaves(state: &[u8]) -> Vec<[u8; 32]> {
    (0..leaf_count(state.len()))
        .map(|i| leaf_hash(&chunk_at(state, i)))
        .collect()
}

fn fold_level(level: &[[u8; 32]]) -> Vec<[u8; 32]> {
    level
        .chunks(2)
        .map(|pair| node_hash(&pair[0], &pair[1]))
        .collect()
}

pub fn state_root(state: &[u8]) -> [u8; 32] {
    let mut level = leaves(state);
    while level.len() > 1 {
        level = fold_level(&level);
    }
    root_hash(state.len() as u64, &level[0])
}

/// Rolling commitment over the input log: chain after tick t+1 is
/// H(tag, chain after t, inputs of t), starting from all zero. A state
/// root alone doesn't pin down which inputs produced it; without this
/// the asserter could invent inputs at replay time.
pub fn extend_input_chain(prev: &[u8; 32], inputs: &[u8]) -> [u8; 32] {
    hashv(&[INPUT_TAG, prev, inputs]).to_bytes()
}

/// Inclusion proof for one chunk. `siblings` runs leaf to root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub state_len: u64,
    pub index: u32,
    pub siblings: Vec<[u8; 32]>,
}

pub fn prove(state: &[u8], index: usize) -> Proof {
    let n = leaf_count(state.len());
    assert!(index < n, "chunk index {index} out of range ({n} leaves)");

    let mut level = leaves(state);
    let mut siblings = Vec::new();
    let mut idx = index;
    while level.len() > 1 {
        siblings.push(level[idx ^ 1]);
        level = fold_level(&level);
        idx >>= 1;
    }
    Proof {
        state_len: state.len() as u64,
        index: index as u32,
        siblings,
    }
}

/// Check that `chunk` sits at `proof.index` under `root`. The final
/// `idx == 0` check rejects indices wider than the tree.
pub fn verify(root: &[u8; 32], chunk: &[u8; CHUNK], proof: &Proof) -> bool {
    let mut h = leaf_hash(chunk);
    let mut idx = proof.index;
    for sibling in &proof.siblings {
        h = if idx & 1 == 0 {
            node_hash(&h, sibling)
        } else {
            node_hash(sibling, &h)
        };
        idx >>= 1;
    }
    idx == 0 && root_hash(proof.state_len, &h) == *root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_chunk_proof_verifies() {
        let state: Vec<u8> = (0..264u32).map(|i| (i * 7 + 3) as u8).collect();
        let root = state_root(&state);
        for i in 0..leaf_count(state.len()) {
            let proof = prove(&state, i);
            assert!(verify(&root, &chunk_at(&state, i), &proof), "chunk {i}");
        }
    }

    #[test]
    fn tampered_chunk_fails() {
        let state = vec![0xABu8; 100];
        let root = state_root(&state);
        let proof = prove(&state, 1);
        let mut chunk = chunk_at(&state, 1);
        chunk[0] ^= 1;
        assert!(!verify(&root, &chunk, &proof));
    }

    #[test]
    fn wrong_index_fails() {
        // chunks must differ here, otherwise swapping siblings is a no-op
        let state: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
        let root = state_root(&state);
        let mut proof = prove(&state, 2);
        proof.index = 3;
        assert!(!verify(&root, &chunk_at(&state, 2), &proof));
    }

    #[test]
    fn oversized_index_fails() {
        let state = vec![1u8; 64];
        let root = state_root(&state);
        let mut proof = prove(&state, 0);
        // same path bits, but pretends the tree is deeper than it is
        proof.index += leaf_count(state.len()) as u32;
        assert!(!verify(&root, &chunk_at(&state, 0), &proof));
    }

    #[test]
    fn length_is_committed() {
        // all-zero states of different sizes hash to identical trees;
        // only the length commitment tells them apart
        assert_ne!(state_root(&[0u8; 32]), state_root(&[0u8; 64]));
        assert_ne!(state_root(&[]), state_root(&[0u8; 1]));
    }

    #[test]
    fn single_byte_flip_changes_root() {
        let mut state = vec![5u8; 264];
        let before = state_root(&state);
        state[263] ^= 0x80;
        assert_ne!(before, state_root(&state));
    }

    #[test]
    fn input_chain_is_order_and_content_sensitive() {
        let zero = [0u8; 32];
        let a = extend_input_chain(&zero, b"a");
        let b = extend_input_chain(&zero, b"b");
        assert_ne!(a, b);
        assert_ne!(extend_input_chain(&a, b"b"), extend_input_chain(&b, b"a"));
        // an empty tick still advances the chain
        assert_ne!(extend_input_chain(&zero, &[]), zero);
    }

    // Frozen so an accidental change to chunking, tags or padding shows
    // up as a test failure instead of silently breaking old commitments.
    #[test]
    fn golden_root() {
        let state: Vec<u8> = (0..=255u8).collect();
        let root = state_root(&state);
        let expected: [u8; 32] = [
            0x5e, 0x74, 0xa6, 0x6a, 0x9c, 0x4a, 0x59, 0x66, 0xe5, 0x76, 0x60, 0x35, 0x56, 0x11,
            0xe1, 0x22, 0x3a, 0xa6, 0x75, 0x82, 0x7f, 0xd7, 0x85, 0x98, 0xb5, 0xdb, 0x5f, 0x4e,
            0xa0, 0x91, 0xc9, 0xb8,
        ];
        assert_eq!(root, expected, "root drifted: {root:02x?}");
    }
}
