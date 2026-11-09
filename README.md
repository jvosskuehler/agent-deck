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

## Installieren und aktualisieren

```powershell
pwsh tools\install.ps1              # bauen, nach %LOCALAPPDATA%\claude-agent-deck\bin kopieren, Hooks einrichten, starten
pwsh tools\install.ps1 -SkipBuild   # dasselbe mit dem schon gebauten Release
```

Die Symbole (Taskleiste der Terminal-Agenten, Vorlage des App-Symbols) zeichnet
`tools/make_icons.py`.

## Aufbau

| Ordner | Inhalt |
|---|---|
| `crates/deck-core` | Fachkern: Zustände, Reihenfolge, Anzeigemodell; voll getestet |
| `crates/deck-win` | Win32: Fenster, Monitore, Prozesse, Tasten an Konsolen, Fokus-Rahmen |
| `crates/deck-hooks` | `deck-report.exe`, `deck-statusline.exe` für Claude Code |
| `crates/deck-services` | Nutzung, Zusammenfassung, Worktrees, Wächter, Einrichtung (`deck-setup`) |
| `app/src-tauri` | die App: Fenster, Andocken, Broker, Windows-Terminal-Host, Befehle |
| `app/src` | die Oberfläche |
| `extension/` | VS-Code-Extension (Brücke zum Broker) |
| `docs/` | `DESIGN.md` (verbindliche Gestaltung), `IPC.md` (Vertrag Oberfläche ↔ Rust) |

Regeln und Fallen für alle, die daran arbeiten: `CLAUDE.md`.
