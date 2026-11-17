# Agent Deck

Ein am Bildschirmrand angedocktes Dashboard für parallel laufende **Claude-Code-Agenten**
in Windows Terminal. Eine Karte je Agent, die Farbe ist der Zustand: wer
denkt, wer eine Rückfrage hat, wer fertig ist. Klick holt genau diesen Agenten nach vorn.

Windows-only. Rust (Tauri 2) und React/TypeScript.

## Installieren

Voraussetzungen: Windows 10/11 oder Server 2025, Windows Terminal, die WebView2-Laufzeit
(auf Windows 11 schon da) und Claude Code (`claude`). Fehlt etwas, sagt der Installer,
wie es nachzuholen ist (`winget install --source winget -e --id …`). Rust, Node und
PowerShell 7 braucht es nicht; Administratorrechte auch nicht, alles landet im eigenen
Benutzerprofil.

1. Unter [Releases](https://github.com/jvosskuehler/agent-deck/releases/latest)
   `agent-deck-<version>-win-x64.zip` laden.
2. Rechtsklick auf das Zip, „Alle extrahieren“.
3. Im entpackten Ordner eine PowerShell öffnen und starten:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```

Oder alles in einem Schritt (in einer PowerShell, lädt das neueste Release):

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/jvosskuehler/agent-deck/main/tools/install.ps1).TrimStart([char]0xFEFF))) -Download
```

Beim ersten Mal fragt der Installer:

- **Repos-Ordner**: der Ordner, in dem alle deine git-Repos liegen (je Repo ein
  Unterordner). Dort starten die Agenten. Vorschlag: `~\Repos`, sonst `~\source\repos`,
  sonst der Ordner mit den meisten Repos.
- **Jira-Kürzel** (z. B. `ORPH`), leer lassen, wenn du kein Jira nutzt.
- **statusLine**: nur, wenn in Claudes `settings.json` schon eine andere steht. Behalten
  oder durch die des Decks ersetzen (dann zeigen die Kacheln Modell, Effort und Kontext-%).
- **Autostart** beim Anmelden.

Ein späterer Lauf ist ein Update und fragt nichts mehr. Ohne Rückfragen geht es mit
Parametern: `-NonInteractive -ReposRoot <ordner> -JiraPrefix <kürzel> -StatusLine keep|replace
-Autostart -NoShortcut`.

**Was der Installer am Rechner ändert:**

| Was | Wo |
|---|---|
| Programme | `%LOCALAPPDATA%\claude-agent-deck\bin` |
| Einstellungen, Protokoll, Zustände | `%LOCALAPPDATA%\claude-agent-deck` |
| Sechs Hooks und (nur wenn gewünscht oder noch keine da) die statusLine | Claudes `settings.json` (`%USERPROFILE%\.claude` bzw. `CLAUDE_CONFIG_DIR`); eine ersetzte statusLine wird gesichert |
| Startmenü-Eintrag „Agent Deck“ | `%APPDATA%\Microsoft\Windows\Start Menu\Programs` |
| Windows-Terminal-Profil „Agent Deck“ und die Farbschemata `papier-ebenen` | Fragment in `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\AgentDeck`; die `settings.json` des Terminals bleibt unberührt |
| Autostart (wenn gewünscht) | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, Wert `AgentDeck` |

PR-Automatik, Titan, Store-Update, SpeechMike, Auto-Modus und Auto-Compact sind nach der
Installation aus. Anschalten im Deck unter Einstellungen.

**Entfernen:**

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\claude-agent-deck\bin\uninstall.ps1"
```

Nimmt Hooks und statusLine heraus (eine ersetzte statusLine kommt zurück), entfernt
Startmenü-Eintrag, Terminal-Profil und Autostart und fragt, ob Einstellungen und Protokoll
mit weg sollen (`-KeepData` / `-RemoveData`).

## Bauen und starten (aus dem Quellcode)

```powershell
cd app
pnpm install
pnpm tauri dev          # Entwicklungsmodus
pnpm dev                # nur die Oberfläche im Browser, mit Beispieldaten
cargo test --workspace  # alle Rust-Tests (aus der Repo-Wurzel)
```

Aus dem Checkout installieren und aktualisieren, mit demselben Installer:

```powershell
pwsh tools\install.ps1              # bauen, installieren, Hooks einrichten, starten
pwsh tools\install.ps1 -SkipBuild   # dasselbe mit dem schon gebauten Release
pwsh tools\package.ps1              # Paket dist\agent-deck-<version>-win-x64.zip bauen
```

Ein Release entsteht mit einem Tag `v<version>` (Version aus
`app/src-tauri/tauri.conf.json`): `.github/workflows/release.yml` baut das Paket und hängt
es samt `install.ps1` an das GitHub-Release.

Die Symbole (Taskleiste der Terminal-Agenten, Vorlage des App-Symbols) zeichnet
`tools/make_icons.py`.

## Aufbau

| Ordner | Inhalt |
|---|---|
| `crates/deck-core` | Fachkern: Zustände, Reihenfolge, Anzeigemodell; voll getestet |
| `crates/deck-win` | Win32: Fenster, Monitore, Prozesse, Tasten an Konsolen, Fokus-Rahmen |
| `crates/deck-hooks` | `deck-report.exe`, `deck-statusline.exe` für Claude Code |
| `crates/deck-services` | Nutzung, Zusammenfassung, Worktrees, Wächter, Einrichtung (`deck-setup`) |
| `app/src-tauri` | die App: Fenster, Andocken, Windows-Terminal-Host, Befehle |
| `app/src` | die Oberfläche |
| `docs/` | `DESIGN.md` (verbindliche Gestaltung), `IPC.md` (Vertrag Oberfläche ↔ Rust), `DATEIEN.md` (Dateiverträge Hooks ↔ Deck), `BETRIEB.md` (Bauen, Installieren, Diagnose, Offenes) |

Regeln und Fallen für alle, die daran arbeiten: `CLAUDE.md`.
