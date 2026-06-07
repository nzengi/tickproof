# tickproof

Verifiable game engine for Solana. Game logic compiles to SBF, runs off-chain
at full speed in an embedded VM, and any disputed tick can be replayed by the
chain itself - the L1 already executes SBF natively, so the final step of a
dispute is just a program invocation. No interpreter-in-a-contract, no zkVM.

Early WIP.

- `crates/tick-core` - no_std deterministic tick primitives: Q32.32 fixed
  point, xorshift64* rng, the `TickLogic` trait
- `crates/merkle` - chunked sha256 merkle tree over game state; the same
  verify path runs on-chain
- `crates/runtime` - off-chain engine; drives the SBF build through the
  real agave program runtime (via mollusk), keeps the input log, produces
  tick-indexed state-root checkpoints
- `games/arena` - minimal physics arena used as the reference game
- `programs/arena-program` - thin on-chain wrapper, one instruction = one
  arena tick

```
cd programs/arena-program && cargo build-sbf && cd ../..
cargo test
```

Current numbers: an arena tick costs at most ~2000 CU under the agave
runtime, and the SBF build matches the native build bit for bit over a
thousand ticks of randomized input.

Next: the on-chain referee (checkpoint, bisection, merkle-proven
single-tick replay).
