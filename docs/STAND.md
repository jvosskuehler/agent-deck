# Stand der Umstellung (Übergabe vom 07.11.2026)

Diese Datei ist die Übergabe an die nächste Session. Die Session, die den Umbau gemacht
hat, lässt sich nicht fortsetzen; alles Nötige steht hier, in `CLAUDE.md`,
`docs/DESIGN.md` und `docs/IPC.md`.

## Was erledigt ist

- Das Agent Deck ist vollständig von Python/tkinter nach **Tauri 2 (Rust) + React/TypeScript**
  übersetzt, im Worktree `C:\Users\JorritVoßkühler\Repos\wt-deck-tauri`, Branch `tauri`.
  **Nichts ist committet** (Jorrits Regel: Commits nur auf ausdrücklichen Auftrag).
- Gestaltung nach Jorrits Wahl im Canvas (https://claude.ai/artifact/CRtjbuWwddHZNFV35A2kxR,
  Seite „Runde 3“): Farbe/Leuchten aus Variante 01, Kartenaufteilung aus 05, normales
  Zahnrad, ＋-Karte in Kartengröße mit gestricheltem Rand, Einstellungen wie „Glas“ aus
  Runde 1 auf Schwarz. Aufbau und Bedienung wie im alten Deck.
- **09.11.2026: Papier-Look** (Variante „02 Farbfeld“, Canvas
  https://claude.ai/artifact/HDeLhN3i7DADxWqu4EoPzz): Rosé Pine Dawn hell / Rosé Pine
  dunkel nach System-Thema, IBM Plex Mono, Karte als getöntes Farbfeld ohne Rand und
  Leuchten. Werte in `docs/DESIGN.md`, `app/src/logic/palette.ts`, `app/src/styles/base.css`.
  Im Browser-Mock hell und dunkel angeschaut; das Release unter `target\release` ist noch
  der schwarze Stand und muss neu gebaut werden.
- Tests: 556 Rust (`cargo test --workspace`), 61 Vitest (`cd app; pnpm test`), Clippy sauber.
- Live geprüft im Probebetrieb neben dem alten Deck (ohne Broker): echte Agenten mit
  Sessiontitel, Aus-/Einklappen, Tooltip, Kontextmenü, Ziehen, Einstellungen, Zoom, freies Panel.
  Screenshots in `docs/screens/live-*.png`.
- Release gebaut: `target\release\agent-deck.exe` + `deck-report`, `deck-statusline`,
  `deck-console`, `deck-watchdog`, `deck-setup`.
- Umstellung und Rückweg als Skripte (`tools\umstellen.ps1`, `tools\zurueck.ps1`), an einer
  Kopie der settings.json erprobt; der Rückweg ist chirurgisch (nur Deck-Einträge, fremde
  Hooks wie Jorrits PermissionRequest-Hook bleiben).

## Was als Nächstes kommt

1. **Umstellen** (Jorrit startet die Befehle selbst; der Auto-Modus sperrt Claude das Beenden
   des laufenden Python-Panels und das Ändern der Hooks in `~/.claude/settings.json`):
   ```
   pwsh -NoProfile -File "C:/Users/JorritVoßkühler/Repos/wt-deck-tauri/tools/umstellen.ps1" -SkipBuild   # Panel tauschen
   pwsh -NoProfile -File "C:/Users/JorritVoßkühler/Repos/wt-deck-tauri/tools/umstellen.ps1" -Hooks       # neue Hooks dazu
   pwsh -NoProfile -File "C:/Users/JorritVoßkühler/Repos/wt-deck-tauri/tools/umstellen.ps1" -Finish      # Python-Hooks heraus
   pwsh -NoProfile -File "C:/Users/JorritVoßkühler/Repos/wt-deck-tauri/tools/zurueck.ps1"                # Rückweg
   ```
   In einer Claude-Session mit `! ` davor eingeben.
2. **Nach dem Tausch prüfen**, was im Probebetrieb abgeschaltet war und nur gegen Attrappen
   getestet ist: VS-Code-Fenster verbinden sich mit dem Broker (Port 8765), Kacheln der
   Windows-Terminal-Fenster, Strg+< springt zum Wartenden, Fokus-Rahmen um das vorderste
   Agentenfenster, Klick auf eine Karte holt das richtige Terminal, ＋ legt einen Agenten
   an, Worktree-Aufräumen. Protokoll: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`.
3. **Fokus-Rahmen nachziehen**, falls die andere Session ihn in der Python-Referenz nach
   dem 06.11., 23:19 Uhr noch geändert hat (`focus-frame/Frame.hlsl`, `Designs.cs` im
   Haupt-Checkout → `crates/deck-win/src/frame.hlsl`, `frame_fx.rs`; Test
   `gpu_and_cpu_agree` muss grün bleiben).
4. Wenn alles läuft: Jorrit fragen, ob committet und nach `main` gebracht werden soll
   (im Haupt-Checkout `Repos\agent-deck` liegen fremde, nicht committete Änderungen am
   Python-Fokus-Rahmen).
5. Danach das nächste Vorhaben: die **6-Tasten-Makro-Tastatur** mit Drehregler und ARGB je
   Taste (Taste = Agent, Licht = Status, Druck = zum Agenten springen, Halten = Aufnahme mit
   Claude `/voice`, später ORPHEUS, je Agent einstellbar; bei ORPHEUS nach dem Loslassen
   noch einmal ins Eingabefeld klicken). Eine andere Session hat dazu schon experimentiert
   (`pad_led_manual.py`).

## Nachtrag 08.11.: Taste „alle nach vorn“

In beiden Fassungen gleich gebaut (Python zuerst, dort live geprüft): `Strg+Shift+<` und
F20 (Drehregler am Makropad gedrückt) holen jedes Agentenfenster in Windows Terminal als
ein Raster auf den Monitor unter der Maus, in Deck-Reihenfolge, auch minimierte; den Fokus
bekommt, wer am längsten wartet. Steht man schon bei einem Agenten, führt der Druck zurück
in das Fenster, aus dem man kam. VS Code bleibt außen vor. Einstellung `front_hotkeys`
(Text oder Liste, leer = aus). F13 bis F24 gehen seither auch ohne Strg/Alt.

Hier: Rechnung `crates/deck-core/src/front.rs`, Raster `app/src-tauri/src/wt_tiles.rs`
(`gather`, `bring_front`), Ablauf `actions.rs` (`bring_all_front`), Tasten `hotkey.rs`;
`Desktop` hat dafür `restore` und `is_alive` bekommen. Das Release in `target\release`
ist mit der Taste neu gebaut. Nach dem Umstellen mitprüfen (Punkt 2 oben).

## Abweichungen vom alten Deck (bewusst)

- Ein Worktree mit nicht committeter Arbeit wird nie automatisch gelöscht (Python nahm `--force`).
- Lage und Zoom des freien Panels bleiben über Neustarts erhalten.
- Blockkopf hat ein natives Rechtsklickmenü („Repo vergessen“, „Mit VS-Code-Fenster
  verbinden“); in Python war das toter Code. Ein ✕ für das ganze Fenster gibt es bewusst nicht.
- Usage-Tooltip trennt mit „·“ statt Gedankenstrich.
- Neue Agenten starten mit `--effort <Wert aus den Einstellungen>` (09.11.2026). Claude Code
  2.1.3xx wertet das globale `effortLevel` für aktuelle Modelle nicht mehr aus, nur noch
  `modelSettings.<Modell>.effortLevel`, das `/effort` im Chat schreibt. Ohne den Schalter
  startete jeder neue Agent mit der Modell-Vorgabe medium.
- Einstellungen ausgedünnt (09.11.2026): kein Bypass-Warnsatz, kein Ring, kein Jira-Präfix,
  kein Heimatordner, keine Werkzeuge außer „Panel neu starten“ (siehe `docs/DESIGN.md`).
- Laufzeitdateien liegen in `%LOCALAPPDATA%\claude-agent-deck\` statt im Repo; bis zum
  ersten Start mit echtem Broker werden sie bei jedem Start aus dem Python-Repo übernommen.

## Werkzeuge

Rust per rustup unter `%USERPROFILE%\.cargo\bin` (ohne PATH-Eintrag; in Befehlen
`export PATH="$HOME/.cargo/bin:$PATH"`). Gebaut wird aus der Worktree-Wurzel bzw. `app\`.
