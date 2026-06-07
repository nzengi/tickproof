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
- `programs/arena-program` - thin on-chain wrapper: a tick instruction
  plus a state-load instruction the referee uses to seed replay scratch
  accounts
- `programs/referee` - the dispute program: operators assert tick-indexed
  checkpoints, a challenger bonds and bisects down to the single tick
  where the parties diverge, and that tick is replayed on-chain via CPI
  into the actual game program

```
cd programs/arena-program && cargo build-sbf && cd ../..
cd programs/referee && cargo build-sbf && cd ../..
cargo test
```

Current numbers, all measured under the real agave runtime via mollusk:

- an arena tick costs at most ~2000 CU; the SBF build matches the native
  build bit for bit over a thousand ticks of randomized input
- the full one-step proof - pre-state root check, input chain check, CPI
  state load, native CPI tick execution, post-state root, payout - lands
  at ~19k CU, about 1.4% of one transaction's compute budget

Next: end-to-end dispute on devnet, throughput numbers, and the
comparison against interpreter-in-contract and zkVM-verifier replay.
