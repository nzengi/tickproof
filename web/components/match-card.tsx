"use client";

import { useState } from "react";
import { Buffer } from "buffer";
import { Transaction } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { EXPLORER_ADDR } from "@/lib/constants";
import {
  MatchAccount,
  PHASE,
  PHASE_NAME,
  SIDE_NAME,
  cancelIx,
  coopSettleIx,
  expireIx,
  joinIxs,
  lamportsToSol,
  short,
} from "@/lib/wager";
import { PublicKey } from "@solana/web3.js";

const ZERO = PublicKey.default.toBase58();

function Badge({ phase, winner }: { phase: number; winner: number }) {
  const styles = [
    "bg-sage-600/15 text-sage-600",
    "bg-accent-600/15 text-accent-700",
    "bg-ink-500/10 text-ink-500",
  ];
  return (
    <span
      className={`rounded-full px-2.5 py-0.5 text-xs font-semibold ${styles[phase] ?? ""}`}
    >
      {PHASE_NAME[phase] ?? "?"}
      {phase === PHASE.SETTLED && ` - ${SIDE_NAME[winner] ?? "?"}`}
    </span>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4 text-sm">
      <span className="text-ink-400">{label}</span>
      <span className="font-[family-name:var(--font-mono)] text-xs text-ink-700">
        {children}
      </span>
    </div>
  );
}

export function MatchCard({
  m,
  slot,
  onChanged,
}: {
  m: MatchAccount;
  slot: number;
  onChanged: () => void;
}) {
  const { connection } = useConnection();
  const wallet = useWallet();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [winner, setWinner] = useState(1);
  const [shareTx, setShareTx] = useState("");
  const [pasteTx, setPasteTx] = useState("");

  const me = wallet.publicKey?.toBase58();
  const isA = me === m.playerA.toBase58();
  const isB = me === m.playerB.toBase58();
  const expired = m.phase === PHASE.LIVE && BigInt(slot) > m.deadline;

  async function run(label: string, fn: () => Promise<void>) {
    setBusy(label);
    setError(null);
    try {
      await fn();
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function send(ixs: Parameters<Transaction["add"]>) {
    const tx = new Transaction().add(...ixs);
    const sig = await wallet.sendTransaction(tx, connection);
    await connection.confirmTransaction(sig, "confirmed");
  }

  const join = () => run("join", () => send(joinIxs(m) as never));
  const cancel = () => run("cancel", () => send([cancelIx(m)] as never));
  const expire = () => run("expire", () => send([expireIx(m)] as never));

  // cooperative settle: player A signs first and shares the partially
  // signed transaction; player B co-signs and submits. The blockhash
  // gives the handoff roughly a minute.
  const signAndCopy = () =>
    run("sign", async () => {
      if (!wallet.signTransaction || !wallet.publicKey) throw new Error("wallet can't partial-sign");
      const tx = new Transaction().add(coopSettleIx(m, winner));
      tx.feePayer = m.playerA;
      tx.recentBlockhash = (
        await connection.getLatestBlockhash("confirmed")
      ).blockhash;
      const signed = await wallet.signTransaction(tx);
      const b64 = signed
        .serialize({ requireAllSignatures: false, verifySignatures: false })
        .toString("base64");
      setShareTx(b64);
      await navigator.clipboard.writeText(b64).catch(() => {});
    });

  const cosignAndSend = () =>
    run("cosign", async () => {
      if (!wallet.signTransaction) throw new Error("wallet can't partial-sign");
      const tx = Transaction.from(Buffer.from(pasteTx.trim(), "base64"));
      const signed = await wallet.signTransaction(tx);
      const sig = await connection.sendRawTransaction(signed.serialize());
      await connection.confirmTransaction(sig, "confirmed");
      setPasteTx("");
    });

  return (
    <div className="rounded-2xl border border-cream-300 bg-cream-50 p-5 shadow-sm">
      <div className="mb-3 flex items-center justify-between">
        <a
          href={EXPLORER_ADDR(m.pubkey.toBase58())}
          target="_blank"
          rel="noopener noreferrer"
          className="font-[family-name:var(--font-mono)] text-sm font-semibold text-accent-600 hover:text-accent-700"
        >
          {short(m.pubkey)}
        </a>
        <Badge phase={m.phase} winner={m.winner} />
      </div>

      <div className="space-y-1.5">
        <Row label="stake / side">{lamportsToSol(m.stake)} SOL</Row>
        <Row label="player A">
          {short(m.playerA)}
          {isA && <span className="ml-1 text-accent-600">(you)</span>}
        </Row>
        <Row label="player B">
          {short(m.playerB)}
          {isB && <span className="ml-1 text-accent-600">(you)</span>}
        </Row>
        <Row label="final tick">{m.finalTick.toString()}</Row>
        {m.phase === PHASE.LIVE && (
          <Row label="deadline slot">
            {m.deadline.toString()}
            {expired && <span className="ml-1 text-claret-600">(expired)</span>}
          </Row>
        )}
        {m.sessionA.toBase58() !== ZERO && (
          <Row label="session A">{short(m.sessionA)}</Row>
        )}
        {m.sessionB.toBase58() !== ZERO && (
          <Row label="session B">{short(m.sessionB)}</Row>
        )}
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        {m.phase === PHASE.OPEN && isB && (
          <Action onClick={join} busy={busy === "join"}>
            join with {lamportsToSol(m.stake)} SOL
          </Action>
        )}
        {m.phase === PHASE.OPEN && isA && (
          <Action onClick={cancel} busy={busy === "cancel"} subtle>
            cancel &amp; refund
          </Action>
        )}
        {expired && (
          <Action onClick={expire} busy={busy === "expire"} subtle>
            expire &amp; refund both
          </Action>
        )}
      </div>

      {m.phase === PHASE.LIVE && (isA || isB) && (
        <div className="mt-4 rounded-xl bg-cream-200/70 p-3">
          <p className="mb-2 text-xs font-semibold uppercase tracking-wider text-ink-500">
            cooperative settle (both signatures)
          </p>
          {isA && (
            <div className="flex flex-wrap items-center gap-2">
              <select
                value={winner}
                onChange={(e) => setWinner(Number(e.target.value))}
                className="rounded-lg border border-cream-400 bg-cream-50 px-2 py-1.5 text-sm"
              >
                <option value={1}>player A wins</option>
                <option value={2}>player B wins</option>
                <option value={0}>draw</option>
              </select>
              <Action onClick={signAndCopy} busy={busy === "sign"}>
                sign &amp; copy for player B
              </Action>
            </div>
          )}
          {isA && shareTx && (
            <p className="mt-2 break-all font-[family-name:var(--font-mono)] text-[10px] leading-relaxed text-ink-500">
              copied - send to player B within ~1 min: {shareTx.slice(0, 56)}…
            </p>
          )}
          {isB && (
            <div className="mt-1 flex flex-col gap-2">
              <textarea
                value={pasteTx}
                onChange={(e) => setPasteTx(e.target.value)}
                placeholder="paste player A's signed transaction (base64)"
                rows={2}
                className="w-full rounded-lg border border-cream-400 bg-cream-50 p-2 font-[family-name:var(--font-mono)] text-[11px]"
              />
              <Action
                onClick={cosignAndSend}
                busy={busy === "cosign"}
                disabled={!pasteTx.trim()}
              >
                co-sign &amp; submit
              </Action>
            </div>
          )}
        </div>
      )}

      {m.phase === PHASE.LIVE && (
        <p className="mt-3 text-xs leading-relaxed text-ink-400">
          Contested settlement (referee proof + native replay) runs through
          the engine: <code className="font-[family-name:var(--font-mono)]">cargo run -p devnet-match</code>
        </p>
      )}

      {error && (
        <p className="mt-3 break-all text-xs text-claret-600">{error}</p>
      )}
    </div>
  );
}

function Action({
  onClick,
  busy,
  disabled,
  subtle,
  children,
}: {
  onClick: () => void;
  busy?: boolean;
  disabled?: boolean;
  subtle?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      disabled={busy || disabled}
      className={
        subtle
          ? "rounded-full border border-ink-500/30 px-4 py-1.5 text-sm font-medium text-ink-700 transition hover:border-ink-700 disabled:opacity-50"
          : "rounded-full bg-ink-900 px-4 py-1.5 text-sm font-semibold text-cream-50 transition hover:bg-ink-700 disabled:opacity-50"
      }
    >
      {busy ? "confirming..." : children}
    </button>
  );
}
