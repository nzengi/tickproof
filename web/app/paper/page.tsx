import type { Metadata } from "next";
import { EXPLORER, EXPLORER_ADDR } from "@/lib/constants";

export const metadata: Metadata = {
  title: "tickproof - technical paper",
  description:
    "Native re-execution as a fraud proof: trustless settlement of real-time games on Solana.",
};

const REPLAY_TX =
  "3NHghoFCePjdaV9fWgMAyeFuTUz252xYee5K43q1DYP2VwpmWyE6heSHmFq1pgpLnnwyytGaeLmvixXPBARrwVBU";
const SETTLE_TX =
  "66RatuBWpAkJDBYi5JXWiTaNiMsDDxufBAguFueMG2wej1utwU6LXbxzg1qJaKDyhV3XxyhmGXZGFNzU6FNp8hn7";

export default function Paper() {
  return (
    <article className="mx-auto max-w-3xl px-5 py-16 prose-cream">
      <p className="font-[family-name:var(--font-mono)] text-xs uppercase tracking-[0.2em] text-accent-600">
        technical paper - june 2026 - devnet prototype
      </p>
      <h1 className="mt-3 mb-2 font-[family-name:var(--font-display)] text-4xl font-semibold leading-tight tracking-tight">
        Native re-execution as a fraud proof: trustless settlement of
        real-time games on Solana
      </h1>
      <p className="mb-10 text-sm text-ink-400">
        tickproof - verifiable game engine &amp; stake escrow.{" "}
        <a
          href="https://github.com/nzengi/tickproof"
          target="_blank"
          rel="noopener noreferrer"
        >
          Source repository
        </a>
        . All figures reproduce from the repo against Solana devnet.
      </p>

      <h2>Abstract</h2>
      <p>
        Real-time games cannot run on a blockchain - 60 Hz tick rates and
        sub-millisecond input latency are incompatible with global consensus.
        So every &ldquo;play for money&rdquo; product on Solana today runs
        the game on a server and asks the chain to believe the server&apos;s
        result. We describe a system that keeps the game off-chain at full
        speed but makes its outcome <strong>objectively enforceable</strong>:
        game logic is compiled once to SBF, executed off-chain inside the
        real Solana program runtime, and committed to with Merkle state
        roots. In a dispute, an interactive bisection narrows disagreement to
        a single tick, and that tick is re-executed <em>natively by the L1
        itself</em> - the same SBF bytecode, as an ordinary program
        invocation. On top of this we build a stake escrow in which two
        players wager real funds on a match and no server, oracle, or
        counterparty is ever trusted with the pot. The complete fraud proof
        costs ~19k compute units (about 1.4% of one transaction&apos;s
        budget); a full adversarial settlement - lie, challenge, bisection,
        native replay, payout - completed in 20 transactions over 47 seconds
        on devnet.
      </p>

      <h2>1. The problem</h2>
      <p>
        A two-player wager has a simple shape: both sides lock a stake, a
        game produces a winner, the winner takes the pot. The hard part is
        the middle. Whoever reports the result can steal the pot, so the
        reporter must be trusted - and on Solana today that reporter is
        always a backend server, sometimes dressed up as an oracle or a
        multisig. The result is custodial risk wearing a decentralization
        costume.
      </p>
      <p>There are three known ways out, and two of them do not work for real-time games:</p>
      <ul>
        <li>
          <strong>Run the game on-chain.</strong> Fine for chess and
          turn-based games; impossible for anything with a tick rate. Block
          times and fees rule out 60 updates per second, and inputs would
          leak to the mempool.
        </li>
        <li>
          <strong>Prove execution with a zkVM.</strong> Proving costs are
          orders of magnitude above real-time budgets, and verification is
          not cheap either: a single SP1 Groth16 verification costs ~280k CU
          on Solana - more than an entire transaction&apos;s default budget -
          before proving a single tick.
        </li>
        <li>
          <strong>Optimistic verification with fraud proofs.</strong> Commit
          to execution, allow challenges, re-execute only the disputed step.
          This is the rollup playbook - but rollups must build an interpreter
          or zk circuit for their VM because their L1 cannot execute their
          state transition natively.
        </li>
      </ul>
      <p>
        The observation behind tickproof:{" "}
        <strong>
          on Solana, the L1 already executes the exact VM the game runs in.
        </strong>{" "}
        If game logic is SBF bytecode, the chain can re-execute any disputed
        tick as a plain CPI - no interpreter-in-a-contract (we measured one:
        80+ CU per emulated instruction, so an interpreted tick starts at
        ~157k CU before memory proofs), no proving infrastructure. The fraud
        proof is the program itself.
      </p>

      <h2>2. Deterministic tick programs</h2>
      <p>
        Everything rests on bit-exact replay, so the game kernel is
        deliberately austere. A game is a pure state transition:
      </p>
      <pre>
        <code>{`state' = tick(state, inputs, tick_index)`}</code>
      </pre>
      <p>
        written in <code>no_std</code> Rust with no floats, no heap, no
        clock, no host entropy (<code>crates/tick-core</code>). Numerics are
        Q32.32 fixed point; randomness, if a game wants it, is a seeded
        xorshift64* whose seed is part of the state. The same crate compiles
        to both native code and SBF, and the test suite pins them to each
        other: a thousand ticks of randomized input produce bit-identical
        state in both builds, and a frozen golden hash catches semantic
        drift.
      </p>
      <p>
        The reference game (<code>games/arena</code>) is a small physics
        arena - eight balls, impulse inputs, wall bounces, friction - chosen
        to exercise the pipeline, not to be fun. One arena tick costs at most
        ~2,000 CU under the real runtime, and the off-chain engine pushes
        ~17k ticks/s through the full pipeline on one core - room for
        hundreds of simultaneous 60 Hz sessions.
      </p>

      <h2>3. Commitments: state roots and the input chain</h2>
      <p>
        The engine (<code>crates/runtime</code>) drives the SBF build
        through the actual agave program runtime (via mollusk) and
        periodically emits checkpoints. A checkpoint commits to two things:
      </p>
      <ul>
        <li>
          <strong>State root.</strong> The game state is split into 32-byte
          chunks and folded into a binary SHA-256 Merkle tree with
          domain-separated leaf/node/root tags; the state length is committed
          at the root so zero-padding cannot be confused with content. Root
          computation costs ~11 CU/byte on-chain, so even an 8 KB game state
          keeps verification under 15% of a transaction budget.
        </li>
        <li>
          <strong>Input chain.</strong> A rolling hash{" "}
          <code>chain&apos; = H(tag, chain, inputs_t)</code> over the input
          log. A state root alone does not pin down <em>which inputs</em>{" "}
          produced it; without the chain, a dishonest asserter could invent a
          convenient input log at replay time.
        </li>
      </ul>
      <p>
        A claim is therefore 64 bytes - state root plus input chain - and
        the claim at tick 0 (the <em>genesis claim</em>) is computable by
        anyone, including this website, which derives it in the browser
        byte-for-byte when opening a match.
      </p>

      <h2>4. The referee: bisection to a single tick</h2>
      <p>
        The referee program (<code>programs/referee</code>) runs the
        optimistic game. An operator asserts a checkpoint at some tick with a
        bond. If nobody objects within the challenge window, it finalizes.
        If a challenger bonds and disagrees, the two parties play an
        interactive bisection: the operator publishes the claim at the
        midpoint of the disputed range, the challenger says &ldquo;agree
        below / disagree above&rdquo; (or vice versa), and the range halves.
        After log<sub>2</sub>(n) rounds the disagreement is exactly one tick
        wide.
      </p>
      <p>That tick is then settled by the chain itself, in one transaction:</p>
      <ol>
        <li>the submitted pre-state must hash to the agreed lower claim,</li>
        <li>the submitted inputs must extend the lower input chain to the asserted upper chain,</li>
        <li>a scratch account is seeded with the pre-state via CPI into the game program,</li>
        <li>
          the game program executes the tick <strong>natively</strong> - the
          same SBF the operator ran off-chain,
        </li>
        <li>the resulting state root either matches the asserted claim or it does not. Winner takes both bonds.</li>
      </ol>
      <p>
        Anyone may submit the replay; the outcome is decided by execution,
        not by who called it. One deliberate asymmetry: if the disputed tick
        cannot execute at all (the operator committed to inputs the game
        program rejects), the replay transaction can never land and the
        challenger wins by timeout - the burden of proof sits with the
        asserter. The complete one-step proof - both hash checks, two CPIs,
        payout - lands at <strong>~19k CU</strong>.
      </p>

      <h2>5. The wager layer: escrow without a reporter</h2>
      <p>
        <code>programs/wager</code> turns the referee into money. A match
        account pins both players, the game program, the stake, the final
        tick, a settlement deadline, and the genesis claim. Player A escrows
        a stake to open; player B matches it to join. Settlement has two
        paths:
      </p>
      <ul>
        <li>
          <strong>Cooperative.</strong> Both players sign the result byte;
          the pot pays out instantly. This is the expected path for nearly
          every match - the trustless machinery below is what makes refusing
          to sign pointless.
        </li>
        <li>
          <strong>Proven.</strong> Either player runs a referee session for
          the match, asserts the final checkpoint, and survives the
          challenge window (or wins the dispute). The wager program then
          checks the session is finalized at exactly the match&apos;s final
          tick, checks the submitted final state hashes to the proven claim,
          and asks <em>the game program itself</em> who won: a{" "}
          <code>Verdict</code> CPI over the proven state returns
          draw/first/second as return data. The escrow knows nothing about
          any game&apos;s state layout - any tick game that exposes
          LoadState/Verdict settles through the same escrow unchanged.
        </li>
      </ul>
      <h3>Per-player session slots</h3>
      <p>
        Each player binds their <em>own</em> referee session to the match,
        and can only ever rebind their own slot. This closes a real griefing
        lane: with a single shared slot, a cheater could rebind a fresh
        virgin session right before the honest player&apos;s settlement
        transaction lands, invalidating it forever and converting a certain
        loss into a deadline refund. With per-player slots a player can only
        sabotage their own path to settlement; the opponent&apos;s proof
        stands.
      </p>
      <h3>Punishment and payout are separate concerns</h3>
      <p>
        A subtle property worth making explicit: when a cheater&apos;s
        assertion is destroyed in a dispute, they lose their{" "}
        <em>bond</em> to the challenger - but the <em>pot</em> still goes to
        whoever actually won the game, as named by the verdict over the
        true final state. Lying about a match you genuinely won costs you
        the bond and nothing else; lying about a match you lost costs you
        the bond and the match. Incentives stay aligned in both directions.
      </p>
      <h3>Liveness</h3>
      <p>
        Funds cannot deadlock. An unjoined match is cancellable by its
        creator; a live match that nobody manages to settle refunds both
        sides after the deadline. The optimistic assumption is the standard
        one: each player (or anyone watching on their behalf) comes online
        at least once per challenge window.
      </p>

      <h2>6. Measured results</h2>
      <p>
        Prototype windows are deliberately short (64-slot challenge window,
        150-slot move deadline) to make devnet runs watchable; mainnet
        values would be hours. Everything below is from confirmed devnet
        transactions or the real agave runtime:
      </p>
      <table>
        <thead>
          <tr>
            <th>Quantity</th>
            <th>Measured</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>one arena tick (real runtime)</td>
            <td>≤ ~2,000 CU</td>
          </tr>
          <tr>
            <td>complete one-step fraud proof</td>
            <td>~19k CU (~1.4% of a tx budget)</td>
          </tr>
          <tr>
            <td>trustless settle instruction</td>
            <td>~13k CU</td>
          </tr>
          <tr>
            <td>interpreted re-execution (comparison)</td>
            <td>80+ CU per emulated instruction; ~157k CU per tick before memory proofs</td>
          </tr>
          <tr>
            <td>SP1 Groth16 verify (comparison)</td>
            <td>~280k CU</td>
          </tr>
          <tr>
            <td>honest match, end to end on devnet</td>
            <td>6 tx / ~31 s / 65k lamports in fees</td>
          </tr>
          <tr>
            <td>adversarial match on devnet</td>
            <td>20 tx / ~47 s; lie injected at tick 21 cornered to exactly ticks 20→21</td>
          </tr>
          <tr>
            <td>state root cost</td>
            <td>~11 CU/byte, linear</td>
          </tr>
          <tr>
            <td>engine throughput</td>
            <td>~17k ticks/s per core through the full runtime pipeline</td>
          </tr>
        </tbody>
      </table>
      <p>
        The adversarial run is worth reading on an explorer: the{" "}
        <a href={EXPLORER(REPLAY_TX)} target="_blank" rel="noopener noreferrer">
          native replay transaction
        </a>{" "}
        is the cluster re-executing the disputed tick and convicting the
        cheater, and the{" "}
        <a href={EXPLORER(SETTLE_TX)} target="_blank" rel="noopener noreferrer">
          settlement transaction
        </a>{" "}
        is the escrow paying the true winner through the game&apos;s own
        verdict. Both times the cluster&apos;s replay matched the locally
        computed trace bit for bit.
      </p>

      <h2>7. Security model and known gaps</h2>
      <p>This is an early prototype. The honest list:</p>
      <ul>
        <li>
          <strong>Input authenticity.</strong> The input chain pins{" "}
          <em>which</em> inputs were committed, not <em>who</em> produced
          them. A session operator could attribute invented inputs to the
          opponent. The fix - player-signed input entries verified inside the
          tick function, which the engine already makes cheap - is the next
          layer. Today the cooperative path plus the opponent&apos;s ability
          to run their own session bounds the damage.
        </li>
        <li>
          <strong>Single challenger per session.</strong> The referee
          resolves one dispute per assertion; a resolved session is dead.
          The per-player slot design absorbs this for wagers, but a
          production referee wants multi-challenger sessions.
        </li>
        <li>
          <strong>Optimistic liveness.</strong> A player who never watches
          the chain can be cheated by an unchallenged false finalization -
          inherent to optimistic systems; watchtowers are the standard
          mitigation.
        </li>
        <li>
          <strong>Prototype parameters.</strong> Devnet windows are minutes,
          not hours, and the cooperative-settle handoff in the web console
          rides a ~60 s blockhash window. Durable nonces remove that limit.
        </li>
      </ul>

      <h2>8. Why this matters beyond games</h2>
      <p>
        The deeper claim is about the substrate: any deterministic
        computation expressible as an SBF program with byte-array state can
        be run off-chain at native speed and held accountable by the chain
        at ~19k CU per dispute, with zero proving overhead in the happy
        path. Real-time games are the most demanding instance - tick rates,
        adversarial counterparties, money on the line - which is exactly why
        they make a good proof. Order matching, simulations, multi-step
        agent workflows: anything with a pure state transition fits the same
        mold.
      </p>
      <p>
        The chain is not a computer you run things on. It is a court you
        never have to visit - but whose verdicts are mechanical.
      </p>

      <h2>Appendix: deployed artifacts</h2>
      <ul>
        <li>
          wager:{" "}
          <a
            href={EXPLORER_ADDR("Cs1z5RKFzUNughgamPk3yA7jJhbMMXHNb1YXp2Mbuv4d")}
            target="_blank"
            rel="noopener noreferrer"
          >
            <code>Cs1z5RKFzUNughgamPk3yA7jJhbMMXHNb1YXp2Mbuv4d</code>
          </a>
        </li>
        <li>
          referee:{" "}
          <a
            href={EXPLORER_ADDR("Fq4ThqS2tFAWbcSce5pKqcEBB9k4XJxbsq6Mzpjh3yJ7")}
            target="_blank"
            rel="noopener noreferrer"
          >
            <code>Fq4ThqS2tFAWbcSce5pKqcEBB9k4XJxbsq6Mzpjh3yJ7</code>
          </a>
        </li>
        <li>
          arena:{" "}
          <a
            href={EXPLORER_ADDR("DcMdSfBtccFMATGfaPWzx6hSEZpsfH4oy2V2t291eHyd")}
            target="_blank"
            rel="noopener noreferrer"
          >
            <code>DcMdSfBtccFMATGfaPWzx6hSEZpsfH4oy2V2t291eHyd</code>
          </a>
        </li>
      </ul>
      <p>
        Reproduce the honest run with{" "}
        <code>cargo run -p devnet-match --release</code> and the adversarial
        run with <code>--cheat</code>.
      </p>
    </article>
  );
}
