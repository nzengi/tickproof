"use client";

import { useMemo } from "react";
import { Buffer } from "buffer";
import {
  ConnectionProvider,
  WalletProvider,
} from "@solana/wallet-adapter-react";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";
import { RPC_URL } from "@/lib/constants";
import "@solana/wallet-adapter-react-ui/styles.css";

// web3.js still reaches for Buffer in a few code paths
if (typeof window !== "undefined" && !(window as any).Buffer) {
  (window as any).Buffer = Buffer;
}

export function Providers({ children }: { children: React.ReactNode }) {
  // empty list: wallet-standard wallets (Phantom, Solflare, Backpack...)
  // register themselves
  const wallets = useMemo(() => [], []);
  return (
    <ConnectionProvider endpoint={RPC_URL}>
      <WalletProvider wallets={wallets} autoConnect>
        <WalletModalProvider>{children}</WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}
