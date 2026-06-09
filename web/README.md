# tickproof web

Landing page, technical paper and devnet escrow console for tickproof.
Next.js (App Router) + Tailwind v4, cream theme. The console talks to the
deployed wager/referee/arena programs on Solana devnet through any
wallet-standard wallet (Phantom, Solflare, Backpack...).

## Develop

```
npm install
npm run dev
```

`npm run check:merkle` pins the TypeScript merkle implementation against
the Rust golden vector - run it after touching `lib/merkle.ts` or the
Rust commitment scheme.

## Deploy on Vercel

1. Push the repository to GitHub.
2. Vercel → New Project → import the repo.
3. Set **Root Directory** to `web`. Framework preset: Next.js. Everything
   else stays default.
4. (Optional) set `NEXT_PUBLIC_RPC_URL` to a private devnet RPC endpoint -
   the public `api.devnet.solana.com` works but is rate-limited.

## Notes

- The console covers the escrow lifecycle: create, join, cancel,
  cooperative settle (partially-signed transaction handoff between the
  two players), deadline refund.
- Playing a match and the contested settlement path (referee checkpoint,
  challenge window, bisection, native replay) run through the off-chain
  engine: `cargo run -p devnet-match --release` in the repository root.
- Genesis claims are computed in the browser and must stay byte-identical
  to `crates/merkle`; that is what `check:merkle` guards.
