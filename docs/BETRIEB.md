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

## Paket

`tools\package.ps1` (`-SkipBuild` nimmt `target\release`) legt
`dist\agent-deck-<version>-win-x64.zip` an, flach: die sechs Programme (`agent-deck`,
`deck-report`, `deck-statusline`, `deck-console`, `deck-watchdog`, `deck-setup`),
`install.ps1`, `uninstall.ps1`, `LICENSE`, `version.txt`. Dazu `dist\install.ps1` einzeln.
Ein Tag `v<version>` startet `.github/workflows/release.yml`: baut auf `windows-latest`,
prüft Tag gegen `tauri.conf.json` und hängt Zip und `install.ps1` an das Release, das
`install.ps1 -Download` über die GitHub-API (`/releases/latest`, ohne Anmeldung) findet.

## Installieren

`tools\install.ps1` läuft unter Windows PowerShell 5.1 und PowerShell 7 (UTF-8 mit BOM,
sonst zerlegt 5.1 die Umlaute). Quelle: neben dem Skript liegende Programme (entpacktes
Paket), `-Package <zip>`, `-Download` oder `-FromSource`/`-SkipBuild` (Vorgabe im
Checkout). Ablauf:

1. Voraussetzungen: Windows Terminal, WebView2-Laufzeit, `claude` (PATH oder
   `~\.local\bin\claude.exe`) sind Pflicht; PowerShell 7, `gh` (mit `gh auth status`),
   `acli`, OpenSSH-Client nur Hinweise. Jeder `winget`-Hinweis trägt `--source winget`:
   auf einem frischen Server 2025 scheitert die msstore-Quelle am Zertifikat
   (0x8a15005e).
2. Deck und Wächter beenden (Wächter zuerst), Programme nach
   `%LOCALAPPDATA%\claude-agent-deck\bin`, dazu `uninstall.ps1` und `version.txt`;
   Herkunftsmarke entfernen (`Unblock-File`), sonst fragt SmartScreen.
3. Nur wenn es `deck_settings.json` noch nicht gibt: Fragen nach Repos-Ordner, Jira-Kürzel,
   Autostart; daraus eine neue Datei mit `settings_version: 1`, `repos_root`,
   `jira_prefix` und allen Schaltern mit Wirkung nach außen auf `false`. Eine vorhandene
   Datei wird nie überschrieben, ein Update fragt nichts.
4. `deck-setup remove-vscode-extension`, nur wenn `~\.vscode\extensions\agent-deck-bridge*`
   da ist (Übergangsschritt, kann entfallen).
5. `deck-setup install --statusline keep|replace` und `check`, `deck-setup wt-profile`,
   `deck-setup shortcut` (beim ersten Mal, danach nur, wenn die Verknüpfung noch da ist),
   Autostart als Wert `AgentDeck` unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
6. Deck starten und prüfen, dass es läuft.

`tools\uninstall.ps1` (liegt nach der Installation in `bin`): Deck und Wächter beenden,
`deck-setup uninstall`, `shortcut --remove`, `wt-profile --remove`, Autostart weg, dann
nach Rückfrage (`-KeepData`, `-RemoveData`) den ganzen Ordner
`%LOCALAPPDATA%\claude-agent-deck`; sonst nur die Programme.

### deck-setup

| Befehl | Wirkung |
|---|---|
| `install --bin <ordner> [--statusline keep\|replace]` | sechs Hooks in Claudes `settings.json` (`CLAUDE_CONFIG_DIR`, sonst `~\.claude`). Eine fremde statusLine bleibt bei `keep` (Vorgabe); `replace` sichert sie vorher nach `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`. Zeigt die statusLine schon auf das Deck, ändert sich an ihr nichts. Vor jeder Änderung `settings.json.bak-<zeit>` daneben. |
| `check --bin <ordner>` | Befunde: fehlende Einträge, `cmd /c`, Pfade ins Leere |
| `uninstall` | unsere Einträge heraus, gesicherte statusLine zurück (nicht, wenn inzwischen eine andere eingetragen ist; dann bleibt die Sicherung liegen) |
| `shortcut [--bin <ordner>] [--remove]` | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk` auf `bin\agent-deck.exe`, Arbeitsordner `bin`, Symbol aus der Exe, `System.AppUserModel.ID` = `de.idm.agentdeck` |
| `wt-profile [--remove]` | Fragment `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\AgentDeck\agent-deck.json`: verstecktes Profil „Agent Deck“ (`closeOnExit: always`) und die Schemata `papier-ebenen`, `papier-ebenen-nacht`. Kein Fragment, wenn die `settings.json` des Terminals schon ein eigenes Profil „Agent Deck“ hat. |
| `remove-vscode-extension` | Eintrag und Ordner der früheren VS-Code-Extension weg |

Das Deck läuft aus `bin`: eine Änderung ist erst nach Build und Installation sichtbar.

Eine neue Installation startet ohne jede Automatik mit Wirkung nach außen (kein ssh,
kein winget, kein Tastatur-Hook, kein Shift+Tab, kein `/compact`, kein erzwungenes
Modell) und ohne Repos-Ordner: das Einstellungsfenster geht beim ersten Start auf. Eine
bestehende Installation behält beim ersten Start der neuen Fassung alles, wie es war
(Versionierung in `docs/DATEIEN.md`, „`deck_settings.json`“). Claude Codes Ordner kommt
überall aus `CLAUDE_CONFIG_DIR`, sonst `~/.claude` (`deck_core::paths::claude_dir`).

## Diagnose

Protokoll: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`. Erste Stellen je Fehlerbild:

| Fehlerbild | Im Log |
|---|---|
| Leiste weg, Neustart tut nichts | „Oberfläche hängt: …“ (sagt Hauptthread oder Webview), „Vorgänger-Deck … hing“ |
| Agentenfenster plötzlich zu | „… ohne das Deck beendet …“; schweigt das Log, im Ereignisprotokoll nachsehen (AppXDeploymentServer/Operational, „Application Hang pwsh.exe … was closed“) |

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
PowerShell und Windows Terminal selbst nach, wenn der Schalter `store_update` an ist
(`deck_services::store_update`: eine Minute
nach dem Start, höchstens alle 12 h, nur ohne lebenden Agenten und nur, wenn das
Programm gerade nicht läuft).

Stirbt eine Pane-Shell, ohne dass das Deck sie schloss, steht das im Log, und war seit
15 s niemand an Maus oder Tastatur, setzt das Deck die Session mit `claude --resume` fort,
mit einem Anstoß, wenn sie gerade arbeitete (`deck_core::crash_resume`, App:
`crash_resume.rs`).

## Im Betrieb noch zu prüfen

Nur gegen Attrappen getestet:

- Strg+< springt zum Wartenden, Klick auf eine Karte holt das richtige Terminal, ＋ legt
  einen Agenten an.
- Worktree-Aufräumen.
- `deck-setup remove-vscode-extension` auf dem echten Rechner.
- Installation aus dem Paket auf einem frischen Rechner (Server 2025, nur PowerShell 5.1):
  Windows-Suche findet „Agent Deck“, das Terminal lädt das Fragment.
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
