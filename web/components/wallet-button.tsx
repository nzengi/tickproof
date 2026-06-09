"use client";

import dynamic from "next/dynamic";

// the wallet button reads wallet state on mount; keep it client-only
export const WalletButton = dynamic(
  () =>
    import("@solana/wallet-adapter-react-ui").then(
      (m) => m.WalletMultiButton,
    ),
  { ssr: false },
);
