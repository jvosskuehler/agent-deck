# Agent Deck (Tauri)

Dashboard für parallel laufende Claude-Code-Agenten in **Windows Terminal** und VS Code.
Eine Kachel je Agent, Farbe = Zustand; dockt am Bildschirmrand an und klappt weg.
Windows-only. Rust (Tauri 2) hinten, React + TypeScript vorn.

Diese Fassung ersetzt das Python/tkinter-Deck **ohne Funktionsänderung**: was das alte
Deck kann, kann dieses auch, nur schöner. Die Python-Fassung ist die **Referenz**: sie liegt seit dem Merge
nach `main` (09.11.2026) nur noch in der Historie, letzter Stand `2856494`
(`git show 2856494:<pfad>`). Im Zweifel gilt, was sie tut. Was ihr gegenüber noch fehlt,
steht in `docs/STAND.md` unter „Was offen ist“.

## Lehre aus dem verworfenen .NET-Port (Juli 2026)

Der C#-Port hatte die Rechnung sauber portiert und golden-getestet, aber das Aussehen
fehlte, und genau daran ist er gescheitert. Deshalb hier umgekehrt: **die Oberfläche ist
nicht Nebensache, sie ist der Grund für den Umbau.** Jede Änderung am Aussehen wird
angeschaut (Screenshot), nicht nur gebaut. Die Vorgabe steht in `docs/DESIGN.md`.

## Aufbau

```
Cargo.toml              Workspace
crates/deck-core/       Fachkern: Zustände, Pfade, Konfiguration, Reihenfolge, Neustart-Plan,
                        Protokoll, Ticket/PR-Erkennung, Usage-Auswertung, settings.json,
                        Anzeigemodell (view.rs). Kein Win32, kein Tauri, voll getestet.
crates/deck-win/        Win32: Fenster finden/fokussieren/anordnen, Monitore, DPI, Prozesse,
                        Tasten in fremde Konsolen, Overlay, globale Tastenkürzel.
crates/deck-hooks/      Die Claude-Code-Hooks als kleine Programme: deck-report, deck-statusline.
crates/deck-services/   Hintergrunddienste: Usage-Abruf (gemeinsamer Poller), KI-Zusammenfassung,
                        Worktree-Aufräumen und Sweep, VS-Code-Neustart, Fenster einsammeln, Ring
                        um den Chat, Extension-Registratur, Hook-Installation, Watchdog
                        (deck-watchdog). Wirkung nach außen hinter Traits, in Tests gesperrt.
app/src-tauri/          Die App: Fenster, Andocken, Broker zur VS-Code-Extension, Windows-
                        Terminal-Host, Poll-Schleife, Usage, Zusammenfassung, Worktrees,
                        VS-Code-Neustart, Befehle für die Oberfläche.
app/src/                React-Oberfläche: Panel, Kacheln, Griff, Tooltip, Einstellungen.
app/src/bindings/       aus Rust erzeugte TypeScript-Typen (nicht von Hand ändern).
extension/              VS-Code-Extension (JavaScript, unverändertes Protokoll).
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
mit `externalBin`). `wt_agent.ps1` und `agent-deck-glow.css` (in `resources/`) sind in die
Exe eingebaut und werden beim Start in den App-Ordner geschrieben.

### Probebetrieb neben dem Python-Deck

`DECK_NO_BROKER=1` (oder `DECK_BROKER_PORT=<anderer Port>`) und `DECK_DOCK_EDGE=off|left|…`
(Kante nur für diesen Lauf). Ohne eigenen Broker auf 8765 räumt das Deck nichts ab, kachelt
keine Terminal-Fenster, erzeugt keine Zusammenfassungen vorab, belegt die Sprung-Taste
nicht und zeichnet keinen Fokus-Rahmen. Bis zur Übernahme (erster Start mit echtem
Broker, Merker `python-adopted` im App-Ordner) holt jeder Start die Laufzeitdateien neu
aus dem Python-Repo (`DECK_LEGACY_ROOT` überschreibt den Ort; abweichende alte als `.bak`).

## Daten und Verträge

1. **Zustandsdateien** `%LOCALAPPDATA%\claude-agent-deck\state\<slot>.json` (Hooks schreiben,
   App liest; zwei Prozesse). Feldnamen snake_case, `ts` Unix-Sekunden als Float. Immer
   atomar schreiben (`.tmp` + ersetzen), nie mit Sperre lesen. Format exakt wie in der
   Python-Fassung (`deck/domain/slot_state.py`, `deck/claude/hooks/report.py`,
   `statusline.py`), damit beide Fassungen während der Umstellung dieselben Dateien lesen.
   Ebenso `<slot>.live.json`, `.ticket`, `.worktree`, `.wtpid`.
2. **Laufzeitdateien der App** (Bindungen, Reihenfolge, Einstellungen, Tickets, WT-Hosts …)
   liegen in `%LOCALAPPDATA%\claude-agent-deck\`, nicht mehr neben dem Code. Beim ersten
   Start werden die alten aus dem Python-Repo übernommen.
3. **Ein Hook endet nie mit Fehler.** Bei `UserPromptSubmit`/`PreToolUse` liest Claude Code
   Exit ≠ 0 als Veto. Jeder Pfad in `deck-hooks` hat ein Fangnetz und Exit-Code 0; jeder
   Eintrag in `settings.json` endet zusätzlich auf `|| exit 0`. **Kein `cmd /c` davor**
   (MSYS macht aus `/c` den Pfad `C:\`, `cmd` liest dann das Hook-JSON als Befehl).
   Umstellen heißt: erst den neuen Pfad beweisen (eine Datei in `state\` wird frisch),
   dann den alten entfernen.
4. **Hook-stdin ist UTF-8**, roh lesen und selbst dekodieren.
5. **Das Protokoll zur Extension** (newline-getrenntes JSON über TCP 127.0.0.1:8765) ist
   `deck-core::protocol` und `extension/extension.js`. Wer einen String ändert, ändert beide.
   **Kein `SO_REUSEADDR`** auf dem Broker-Port (Windows erlaubt sonst zwei Listener).
6. **VS Code lädt die Extension nur, wenn sie in `~/.vscode/extensions/extensions.json`
   registriert ist**, der Ordner allein beweist nichts; dort stehen alle Extensions des
   Nutzers, also nur ergänzen. Fehlt eine der Nebendateien, lädt VS Code gar nichts.
7. **`Ctrl+Alt+K` (Extension) ist eine Sicherheitsfrage:** Claude, Sitzung 0 und der
   Broker-Port bleiben gesperrt (`extension/killable.js`).
8. **Neustart mit Chats** braucht `session_id` UND `cwd` je Slot, prüft, ob das Transcript
   noch existiert, prüft echte Prozesse statt zu warten (unlesbare Liste = Nein), schließt
   Terminals vor Fenstern, und setzt währenddessen Aufräumarbeiten aus.

## Fallen, die schon einmal wehgetan haben

- **Tests dürfen nie echte Eingriffe auslösen** (taskkill, Fenster schließen, `code`
  starten). Alles mit Wirkung nach außen steckt hinter einem Trait, Tests bekommen eine
  Attrappe. Am 2026-11-06 hat die Python-Suite zweimal das VS Code beendet, in dem sie lief.
- **Usage-Werte:** ein Limit hinter seinem `resets_at` ist keine Zahl mehr (→ „—“); ein
  alter Wert wird matt gezeigt, nicht versteckt; das Deck pollt den Endpoint **nicht**
  selbst nach, wenn der gemeinsame Poller (`claude-usage-shared`) einen Cache hat. Der
  Endpoint ist eng limitiert, 429 heißt zurückweichen.
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
