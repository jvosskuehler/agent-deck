# Stand der Umstellung (Übergabe vom 07.11.2026, nachgeführt 09.11.2026)

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

## Stand 09.11.2026: umgestellt, in `main`

- Auf Jorrits Rechner läuft nur noch das Rust-Deck (`%LOCALAPPDATA%\claude-agent-deck\bin`).
  Alle drei Schritte von `tools\umstellen.ps1` sind durch: Panel getauscht, Hooks und
  statusLine zeigen auf `deck-report.exe` / `deck-statusline.exe`, keine Python-Einträge mehr.
  Rückweg bleibt `tools\zurueck.ps1` (braucht die Python-Fassung aus `git show 2856494`).
- `tauri` ist in `main` gemergt, der Worktree `wt-deck-tauri` ist weg. Die Python-Fassung
  liegt nur noch in der Historie (letzter Stand `2856494`); ihre Reste aus dem Arbeitsordner
  (Build-Ausgaben, alte Laufzeitdateien) liegen in `%LOCALAPPDATA%\claude-agent-deck\python-reste`.
- VS Code lädt den Ring (`vscode_custom_css.imports`) jetzt aus dem App-Ordner, nicht mehr aus
  dem Repo.

## Was offen ist

1. **Automatisches `/compact` vor Ablauf des Prompt-Caches fehlt** in der Rust-Fassung. In
   Python: Regel in `deck/domain/prompt_cache.py` (`compact_due`, `user_busy`), Senden in
   `deck/ui/auto_compact.py`, Schalter `auto_compact`, Ende des Timers nach einem Compact in
   `deck/claude/hooks/statusline.py` (`compact_boundary`). Lesen mit `git show 2856494:<pfad>`.
2. **Im Betrieb prüfen**, was nur gegen Attrappen getestet ist: VS-Code-Fenster am Broker
   (Port 8765), Strg+< springt zum Wartenden, Klick auf eine Karte holt das richtige
   Terminal, ＋ legt einen Agenten an, Worktree-Aufräumen. Protokoll:
   `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`.
3. **Fokus-Rahmen abgleichen** mit dem letzten Python-Stand (`git show
   2856494:focus-frame/Frame.hlsl`, `Designs.cs` nach `crates/deck-win/src/frame.hlsl`,
   `frame_fx.rs`; Test `gpu_and_cpu_agree` muss grün bleiben).
4. Taskleiste: Abzeichen mit dem dringlichsten Status auf dem Agenten-Knopf
   (`ITaskbarList3::SetOverlayIcon`, wirkt auch auf fremde Fenster, gemessen 09.11.).
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
