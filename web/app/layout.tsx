import type { Metadata } from "next";
import { Fraunces, Inter, JetBrains_Mono } from "next/font/google";
import Link from "next/link";
import "./globals.css";

const fraunces = Fraunces({
  subsets: ["latin"],
  variable: "--font-fraunces",
  display: "swap",
});
const inter = Inter({
  subsets: ["latin"],
  variable: "--font-inter",
  display: "swap",
});
const jetbrains = JetBrains_Mono({
  subsets: ["latin"],
  variable: "--font-jetbrains",
  display: "swap",
});

export const metadata: Metadata = {
  title: "tickproof - trustless skill wagering on Solana",
  description:
    "A verifiable game engine for Solana. Game logic compiles to SBF, runs off-chain at full speed, and any disputed tick is replayed natively by the chain itself. Real stakes, no trusted reporter.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body
        className={`${fraunces.variable} ${inter.variable} ${jetbrains.variable} min-h-screen flex flex-col`}
      >
        <header className="sticky top-0 z-40 border-b border-cream-300 bg-cream-100/90 backdrop-blur">
          <nav className="mx-auto flex h-14 max-w-5xl items-center justify-between px-5">
            <Link
              href="/"
              className="font-[family-name:var(--font-display)] text-lg font-semibold tracking-tight"
            >
              tickproof
              <span className="ml-2 rounded-full bg-accent-600 px-2 py-0.5 text-[10px] font-sans font-semibold uppercase tracking-wider text-cream-50 align-middle">
                devnet
              </span>
            </Link>
            <div className="flex items-center gap-6 text-sm font-medium text-ink-700">
              <Link href="/paper" className="hover:text-accent-600 transition-colors">
                Technical paper
              </Link>
              <Link href="/console" className="hover:text-accent-600 transition-colors">
                Console
              </Link>
              <a
                href="https://github.com/nzengi/tickproof"
                target="_blank"
                rel="noopener noreferrer"
                className="hover:text-accent-600 transition-colors"
              >
                GitHub
              </a>
            </div>
          </nav>
        </header>
        <main className="flex-1">{children}</main>
        <footer className="border-t border-cream-300 py-8">
          <div className="mx-auto flex max-w-5xl flex-col gap-2 px-5 text-xs text-ink-400 sm:flex-row sm:items-center sm:justify-between">
            <p>
              tickproof - verifiable game engine for Solana. Early WIP, devnet
              only. Apache-2.0.
            </p>
            <p className="font-[family-name:var(--font-mono)]">
              the chain is the referee
            </p>
          </div>
        </footer>
      </body>
    </html>
  );
}
