# Bauen, Installieren, Betrieb

## Bauen

Neben `agent-deck.exe` müssen `deck-console`, `deck-report`, `deck-statusline` und
`deck-watchdog` liegen. Sie gehören zum selben Workspace, landen also im selben
`target/<profil>/`; `app/scripts/helpers.mjs` baut sie (`pnpm helpers` bzw.
`pnpm helpers:release`) und legt Kopien mit Zielsystem-Anhängsel nach
`app/src-tauri/binaries/` (nicht eingecheckt). `pnpm tauri dev` und `pnpm tauri build`
rufen das selbst auf (`beforeDevCommand`/`beforeBuildCommand`). `pnpm tauri build` baut nur
die Exe; das Installationspaket macht `pnpm bundle` (zusätzlich `tauri.bundle.conf.json`
mit `externalBin`). `wt_agent.ps1` und `agent-taskbar.ico` (in `resources/`) sind in die
Exe eingebaut und werden beim Start in den App-Ordner geschrieben.

## Installieren

`pwsh tools\install.ps1` (`-SkipBuild` nimmt das schon gebaute Release): bauen, Deck und
Wächter beenden, alle Programme samt `deck-setup` nach
`%LOCALAPPDATA%\claude-agent-deck\bin` kopieren, `deck-setup remove-vscode-extension`
(nimmt die frühere VS-Code-Extension aus `~/.vscode/extensions`, nur ihren Eintrag und
Ordner; Übergangsschritt, kann später entfallen), `deck-setup install` und `check` (Hooks
in `~/.claude/settings.json`), Deck starten und prüfen, dass es läuft.
`deck-setup uninstall` nimmt die Hooks wieder heraus.

Das Deck läuft aus `bin`: eine Änderung ist erst nach Build und Installation sichtbar.

## Diagnose

Protokoll: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`. Erste Stellen je Fehlerbild:

| Fehlerbild | Im Log |
|---|---|
| Leiste weg, Neustart tut nichts | „Oberfläche hängt: …“ (sagt Hauptthread oder Webview), „Vorgänger-Deck … hing“ |
| Agentenfenster plötzlich zu | „… ohne das Deck beendet …“; schweigt das Log, im Ereignisprotokoll nachsehen (AppXDeploymentServer/Operational, „Application Hang pwsh.exe … was closed“) |
| PR nicht gemergt, Agent nicht gestartet, Jira nicht nachgezogen | `state\dash-feed.log` (je Merge, Agent, Ticket eine Zeile mit Grund); im Feed steht unter „Automatik“, worauf jeder PR wartet |

### Hängendes Deck (11.11.2026)

Ein Deck, das lebt, aber nichts zeigt, blockiert jeden Neustart: die
Einzelinstanz-Sperre reicht das Signal an die tote Instanz weiter. Der Herzschlag merkt
das nicht, ihn schreibt der Poll-Thread. Darum `ui_watch` (Hauptthread und Seite des
sichtbaren Fensters, `ui-alive.json`, Selbst-Neustart nach 60 s) und `instance_guard`
(ein neuer Start beendet eine hängende Instanz).

### Store-Update beendet alle Agenten (15.11.2026, 04:28)

Ein Store-Update von PowerShell beendet jede `pwsh.exe` und damit jeden Agenten.
Seitdem: automatische Store-Updates per Richtlinie aus
(`HKLM\SOFTWARE\Policies\Microsoft\WindowsStore`, `AutoDownload` = 2); das Deck holt
PowerShell und Windows Terminal selbst nach (`deck_services::store_update`: eine Minute
nach dem Start, höchstens alle 12 h, nur ohne lebenden Agenten und nur, wenn das
Programm gerade nicht läuft).

Stirbt eine Pane-Shell, ohne dass das Deck sie schloss, steht das im Log, und war seit
15 s niemand an Maus oder Tastatur, setzt das Deck die Session mit `claude --resume` fort,
mit einem Anstoß, wenn sie gerade arbeitete (`deck_core::crash_resume`, App:
`crash_resume.rs`).

## Im Betrieb noch zu prüfen

Nur gegen Attrappen getestet:

- PR-Feed-Mechanik echt (17.11.2026 nur gegen Attrappen und mit echten Lese-Abfragen):
  Merge per `gh pr merge --merge`, Branch löschen, Reviewer/Assignee nachtragen, Jira
  per `acli`, Hintergrund-Agent `claude -p` samt worktree anlegen, Antwort senden und
  Thread auflösen, Kommentar-Fenster über dem vordersten Agenten.

- Strg+< springt zum Wartenden, Klick auf eine Karte holt das richtige Terminal, ＋ legt
  einen Agenten an.
- Worktree-Aufräumen.
- `deck-setup remove-vscode-extension` auf dem echten Rechner.
- `crash_resume` bei einem echten Absturz.
- Ob `live.json` `cwd` bei Agenten im `wt/`-Ordner den worktree trägt.
- „Ohne Fenster“ echt (startet node/pwsh/cmd versteckt):
  `cargo test -p deck-services --test orphans_real -- --ignored --nocapture`.

## Offen

- **Taskleisten-Abzeichen:** der dringlichste Status als Overlay auf dem Agenten-Knopf
  (`ITaskbarList3::SetOverlayIcon`, wirkt auch auf fremde Fenster, gemessen 09.11.2026).
- **Makropad:** 6 Tasten mit Drehregler und ARGB je Taste (Taste = Agent, Licht = Status,
  Druck = zum Agenten springen, Halten = Aufnahme mit Claude `/voice`). Die Tasten
  (F13 bis F21 ohne Modifier) kommen schon an; ARGB-Licht je Status fehlt.
