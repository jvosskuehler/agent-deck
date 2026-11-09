// Baut die Hilfsprogramme, die neben agent-deck.exe liegen müssen, und legt sie für
// das Installationspaket bereit (CLAUDE.md, „Bauen“):
//
//   deck-console     Tasten in fremde Konsolen (crates/deck-win)
//   deck-report      Hook „Zustand melden“ (crates/deck-hooks)
//   deck-statusline  statusLine von Claude Code (crates/deck-hooks)
//   deck-watchdog    Wächter, startet die App neu, wenn sie verschwindet (crates/deck-services)
//
// Sie sind Teil desselben Cargo-Workspace und landen darum im selben Zielordner wie die
// App (target/<profil>/), also direkt neben agent-deck.exe. Zusätzlich kommt je eine
// Kopie mit Zielsystem-Anhängsel nach src-tauri/binaries/, wo `tauri build --config
// src-tauri/tauri.bundle.conf.json` sie als externalBin ins Paket nimmt.
//
// Aufruf: node scripts/helpers.mjs [debug|release]

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HELPERS = ["deck-console", "deck-report", "deck-statusline", "deck-watchdog"];
const PACKAGES = ["deck-win", "deck-hooks", "deck-services"];
const TRIPLE = "x86_64-pc-windows-msvc";

const profile = process.argv[2] === "release" ? "release" : "debug";
const app = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(app, "..");

// cargo steht oft nicht im PATH (rustup ohne PATH-Eintrag): dann aus ~/.cargo/bin.
const fallback = join(homedir(), ".cargo", "bin", "cargo.exe");
const cargo = existsSync(fallback) ? fallback : "cargo";

const args = ["build", ...(profile === "release" ? ["--release"] : []), "--bins"];
for (const p of PACKAGES) args.push("-p", p);
execFileSync(cargo, args, { cwd: root, stdio: "inherit" });

const target = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, "target");
const outDir = join(target, profile);
const binaries = join(app, "src-tauri", "binaries");
mkdirSync(binaries, { recursive: true });
for (const name of HELPERS) {
  const exe = join(outDir, `${name}.exe`);
  if (!existsSync(exe)) throw new Error(`${exe} fehlt nach dem Build`);
  copyFileSync(exe, join(binaries, `${name}-${TRIPLE}.exe`));
}
console.log(`Hilfsprogramme (${profile}) liegen in ${outDir}`);
