# Agent Deck

Ein am Bildschirmrand angedocktes Dashboard für parallel laufende **Claude-Code-Agenten**
in Windows Terminal und VS Code. Eine Karte je Agent, die Farbe ist der Zustand: wer
denkt, wer eine Rückfrage hat, wer fertig ist. Klick holt genau diesen Agenten nach vorn.

Windows-only. Rust (Tauri 2) und React/TypeScript.

## Bauen und starten

```powershell
cd app
pnpm install
pnpm tauri dev          # Entwicklungsmodus
pnpm dev                # nur die Oberfläche im Browser, mit Beispieldaten
cargo test --workspace  # alle Rust-Tests (aus der Repo-Wurzel)
```

Neben dem laufenden alten Deck ausprobieren: `DECK_NO_BROKER=1` und `DECK_DOCK_EDGE=left`
setzen (Details in `CLAUDE.md`, Abschnitt „Probebetrieb“).

## Umstellen vom Python-Deck

```powershell
pwsh tools\umstellen.ps1 -SkipBuild   # Panel tauschen (Release vorher gebaut), settings.json bleibt
pwsh tools\umstellen.ps1 -Hooks       # neue Hooks dazu, über Git Bash bewiesen
pwsh tools\umstellen.ps1 -Finish      # Python-Hooks heraus, statusLine umstellen
pwsh tools\zurueck.ps1                # Rückweg zum Python-Deck, jederzeit
```

## Aufbau

| Ordner | Inhalt |
|---|---|
| `crates/deck-core` | Fachkern: Zustände, Reihenfolge, Neustart-Plan, Anzeigemodell; voll getestet |
| `crates/deck-win` | Win32: Fenster, Monitore, Prozesse, Tasten an Konsolen, Fokus-Rahmen |
| `crates/deck-hooks` | `deck-report.exe`, `deck-statusline.exe` für Claude Code |
| `crates/deck-services` | Nutzung, Zusammenfassung, Worktrees, VS-Code-Neustart, Wächter, Einrichtung |
| `app/src-tauri` | die App: Fenster, Andocken, Broker, Windows-Terminal-Host, Befehle |
| `app/src` | die Oberfläche |
| `extension/` | VS-Code-Extension (Brücke zum Broker) |
| `docs/` | `DESIGN.md` (verbindliche Gestaltung), `IPC.md` (Vertrag Oberfläche ↔ Rust) |

Regeln und Fallen für alle, die daran arbeiten: `CLAUDE.md`.
