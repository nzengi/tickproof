# tickproof

Verifiable game engine for Solana. Game logic compiles to SBF, runs off-chain
at full speed in an embedded VM, and any disputed tick can be replayed by the
chain itself - the L1 already executes SBF natively, so the final step of a
dispute is just a program invocation. No interpreter-in-a-contract, no zkVM.

Early WIP.

- `crates/tick-core` - no_std deterministic tick primitives: Q32.32 fixed
  point, xorshift64* rng, the `TickLogic` trait
- `games/arena` - minimal physics arena used as the reference game

```
cargo test
cd games/arena && cargo build-sbf
```

More soon: off-chain runtime (Mollusk-based), on-chain referee program,
bisection protocol.
