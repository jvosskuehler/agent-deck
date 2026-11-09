# Agent Deck (Tauri)

Dashboard für parallel laufende Claude-Code-Agenten in **Windows Terminal** (seit
09.11.2026 nur dort; die VS-Code-Extension samt Broker ist entfernt, Stand davor in der
Historie).
Eine Kachel je Agent, Farbe = Zustand; dockt am Bildschirmrand an und klappt weg.
Windows-only. Rust (Tauri 2) hinten, React + TypeScript vorn.

Die frühere Python/tkinter-Fassung liegt nur noch in der Historie (letzter Stand
`2856494`, lesen mit `git show 2856494:<pfad>`).

## Lehre aus dem verworfenen .NET-Port (Juli 2026)

Der C#-Port hatte die Rechnung sauber portiert und golden-getestet, aber das Aussehen
fehlte, und genau daran ist er gescheitert. Deshalb hier umgekehrt: **die Oberfläche ist
nicht Nebensache, sie ist der Grund für den Umbau.** Jede Änderung am Aussehen wird
angeschaut (Screenshot), nicht nur gebaut. Die Vorgabe steht in `docs/DESIGN.md`.

## Aufbau

```
Cargo.toml              Workspace
crates/deck-core/       Fachkern: Zustände, Pfade, Konfiguration, Reihenfolge, Flag-Zeile für
                        claude, Windows-Terminal-Rechnung, Ticket/PR-Erkennung, Usage-Auswertung,
                        settings.json, Anzeigemodell (view.rs). Kein Win32, kein Tauri, voll
                        getestet.
crates/deck-win/        Win32: Fenster finden/fokussieren/anordnen, Monitore, DPI, Prozesse,
                        Tasten in fremde Konsolen, Fokus-Rahmen (GPU), globale Tastenkürzel.
crates/deck-hooks/      Die Claude-Code-Hooks als kleine Programme: deck-report, deck-statusline.
crates/deck-services/   Hintergrunddienste: Usage-Abruf (gemeinsamer Poller), KI-Zusammenfassung,
                        Worktree-Aufräumen und Sweep, Hook-Installation und Entfernen der
                        früheren VS-Code-Extension (deck-setup), Watchdog (deck-watchdog).
                        Wirkung nach außen hinter Traits, in Tests gesperrt.
app/src-tauri/          Die App: Fenster, Andocken, Windows-Terminal-Host, Poll-Schleife, Usage,
                        Zusammenfassung, Worktrees, Befehle für die Oberfläche.
app/src/                React-Oberfläche: Panel, Kacheln, Griff, Tooltip, Einstellungen.
app/src/bindings/       aus Rust erzeugte TypeScript-Typen (nicht von Hand ändern).
docs/DESIGN.md          verbindliche Gestaltung.
```

Abhängigkeiten zeigen nur nach unten: `app` → `deck-services`, `deck-win`, `deck-core`;
`deck-services` → `deck-win`, `deck-core`; `deck-hooks` → `deck-core`; `deck-core` → nichts
aus dem Projekt. Ein Worktree mit nicht committeter Arbeit wird nie automatisch gelöscht
(strenger als die Python-Fassung, die `--force` nahm). **Rechnen gehört nach `deck-core` und
wird dort getestet; Zeichnen gehört nach `app/src` und wird angeschaut.**

## Kommandos

```powershell
cargo test --workspace                               # alle Rust-Tests
cargo test -p deck-core export_bindings              # TS-Typen nach app/src/bindings erzeugen
cd app; pnpm install; pnpm tauri dev                 # App im Entwicklungsmodus
cd app; pnpm dev                                     # nur Oberfläche im Browser, Daten aus dem Mock
cd app; pnpm build; pnpm test                        # Typprüfung, Build, Oberflächen-Tests
```

`cargo` liegt unter `%USERPROFILE%\.cargo\bin` (rustup, ohne PATH-Eintrag installiert).

### Bauen

Neben `agent-deck.exe` müssen `deck-console`, `deck-report`, `deck-statusline` und
`deck-watchdog` liegen. Sie gehören zum selben Workspace, landen also im selben
`target/<profil>/`; `app/scripts/helpers.mjs` baut sie (`pnpm helpers` bzw.
`pnpm helpers:release`) und legt Kopien mit Zielsystem-Anhängsel nach
`app/src-tauri/binaries/` (nicht eingecheckt). `pnpm tauri dev` und `pnpm tauri build`
rufen das selbst auf (`beforeDevCommand`/`beforeBuildCommand`). `pnpm tauri build` baut nur
die Exe; das Installationspaket macht `pnpm bundle` (zusätzlich `tauri.bundle.conf.json`
mit `externalBin`). `wt_agent.ps1` und `agent-taskbar.ico` (in `resources/`) sind in die
Exe eingebaut und werden beim Start in den App-Ordner geschrieben.

Installiert und aktualisiert wird mit `pwsh tools\install.ps1` (`-SkipBuild` nimmt das
schon gebaute Release): bauen, Deck und Wächter beenden, alle Programme samt `deck-setup`
nach `%LOCALAPPDATA%\claude-agent-deck\bin` kopieren, `deck-setup remove-vscode-extension`
(nimmt die frühere VS-Code-Extension aus `~/.vscode/extensions`, nur ihren Eintrag und
Ordner; Übergangsschritt, kann später entfallen), `deck-setup install` und `check` (Hooks
in `~/.claude/settings.json`), Deck starten und prüfen, dass es läuft.
`deck-setup uninstall` nimmt die Hooks wieder heraus.

## Daten und Verträge

1. **Zustandsdateien** `%LOCALAPPDATA%\claude-agent-deck\state\<slot>.json` (Hooks schreiben,
   App liest; zwei Prozesse). Feldnamen snake_case, `ts` Unix-Sekunden als Float. Immer
   atomar schreiben (`.tmp` + ersetzen, kompaktes UTF-8-JSON über serde), nie mit Sperre
   lesen. Gelesen wird tolerant: ein Feld mit falschem Typ gilt als fehlend
   (`deck_core::field`), kein Hook scheitert daran. `deck-report` und
   `deck-statusline` schreiben, die App liest; das Format bleibt stabil, wer ein Feld
   ändert, ändert beide Seiten. Ebenso `<slot>.live.json`, `.ticket`, `.worktree`, `.wtpid`.
2. **Laufzeitdateien der App** (Bindungen, Reihenfolge, Einstellungen, Tickets, WT-Hosts …)
   liegen in `%LOCALAPPDATA%\claude-agent-deck\`, nicht neben dem Code.
3. **Ein Hook endet nie mit Fehler.** Bei `UserPromptSubmit`/`PreToolUse` liest Claude Code
   Exit ≠ 0 als Veto. Jeder Pfad in `deck-hooks` hat ein Fangnetz und Exit-Code 0; jeder
   Eintrag in `settings.json` endet zusätzlich auf `|| exit 0`. **Kein `cmd /c` davor**
   (MSYS macht aus `/c` den Pfad `C:\`, `cmd` liest dann das Hook-JSON als Befehl).
4. **Hook-stdin ist UTF-8**, roh lesen und selbst dekodieren.
5. **Der Slot eines Agenten kommt nur aus `AGENT_SLOT`**, gesetzt von `wt_agent.ps1` im
   Fenster des Agenten. Ohne ihn meldet kein Hook etwas: eine selbst gestartete Session
   gehört nicht zum Deck.

## Fallen, die schon einmal wehgetan haben

- **Tests dürfen nie echte Eingriffe auslösen** (taskkill, Fenster schließen, Programme
  starten). Alles mit Wirkung nach außen steckt hinter einem Trait, Tests bekommen eine
  Attrappe. Am 2026-11-06 hat die Python-Suite zweimal das VS Code beendet, in dem sie lief.
- **Usage-Werte:** ein Limit hinter seinem `resets_at` ist keine Zahl mehr (→ „—“); ein
  alter Wert wird matt gezeigt, nicht versteckt; das Deck pollt den Endpoint **nicht**
  selbst nach, wenn der gemeinsame Poller (`claude-usage-shared`) einen Cache hat. Der
  Endpoint ist eng limitiert, 429 heißt zurückweichen. `%LOCALAPPDATA%\claude-usage\usage.json`
  liest und schreibt weiter auch der Tray-Poller in Python: Felder, `account` und
  Zahlentypen bleiben, wie `usage/shared_format_tests.rs` sie festhält.
- **Ein maximiertes Fenster klebt an seinem Monitor:** erst `SW_SHOWNORMAL` auf die
  Zielfläche, dann maximieren.
- **Ein halb ausgefahrenes Deck ist der eine unzulässige Zustand.** Die Ein-/Ausklapp-
  Animation hat genau einen Ausgang, eine Deadline und landet im Zweifel ganz offen.
- **Der „gesehen“-Merker hält über den Poll hinaus** (in der Datei steht weiter `done`).

## Konventionen

- Kommentare und Doku auf Deutsch, sie erklären das Warum. Bezeichner englisch.
- Eine Datei = ein Konzept, unter 400 Zeilen.
- Testnamen beschreiben die Regel, nicht die Methode.
- In Texten der Oberfläche keine Gedankenstriche („—“, „–“).
- Keine Commits ohne ausdrücklichen Auftrag.

## Offen

- **Im Betrieb prüfen**, was nur gegen Attrappen getestet ist: Strg+< springt zum
  Wartenden, Klick auf eine Karte holt das richtige Terminal, ＋ legt einen Agenten an,
  Worktree-Aufräumen, `deck-setup remove-vscode-extension` auf dem echten Rechner.
  Protokoll: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`.
- **Taskleisten-Abzeichen:** der dringlichste Status als Overlay auf dem Agenten-Knopf
  (`ITaskbarList3::SetOverlayIcon`, wirkt auch auf fremde Fenster, gemessen 09.11.2026).
- **Makropad:** 6 Tasten mit Drehregler und ARGB je Taste (Taste = Agent, Licht = Status,
  Druck = zum Agenten springen, Halten = Aufnahme mit Claude `/voice`).
- **Rest des letzten Python-Commits:** der Rust-Port beruht auf `1c41595`, nicht auf
  `2856494`. Nachgezogen sind automatisches `/compact` (`deck-core::prompt_cache`,
  `auto_compact`), Konto je Token samt Konto im gemeinsamen Cache (`usage/account.rs`),
  `limits` und `pr` in der Statuszeile (`deck-hooks::status_extras`) und der Hub-Prompt
  mit `wt/<Nummer>-<Kürzel>`; offen ist die Cache-Restzeit auf der Kachel (braucht eine
  Design-Entscheidung).
