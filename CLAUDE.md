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
| `docs/IPC.md` | du Fenster, Ereignisse oder `invoke`-Befehle zwischen Oberfläche und Rust änderst oder an der Erweiterungsschnittstelle arbeitest |
| `docs/DATEIEN.md` | du Zustandsdateien, `.wtpid`/Herkunftsmarke, `refs.json` oder die Ticket/PR-Erkennung anfasst |
| `docs/BETRIEB.md` | du baust, installierst, einen Fehler im Betrieb suchst (Log, Hänger, Absturz) oder offene Punkte suchst |

## Aufbau

```
crates/deck-core/       Fachkern: Zustände, Konfiguration, Reihenfolge, WT-Rechnung, Ticket/PR,
                        Usage, Anzeigemodell (view.rs). Kein Win32, kein Tauri, voll getestet.
crates/deck-win/        Win32: Fenster, Monitore, DPI, Prozesse, Tasten an Konsolen,
                        Fokus-Rahmen (GPU), globale Tastenkürzel.
crates/deck-hooks/      Claude-Code-Hooks: deck-report, deck-statusline.
crates/deck-services/   Hintergrunddienste: Usage-Poller, Zusammenfassung, Hell/Dunkel,
                        deck-setup. Wirkung nach außen hinter Traits.
app/src-tauri/          Die App: Fenster, Andocken, WT-Host, Poll-Schleife, Befehle,
                        Erweiterungsschnittstelle (extension.rs).
app/src/                React-Oberfläche. app/src/bindings/ ist aus Rust erzeugt, nie von Hand.
```

Dieses Repo ist die schlanke Ausgabe fürs Team. Persönliche Teile (Info-Fenster,
PR-Automatik, Diktiermikrofon, Store-Update, worktree-Aufräumen, Jira-Kürzel) liegen in
einer eigenen Ausgabe, die eine eigene Binärdatei gegen `agent_deck_lib::run_with` baut
und über `extension.rs` bzw. `app/src/extensions.ts` anbaut (`docs/IPC.md`,
„Erweiterungen“). Hier kommt davon nichts hinein, auch kein Schalter:
`crates/deck-core/tests/lean_edition.rs` sucht im Quellbaum nach Spuren.

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
4. **Der Slot eines Agenten kommt nur aus `AGENT_SLOT`** (gesetzt von `wt_agent.ps1`, bei
   Agenten in WSL von `deck-wsl.exe` über `WSLENV`). Ohne ihn meldet kein Hook etwas: eine
   selbst gestartete Session gehört nicht zum Deck.
5. **Ein Worktree mit nicht committeter Arbeit wird nie automatisch gelöscht.** Diese
   Ausgabe löscht gar keinen; ein frei gewordener Slot meldet sich nur bei Erweiterungen
   (`on_slot_released`), und wer dort aufräumt, hält sich daran.

## Agenten in WSL

Die Team-Ausgabe lässt Claude Code ganz in einer WSL-Distro laufen (`agent_env` = `wsl`,
`deck_core::wsl`); der Windows-Weg (PowerShell, `wt_agent.ps1`) bleibt. Im Fenster steht
`deck-wsl.exe` (Herkunftsmarke, Umgebung), das `wsl.exe … bash -li wt_agent.sh` startet.
**Zustandsdateien enthalten nur Windows-Pfade:** die Hooks rechnen Linux-Pfade aus dem
Hook-Eingang um (`\\wsl.localhost\<Distro>\…`, `/mnt/c` → `C:\`), das Deck rechnet
zurück, wo es dem Agenten oder `wsl.exe` einen Pfad gibt (`AgentEnv::for_agent`).
`deck-hooks` baut darum auch für Linux (`x86_64-unknown-linux-musl`): Windows-Eigenes
steht dort hinter `#[cfg(windows)]`, die Linux-Fassungen laufen in der Distro.
Einzelheiten: `docs/BETRIEB.md` („Agenten in WSL“), `docs/DATEIEN.md`.

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
- **Ein Store-Update von PowerShell beendet jeden Agenten.** Diese Ausgabe zieht
  PowerShell und Windows Terminal nicht selbst nach; stirbt die Shell eines Agenten, setzt
  `crash_resume` die Session fort (`docs/BETRIEB.md`).
- **Ein Overlay, das bei jedem Wechsel umzieht, zuckt.** Fensterlage und Swapchain-Bild
  kommen nicht im selben DWM-Bild an. Der Fokus-Rahmen ist darum eine feste Bühne über dem
  ganzen Monitor, nur der Inhalt bewegt sich (`frame_gpu_overlay`).
- **Fenster umlegen, das man nicht sehen soll: unter den Vorhang.** Öffnen und Schließen
  der Agenten und jedes Umlegen mit neuer Kachelgröße laufen unter einem Standbild
  (`Desktop::curtain`, `veil`). Umsortieren gleich großer Kacheln nicht: das sind nur
  Orte, die Fenster springen im selben Bild, ein Vorhang kostete nur Zeit. Vor
  dem Lüften `FrameHold` lösen und warten, bis Ringe und Schatten nachgezogen haben
  (`group_shadow::catch_up`), sonst stehen die Statusringe noch an alten Plätzen. Auch das
  Nachlegen nach einem Wechsel (`settle_due`) und nach neuer Auflösung geht darunter.
- **Ob ein Ablauf ruhig ist, wird gemessen, nicht geschätzt:** `tools/probe` (Test-VM,
  Ereignisprotokoll und Video, Kennzahlen je Ablauf). Vor und nach jeder Änderung an
  Kacheln, Vorhang, Ringen oder Glow laufen lassen.
- **Reihenfolge beim Nachkacheln nur über `arrange::keep_order`.** `Vec::dedup` entfernt
  nur benachbarte Doppelte; so lagen nach dem Schließen die übrigen Fenster als 2 × 2 in
  der unteren Hälfte.
- **Ein stiller Glow wird nicht neu gezeichnet** (`frame_still`): 60 Bilder je Sekunde
  für ein unverändertes Bild kosteten in der VM drei Kerne. Wer dem Shader etwas
  Zeitabhängiges gibt, muss es in `Still::animated` eintragen.

## Konventionen

- Kommentare und Doku auf Deutsch, sie erklären das Warum. Bezeichner englisch.
- Eine Datei = ein Konzept, unter 400 Zeilen.
- Testnamen beschreiben die Regel, nicht die Methode.
- In Texten der Oberfläche keine Gedankenstriche („—“, „–“).
- Keine Commits ohne ausdrücklichen Auftrag.
