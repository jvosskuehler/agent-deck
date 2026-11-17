# Agent Deck (Tauri)

Dashboard für parallel laufende Claude-Code-Agenten in **Windows Terminal**. Eine Kachel
je Agent, Farbe = Zustand; dockt am Bildschirmrand an und klappt weg. Windows-only. Rust
(Tauri 2) hinten, React + TypeScript vorn. Frühere Fassungen nur in der Historie:
VS-Code-Extension bis 09.11.2026, Python/tkinter bis `2856494`
(`git show 2856494:<pfad>`).

**Die Oberfläche ist nicht Nebensache, sie ist der Grund für den Umbau.** Der .NET-Port
(Juli 2026) hatte die Rechnung sauber portiert, aber das Aussehen fehlte, und daran ist
er gescheitert. Jede Änderung am Aussehen wird angeschaut (Screenshot), nicht nur gebaut,
und zwar in einer Windows-VM auf dem Titan (`titan vm`), nie auf Jorrits Laptop.

## Wo was steht

| Datei | Lesen, wenn … |
|---|---|
| `docs/DESIGN.md` | du etwas an der Oberfläche änderst (verbindlich, mit Vorlagen in `docs/design-refs/`) |
| `docs/IPC.md` | du Fenster, Ereignisse oder `invoke`-Befehle zwischen Oberfläche und Rust änderst |
| `docs/DATEIEN.md` | du Zustandsdateien, `.wtpid`/Herkunftsmarke, `refs.json` oder die Ticket/PR-Erkennung anfasst |
| `docs/BETRIEB.md` | du baust, installierst, einen Fehler im Betrieb suchst (Log, Hänger, Absturz) oder offene Punkte suchst |

## Aufbau

```
crates/deck-core/       Fachkern: Zustände, Konfiguration, Reihenfolge, WT-Rechnung, Ticket/PR,
                        Usage, Anzeigemodell (view.rs). Kein Win32, kein Tauri, voll getestet.
crates/deck-win/        Win32: Fenster, Monitore, DPI, Prozesse, Tasten an Konsolen,
                        Fokus-Rahmen (GPU), globale Tastenkürzel.
crates/deck-hooks/      Claude-Code-Hooks: deck-report, deck-statusline.
crates/deck-services/   Hintergrunddienste: Usage-Poller, Zusammenfassung, Worktrees,
                        deck-setup, deck-watchdog, Store-Update. Wirkung nach außen hinter Traits.
app/src-tauri/          Die App: Fenster, Andocken, WT-Host, Poll-Schleife, Befehle.
app/src/                React-Oberfläche. app/src/bindings/ ist aus Rust erzeugt, nie von Hand.
```

Abhängigkeiten zeigen nur nach unten: `app` → `deck-services` → `deck-win` → `deck-core`;
`deck-hooks` → `deck-core`; `deck-core` → nichts aus dem Projekt. **Rechnen gehört nach
`deck-core` und wird dort getestet; Zeichnen gehört nach `app/src` und wird angeschaut.**

## Kommandos

```powershell
cargo test --workspace                               # alle Rust-Tests
cargo test -p deck-core export_bindings              # TS-Typen nach app/src/bindings erzeugen
cd app; pnpm install; pnpm tauri dev                 # App im Entwicklungsmodus
cd app; pnpm dev                                     # nur Oberfläche im Browser, Daten aus dem Mock
cd app; pnpm build; pnpm test                        # Typprüfung, Build, Oberflächen-Tests
pwsh tools\install.ps1                               # bauen, installieren, Hooks, starten
pwsh tools\package.ps1                               # Paket dist\agent-deck-<version>-win-x64.zip
```

`tools\install.ps1`, `uninstall.ps1` und `package.ps1` müssen auch unter Windows
PowerShell 5.1 laufen (Kolleginnen und Kollegen haben oft kein pwsh): UTF-8 mit BOM,
keine 7er-Syntax (`??`, `?:`, `&&`), jeder `winget`-Aufruf mit `--source winget`.

`cargo` liegt unter `%USERPROFILE%\.cargo\bin` (ohne PATH-Eintrag). Das Deck läuft aus
`%LOCALAPPDATA%\claude-agent-deck\bin`: Änderungen sind erst nach `install.ps1` sichtbar.
Log: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`.

## Verträge

Einzelheiten in `docs/DATEIEN.md`.

1. **Hooks und App reden nur über Dateien** in `%LOCALAPPDATA%\claude-agent-deck\`.
   Immer atomar schreiben (`.tmp` + ersetzen), nie mit Sperre lesen, tolerant lesen
   (falscher Typ = fehlend, `deck_core::field`). Wer ein Feld ändert, ändert beide Seiten.
2. **Ein Hook endet nie mit Fehler.** Bei `UserPromptSubmit`/`PreToolUse` liest Claude Code
   Exit ≠ 0 als Veto. Jeder Pfad in `deck-hooks` hat ein Fangnetz und Exit-Code 0; jeder
   Eintrag in `settings.json` endet zusätzlich auf `|| exit 0`. **Kein `cmd /c` davor**
   (MSYS macht aus `/c` den Pfad `C:\`, `cmd` liest dann das Hook-JSON als Befehl).
3. **Hook-stdin ist UTF-8**, roh lesen und selbst dekodieren.
4. **Der Slot eines Agenten kommt nur aus `AGENT_SLOT`** (gesetzt von `wt_agent.ps1`).
   Ohne ihn meldet kein Hook etwas: eine selbst gestartete Session gehört nicht zum Deck.
5. **Ein Worktree mit nicht committeter Arbeit wird nie automatisch gelöscht.**

## Fallen, die schon einmal wehgetan haben

- **Tests dürfen nie echte Eingriffe auslösen** (taskkill, Fenster schließen, Programme
  starten). Alles mit Wirkung nach außen steckt hinter einem Trait, Tests bekommen eine
  Attrappe. Die Python-Suite hat zweimal das VS Code beendet, in dem sie lief.
- **Usage-Werte:** ein Limit hinter seinem `resets_at` ist keine Zahl mehr (→ „—“); ein
  alter Wert wird matt gezeigt, nicht versteckt; das Deck pollt den Endpoint **nicht**
  selbst, wenn der gemeinsame Poller (`claude-usage-shared`) einen Cache hat. 429 heißt
  zurückweichen. `%LOCALAPPDATA%\claude-usage\usage.json` teilt sich das Deck mit dem
  Tray-Poller in Python: Format bleibt, wie `usage/shared_format_tests.rs` es festhält.
- **Ein maximiertes Fenster klebt an seinem Monitor:** erst `SW_SHOWNORMAL` auf die
  Zielfläche, dann maximieren.
- **Ein halb ausgefahrenes Deck ist der eine unzulässige Zustand.** Die Ein-/Ausklapp-
  Animation hat genau einen Ausgang, eine Deadline und landet im Zweifel ganz offen.
- **Der „gesehen“-Merker hält über den Poll hinaus** (in der Datei steht weiter `done`).
- **Ein Deck, das lebt, aber nichts zeigt, blockiert jeden Neustart.** Dagegen `ui_watch`
  und `instance_guard`; nicht entfernen (`docs/BETRIEB.md`).
- **Ein Store-Update von PowerShell beendet jeden Agenten.** PowerShell und Windows
  Terminal nur aktualisieren, solange kein Agent lebt (`store_update`, `crash_resume`,
  `docs/BETRIEB.md`).
- **Ein Overlay, das bei jedem Wechsel umzieht, zuckt.** Fensterlage und Swapchain-Bild
  kommen nicht im selben DWM-Bild an. Der Fokus-Rahmen ist darum eine feste Bühne über dem
  ganzen Monitor, nur der Inhalt bewegt sich (`frame_gpu_overlay`).
- **Fenster umlegen, das man nicht sehen soll: unter den Vorhang.** Öffnen, Schließen und
  Umsortieren der Agenten laufen unter einem Standbild (`Desktop::curtain`, `veil`); vor
  dem Lüften `FrameHold` lösen und kurz warten, sonst stehen die Statusringe noch an alten
  Plätzen.

## Konventionen

- Kommentare und Doku auf Deutsch, sie erklären das Warum. Bezeichner englisch.
- Eine Datei = ein Konzept, unter 400 Zeilen.
- Testnamen beschreiben die Regel, nicht die Methode.
- In Texten der Oberfläche keine Gedankenstriche („—“, „–“).
- Keine Commits ohne ausdrücklichen Auftrag.
