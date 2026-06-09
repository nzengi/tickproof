import Link from "next/link";
import { EXPLORER_ADDR } from "@/lib/constants";

const numbers = [
  { value: "~2,000 CU", label: "one arena tick under the real agave runtime" },
  { value: "~19k CU", label: "complete one-step fraud proof, ~1.4% of a tx budget" },
  { value: "~13k CU", label: "trustless settle instruction (root check + verdict CPI + payout)" },
  { value: "~17k ticks/s", label: "engine throughput through the full runtime pipeline" },
  { value: "6 tx / 31 s", label: "honest match settled on devnet, 65k lamports in fees" },
  { value: "20 tx / 47 s", label: "full adversarial settlement: cheater bisected, convicted, paid out" },
];

const programs = [
  ["wager", "Cs1z5RKFzUNughgamPk3yA7jJhbMMXHNb1YXp2Mbuv4d", "stake escrow, verdict CPI, payouts"],
  ["referee", "Fq4ThqS2tFAWbcSce5pKqcEBB9k4XJxbsq6Mzpjh3yJ7", "checkpoints, bisection, native replay"],
  ["arena", "DcMdSfBtccFMATGfaPWzx6hSEZpsfH4oy2V2t291eHyd", "the game: tick, load-state, verdict"],
];

export default function Home() {
  return (
    <div className="mx-auto max-w-5xl px-5">
      {/* hero */}
      <section className="py-20 sm:py-28">
        <p className="mb-4 font-[family-name:var(--font-mono)] text-xs uppercase tracking-[0.2em] text-accent-600">
          verifiable game engine - live on solana devnet
        </p>
        <h1 className="max-w-3xl font-[family-name:var(--font-display)] text-4xl font-semibold leading-tight tracking-tight sm:text-6xl">
          Real-money matches where{" "}
          <em className="text-accent-600">the chain is the referee.</em>
        </h1>
        <p className="mt-6 max-w-2xl text-lg leading-relaxed text-ink-700">
          Every PvP wagering platform on Solana today trusts a server to
          report who won. tickproof removes the reporter entirely: game logic
          compiles to SBF, runs off-chain at full speed, and any disputed
          tick is replayed by the L1 itself - the chain already executes SBF
          natively, so the final step of a dispute is just a program
          invocation. No oracle, no zkVM, no interpreter-in-a-contract.
        </p>
        <div className="mt-8 flex flex-wrap gap-3">
          <Link
            href="/console"
            className="rounded-full bg-ink-900 px-6 py-2.5 text-sm font-semibold text-cream-50 transition hover:bg-ink-700"
          >
            Open the console
          </Link>
          <Link
            href="/paper"
            className="rounded-full border border-ink-500/30 px-6 py-2.5 text-sm font-semibold text-ink-700 transition hover:border-ink-700"
          >
            Read the technical paper
          </Link>
        </div>
      </section>

      {/* how it works */}
      <section className="border-t border-cream-300 py-16">
        <h2 className="mb-10 font-[family-name:var(--font-display)] text-2xl font-semibold">
          How a match settles without trust
        </h2>
        <ol className="grid gap-8 md:grid-cols-2 lg:grid-cols-4">
          {[
            {
              t: "Escrow",
              d: "Both players lock their stake in the wager program. The match account pins the game program, the genesis state commitment and the final tick.",
            },
            {
              t: "Play off-chain",
              d: "The match runs at full speed through the real agave runtime in an embedded VM. State roots and an input chain are committed at checkpoints.",
            },
            {
              t: "Prove or co-sign",
              d: "If both players agree, two signatures settle instantly. If not, either player asserts the final checkpoint with a bond and it survives a challenge window.",
            },
            {
              t: "The chain decides",
              d: "A lying assertion gets bisected to the exact divergent tick, which the cluster replays natively. The game program itself names the winner over the proven state.",
            },
          ].map((s, i) => (
            <li key={s.t}>
              <span className="font-[family-name:var(--font-mono)] text-xs text-accent-600">
                0{i + 1}
              </span>
              <h3 className="mt-1 mb-2 font-[family-name:var(--font-display)] text-lg font-semibold">
                {s.t}
              </h3>
              <p className="text-sm leading-relaxed text-ink-700">{s.d}</p>
            </li>
          ))}
        </ol>
      </section>

      {/* numbers */}
      <section className="border-t border-cream-300 py-16">
        <h2 className="mb-2 font-[family-name:var(--font-display)] text-2xl font-semibold">
          Measured, not promised
        </h2>
        <p className="mb-10 max-w-2xl text-sm text-ink-500">
          Every figure below comes from the real agave runtime or from
          confirmed devnet transactions - the repository reproduces all of
          them.
        </p>
        <div className="grid gap-px overflow-hidden rounded-2xl border border-cream-300 bg-cream-300 sm:grid-cols-2 lg:grid-cols-3">
          {numbers.map((n) => (
            <div key={n.label} className="bg-cream-50 p-6">
              <p className="font-[family-name:var(--font-display)] text-2xl font-semibold text-accent-600">
                {n.value}
              </p>
              <p className="mt-1 text-sm leading-relaxed text-ink-700">
                {n.label}
              </p>
            </div>
          ))}
        </div>
      </section>

      {/* programs */}
      <section className="border-t border-cream-300 py-16">
        <h2 className="mb-10 font-[family-name:var(--font-display)] text-2xl font-semibold">
          Deployed programs
        </h2>
        <div className="space-y-3">
          {programs.map(([name, id, desc]) => (
            <a
              key={id}
              href={EXPLORER_ADDR(id)}
              target="_blank"
              rel="noopener noreferrer"
              className="flex flex-col gap-1 rounded-xl border border-cream-300 bg-cream-50 p-4 transition hover:border-accent-600 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="flex items-center gap-3">
                <span className="font-[family-name:var(--font-display)] font-semibold">
                  {name}
                </span>
                <span className="text-sm text-ink-500">{desc}</span>
              </div>
              <span className="font-[family-name:var(--font-mono)] text-xs text-ink-400">
                {id}
              </span>
            </a>
          ))}
        </div>
      </section>
    </div>
  );
}
