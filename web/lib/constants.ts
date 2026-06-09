import { PublicKey } from "@solana/web3.js";

// devnet deployments, owned by the tickproof workspace keypairs
export const WAGER_PROGRAM_ID = new PublicKey(
  "Cs1z5RKFzUNughgamPk3yA7jJhbMMXHNb1YXp2Mbuv4d",
);
export const REFEREE_PROGRAM_ID = new PublicKey(
  "Fq4ThqS2tFAWbcSce5pKqcEBB9k4XJxbsq6Mzpjh3yJ7",
);
export const ARENA_PROGRAM_ID = new PublicKey(
  "DcMdSfBtccFMATGfaPWzx6hSEZpsfH4oy2V2t291eHyd",
);

export const RPC_URL =
  process.env.NEXT_PUBLIC_RPC_URL ?? "https://api.devnet.solana.com";

export const MATCH_LEN = 296;
export const STATE_SIZE = 8 + 8 * 32; // arena: tick counter + 8 balls

export const EXPLORER = (sig: string) =>
  `https://explorer.solana.com/tx/${sig}?cluster=devnet`;
export const EXPLORER_ADDR = (addr: string) =>
  `https://explorer.solana.com/address/${addr}?cluster=devnet`;
