"use client";

import { useCallback, useEffect, useState } from "react";
import { Keypair, PublicKey, Transaction } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { Providers } from "@/components/providers";
import { WalletButton } from "@/components/wallet-button";
import { MatchCard } from "@/components/match-card";
import {
  EXPLORER_ADDR,
  MATCH_LEN,
  WAGER_PROGRAM_ID,
} from "@/lib/constants";
import {
  MatchAccount,
  createMatchIxs,
  decodeMatch,
} from "@/lib/wager";

function CreateMatch({ onCreated }: { onCreated: () => void }) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const [opponent, setOpponent] = useState("");
  const [stakeSol, setStakeSol] = useState("0.01");
  const [ticks, setTicks] = useState("32");
  const [deadlineSlots, setDeadlineSlots] = useState("5000");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<string | null>(null);

  async function create() {
    setBusy(true);
    setError(null);
    setCreated(null);
    try {
      if (!wallet.publicKey) throw new Error("connect a wallet first");
      const playerB = new PublicKey(opponent.trim());
      const stake = BigInt(Math.round(parseFloat(stakeSol) * 1e9));
      if (stake <= 0n) throw new Error("stake must be positive");
      const matchKp = Keypair.generate();
      const rent =
        await connection.getMinimumBalanceForRentExemption(MATCH_LEN);
      const tx = new Transaction().add(
        ...createMatchIxs({
          payer: wallet.publicKey,
          matchPubkey: matchKp.publicKey,
          playerA: wallet.publicKey,
          playerB,
          stake,
          finalTick: BigInt(ticks),
          deadlineSlots: BigInt(deadlineSlots),
          rentLamports: rent,
        }),
      );
      const sig = await wallet.sendTransaction(tx, connection, {
        signers: [matchKp],
      });
      await connection.confirmTransaction(sig, "confirmed");
      setCreated(matchKp.publicKey.toBase58());
      onCreated();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const input =
    "w-full rounded-lg border border-cream-400 bg-cream-50 px-3 py-2 text-sm focus:border-accent-600 focus:outline-none";

  return (
    <div className="rounded-2xl border border-cream-300 bg-cream-50 p-6 shadow-sm">
      <h2 className="mb-1 font-[family-name:var(--font-display)] text-xl font-semibold">
        Open a match
      </h2>
      <p className="mb-4 text-sm text-ink-500">
        Escrows your stake on devnet. Your opponent joins with theirs; the
        genesis claim is computed in your browser, byte-identical to the
        engine&apos;s.
      </p>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="sm:col-span-2 text-sm">
          <span className="mb-1 block font-medium text-ink-700">
            Opponent (player B)
          </span>
          <input
            className={input}
            value={opponent}
            onChange={(e) => setOpponent(e.target.value)}
            placeholder="opponent wallet address"
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block font-medium text-ink-700">
            Stake per side (SOL)
          </span>
          <input
            className={input}
            value={stakeSol}
            onChange={(e) => setStakeSol(e.target.value)}
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block font-medium text-ink-700">
            Match length (ticks)
          </span>
          <input
            className={input}
            value={ticks}
            onChange={(e) => setTicks(e.target.value)}
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block font-medium text-ink-700">
            Settlement deadline (slots after join)
          </span>
          <input
            className={input}
            value={deadlineSlots}
            onChange={(e) => setDeadlineSlots(e.target.value)}
          />
        </label>
        <div className="flex items-end">
          <button
            onClick={create}
            disabled={busy || !wallet.publicKey}
            className="rounded-full bg-accent-600 px-6 py-2 text-sm font-semibold text-cream-50 transition hover:bg-accent-700 disabled:opacity-50"
          >
            {busy ? "confirming..." : "create match"}
          </button>
        </div>
      </div>
      {created && (
        <p className="mt-3 text-sm text-sage-600">
          Match created:{" "}
          <a
            className="underline"
            href={EXPLORER_ADDR(created)}
            target="_blank"
            rel="noopener noreferrer"
          >
            {created}
          </a>
        </p>
      )}
      {error && <p className="mt-3 break-all text-sm text-claret-600">{error}</p>}
    </div>
  );
}

function Console() {
  const { connection } = useConnection();
  const wallet = useWallet();
  const [matches, setMatches] = useState<MatchAccount[]>([]);
  const [slot, setSlot] = useState(0);
  const [loading, setLoading] = useState(true);
  const [onlyMine, setOnlyMine] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [accounts, currentSlot] = await Promise.all([
        connection.getProgramAccounts(WAGER_PROGRAM_ID, {
          filters: [{ dataSize: MATCH_LEN }],
        }),
        connection.getSlot("confirmed"),
      ]);
      const decoded = accounts
        .map(({ pubkey, account }) =>
          decodeMatch(pubkey, account.lamports, account.data),
        )
        .filter((m): m is MatchAccount => m !== null)
        .sort((a, b) => a.phase - b.phase);
      setMatches(decoded);
      setSlot(currentSlot);
    } finally {
      setLoading(false);
    }
  }, [connection]);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 30_000);
    return () => clearInterval(t);
  }, [refresh]);

  const me = wallet.publicKey?.toBase58();
  const visible = onlyMine
    ? matches.filter(
        (m) => m.playerA.toBase58() === me || m.playerB.toBase58() === me,
      )
    : matches;

  return (
    <div className="mx-auto max-w-5xl px-5 py-10">
      <div className="mb-8 flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="font-[family-name:var(--font-display)] text-3xl font-semibold tracking-tight">
            Escrow console
          </h1>
          <p className="mt-1 text-sm text-ink-500">
            Live against the wager program on devnet -{" "}
            <a
              className="text-accent-600 underline"
              href={EXPLORER_ADDR(WAGER_PROGRAM_ID.toBase58())}
              target="_blank"
              rel="noopener noreferrer"
            >
              {WAGER_PROGRAM_ID.toBase58().slice(0, 8)}…
            </a>
          </p>
        </div>
        <WalletButton />
      </div>

      <CreateMatch onCreated={refresh} />

      <div className="mt-10 mb-4 flex items-center justify-between">
        <h2 className="font-[family-name:var(--font-display)] text-xl font-semibold">
          Matches
          <span className="ml-2 text-sm font-sans font-normal text-ink-400">
            {loading ? "loading..." : `${visible.length} on-chain`}
          </span>
        </h2>
        <div className="flex items-center gap-3 text-sm">
          <label className="flex cursor-pointer items-center gap-1.5 text-ink-500">
            <input
              type="checkbox"
              checked={onlyMine}
              onChange={(e) => setOnlyMine(e.target.checked)}
              className="accent-accent-600"
            />
            only mine
          </label>
          <button
            onClick={refresh}
            className="rounded-full border border-ink-500/30 px-3 py-1 text-ink-700 transition hover:border-ink-700"
          >
            refresh
          </button>
        </div>
      </div>

      {visible.length === 0 && !loading ? (
        <p className="rounded-2xl border border-dashed border-cream-400 p-10 text-center text-sm text-ink-400">
          No match accounts yet - open one above.
        </p>
      ) : (
        <div className="grid gap-4 md:grid-cols-2">
          {visible.map((m) => (
            <MatchCard
              key={m.pubkey.toBase58()}
              m={m}
              slot={slot}
              onChanged={refresh}
            />
          ))}
        </div>
      )}

      <p className="mt-10 text-xs leading-relaxed text-ink-400">
        The console drives the escrow lifecycle: create, join, cancel,
        cooperative settle, deadline refund. Playing the match itself and the
        trustless settlement path (referee checkpoint, challenge window,
        bisection, native replay) run through the off-chain engine - see{" "}
        <code className="font-[family-name:var(--font-mono)]">tools/devnet-match</code>{" "}
        in the repository, or read the technical paper.
      </p>
    </div>
  );
}

export default function ConsolePage() {
  return (
    <Providers>
      <Console />
    </Providers>
  );
}
