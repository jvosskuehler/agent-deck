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
`install.ps1`, `uninstall.ps1`, `LICENSE`, `version.txt`, dazu der Ordner
`resources\papier` (Schrift samt `OFL.txt`, Claude-Theme, Einstellungen der Deck-Agenten,
Shader). Dazu `dist\install.ps1` einzeln.
Ein Tag `v<version>` startet `.github/workflows/release.yml`: baut auf `windows-latest`,
prüft Tag gegen `tauri.conf.json` und hängt Zip und `install.ps1` an das Release, das
`install.ps1 -Download` über die GitHub-API (`/releases/latest`, ohne Anmeldung) findet.

## Installieren

`tools\install.ps1` läuft unter Windows PowerShell 5.1 und PowerShell 7 (UTF-8 mit BOM,
sonst zerlegt 5.1 die Umlaute), ohne Administratorrechte. Quelle: neben dem Skript
liegende Programme (entpacktes Paket), `-Package <zip>`, `-Download` oder
`-FromSource`/`-SkipBuild` (Vorgabe im Checkout). Den Papier-Look findet es in
`resources\papier` neben sich (Paket) bzw. im Checkout.

Die Texte sind für Menschen, die nichts über Claude Code wissen wollen: je Frage ein
Satz, keine Fachwörter (Infozeile statt statusLine, Claude-Einstellungen statt
settings.json), Fortschritt als ✓-Zeilen, am Ende höchstens fünf Zeilen. Was
`deck-setup` meldet, zeigt der Installer nur bei `warn`/`fail`; alles andere mit
`-Verbose`. Die Zusammenfassung baut er aus den Merkern (`## statusline`,
`## statusline-chained`, `## default <schlüssel>=<wert>`), nicht aus Meldungstexten.
Ablauf:

1. Voraussetzungen: Windows Terminal, WebView2-Laufzeit, `claude` (PATH oder
   `~\.local\bin\claude.exe`) sind Pflicht; `gh` nur ein Hinweis. `gh auth status` läuft
   über `cmd /d /c "… >nul 2>&1"`: direkt aufgerufen schreibt 5.1 sein stderr als roten
   NativeCommandError, auch ins Transcript. Jeder `winget`-Hinweis trägt
   `--source winget`: auf einem frischen Server 2025 scheitert die msstore-Quelle am
   Zertifikat (0x8a15005e).
2. Nur wenn es `deck_settings.json` noch nicht gibt: Fragen nach Repos-Ordner, Jira-Kürzel,
   Reviewern, Autostart; daraus eine neue Datei mit `settings_version: 1`, `repos_root`,
   `jira_prefix` und allen Schaltern mit Wirkung nach außen auf `false`. Eine vorhandene
   Datei wird nie überschrieben, ein Update fragt nichts.
3. Deck und Wächter beenden (Wächter zuerst), Programme nach
   `%LOCALAPPDATA%\claude-agent-deck\bin`, dazu `uninstall.ps1` und `version.txt`;
   Herkunftsmarke entfernen (`Unblock-File`), sonst fragt SmartScreen.
4. `deck-setup remove-vscode-extension`, nur wenn `~\.vscode\extensions\agent-deck-bridge*`
   da ist (Übergangsschritt, kann entfallen).
5. `deck-setup install` und `check`, `look --from <papier>`, `wt-profile` (nach `look`,
   weil das Profil den Shader nennt), `shortcut` (beim ersten Mal, danach nur, wenn die
   Verknüpfung noch da ist), `autostart` bzw. `autostart --remove`, `apps-entry`. Gleich
   nach dem Schreiben der Einstellungen `migrate-darkmode`; steht dazu nur dann eine
   Zeile da, wenn die alte Aufgabe gelöscht wurde.
6. Deck starten und prüfen, dass es läuft.

`-StatusLine keep` (nicht in der Hilfe) lässt eine fremde Infozeile allein stehen.

`tools\uninstall.ps1` (liegt nach der Installation in `bin`, gestartet meist über
„Apps“): liegt es in `bin`, kopiert es sich nach `%TEMP%` und läuft dort weiter (mit
`-Relaunched`), damit `bin` gelöscht werden kann; hat es ein eigenes Konsolenfenster
(`GetConsoleProcessList` = 1, also aus „Apps“ gestartet), wartet es am Ende auf Enter.
Dann Deck und Wächter beenden, `deck-setup uninstall`, nach Rückfrage (`-KeepData`,
`-RemoveData`; `-Quiet` fragt nichts und behält die Daten) den ganzen Ordner
`%LOCALAPPDATA%\claude-agent-deck`, sonst nur `bin`.

### deck-setup

Jeder Befehl verzeichnet, was er angelegt hat, in
`%LOCALAPPDATA%\claude-agent-deck\installed.json` (`deck_services::installed`, Format in
`docs/DATEIEN.md`). Was Registratur und Schriften der Sitzung anfasst, steckt hinter
`deck_services::user_system::UserSystem`; Tests nehmen `MemSystem`, die echte Fassung
schreibt in Tests nur unter `HKCU\Software\AgentDeckTest`.

| Befehl | Wirkung |
|---|---|
| `install --bin <ordner> [--statusline replace\|keep]` | sechs Hooks und die Infozeile (`statusLine`) in Claudes `settings.json` (`CLAUDE_CONFIG_DIR`, sonst `~\.claude`). Eine fremde Infozeile kommt vorher nach `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`; `deck-statusline` ruft sie weiter auf und hängt ihre Ausgabe an (`deck_hooks::chain`: Git Bash wie Claude Code, sonst `cmd`, höchstens 1 s, Fehler fallen still weg). `keep` lässt sie allein stehen. Fehlen Modus oder Effort: Bypass und high, verzeichnet. Vor jeder Änderung `settings.json.bak-<zeit>` daneben. |
| `check --bin <ordner>` | Befunde: fehlende Einträge, `cmd /c`, Pfade ins Leere |
| `look --from <ordner> [--remove]` | Papier-Look (`deck_services::look`): IBM Plex Mono nach `%LOCALAPPDATA%\Microsoft\Windows\Fonts`, angemeldet unter `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts` und in der Sitzung geladen (`AddFontResource`, `WM_FONTCHANGE`); übersprungen, wenn es die Datei oder den Namen beim Benutzer oder im System schon gibt. `themes\papier.json` und `agent-deck-wt.settings.json` in Claudes Ordner, `wt-faint-hell.hlsl` im App-Ordner. Eine vorhandene Datei, die nicht vom Deck stammt oder geändert wurde, bleibt. |
| `shortcut [--bin <ordner>] [--remove]` | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk` auf `bin\agent-deck.exe`, Arbeitsordner `bin`, Symbol aus der Exe, `System.AppUserModel.ID` = `de.idm.agentdeck` |
| `wt-profile [--remove]` | Fragment `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\AgentDeck\agent-deck.json`: verstecktes Profil „Agent Deck“ genau wie das des Autors (`closeOnExit: always`, `pwsh.exe -NoLogo` bzw. `powershell.exe -NoLogo`, IBM Plex Mono 10.5, Shader mit absolutem Pfad, wenn er da ist) und die Schemata `papier-ebenen`, `papier-ebenen-nacht`. Kein Fragment, wenn die `settings.json` des Terminals schon ein eigenes Profil „Agent Deck“ hat. |
| `autostart [--bin <ordner>] [--remove]` | Wert `AgentDeck` unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| `apps-entry --bin <ordner> --version <v> [--remove]` | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentDeck`: Name, Version, Herausgeber „Agent Deck“, Symbol, `UninstallString` (`powershell.exe -NoProfile -ExecutionPolicy Bypass -File "<bin>\uninstall.ps1"`), `QuietUninstallString` (dasselbe mit `-Quiet -KeepData`), `NoModify`, `NoRepair`, `EstimatedSize` |
| `uninstall` | alles heraus (`deck_services::teardown`): Hooks und Infozeile, die gesicherte Infozeile zurück (nicht, wenn inzwischen eine andere eingetragen ist; dann bleibt die Sicherung liegen), Vorgaben nur mit unverändertem Wert, Schriften (eine geladene wird beim nächsten Anmelden über `RunOnce` gelöscht), Theme- und Einstellungsdateien nur mit unverändertem Inhalt, Verknüpfung, Fragment, Autostart, Eintrag unter „Apps“. Ohne `installed.json` nur, was am Namen dem Deck gehört. |
| `remove-vscode-extension` | Eintrag und Ordner der früheren VS-Code-Extension weg |
| `migrate-darkmode` | Löscht die geplante Aufgabe `DarkModeZeitplan` (`schtasks /Delete /TN DarkModeZeitplan /F`), wenn es sie gibt und Hell/Dunkel auf Sonnenstand steht (bei einer Datei ohne `settings_version` gilt schon die Migration, also `sun`). Der Ordner `%LOCALAPPDATA%\DarkModeZeitplan` bleibt. Sonst keine Zeile. `deck_services::darkmode_task`, hinter dem Trait `Scheduler`. |

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
| PR nicht gemergt, Agent nicht gestartet, Jira nicht nachgezogen | `state\dash-feed.log` (je Merge, Agent, Ticket eine Zeile mit Grund); im Feed steht unter „Automatik“, worauf jeder PR wartet |
| PR bleibt „blocked“, obwohl alles erledigt und freigegeben ist | `state\dash-feed.log` „überholtes Review von claude verworfen“ bzw. „Review verwerfen gescheitert“: ein altes „Änderungen gewünscht“ von Claude verwirft das Deck nur unter den Bedingungen in `deck_core::dash::stale_review`; ein Mensch wird nie verworfen |

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

## Hell/Dunkel

Einstellung `appearance` (`docs/DATEIEN.md`). Im Log steht jede Änderung der Einstellung
(`Hell/Dunkel: sun (Hamburg)`) und bei Sonnenstand jedes Umschalten von Windows mit Ziel
und nächstem Wechsel in UTC (`Hell/Dunkel nach Sonnenstand (Hamburg): Windows auf dunkel,
nächster Wechsel 2026-11-18 06:50:12Z`), ein Fehler beim ersten Mal und bei der Erholung.

- **Sonnenstand:** NOAA-Näherung aus dem früheren Skript (`deck_core::sun`), genau auf
  2 min um die Sonnenwenden, bis 3 min dazwischen. Geschaltet werden
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
  `AppsUseLightTheme` und `SystemUsesLightTheme` (nur wenn einer abweicht), dann
  `WM_SETTINGCHANGE` „ImmersiveColorSet“ an alle Fenster (5 s, hängende übersprungen).
  Nachgesehen wird beim Start, nach dem Aufwachen (`PowerRegisterSuspendResumeNotification`),
  bei einer Änderung der Einstellung und vom Takt kurz nach dem nächsten Wechsel
  (höchstens alle 10 min). Der Takt schaltet nur, wenn die Sonne seit dem letzten Blick
  auf- oder untergegangen ist: wer von Hand umstellt, behält das bis zum nächsten Wechsel.
- **Fest hell oder dunkel:** Windows bleibt unberührt. Neue Agentenfenster gehen mit
  `--colorScheme` auf. Offene behalten ihr Schema bis zum nächsten Start: Windows
  Terminal stellt das Schema eines laufenden Fensters von außen nicht um, und das Profil
  im Fragment (bzw. in der `settings.json` des Besitzers) für alle Fenster umzuschreiben
  hieße, fremde Dateien bei jedem Umschalten anzufassen. Das Schema muss so heißen wie im
  Profil (`papier-ebenen`, `papier-ebenen-nacht`); ein eigenes Profil mit anderen Namen
  bekommt dann das Paar des Profils.
- **Deinstallieren:** nichts zurückzustellen, Windows bleibt im letzten Stand.

## Im Betrieb noch zu prüfen

Nur gegen Attrappen getestet:

- Hell/Dunkel echt: Sonnenstand schaltet Windows um (und nach dem Aufwachen), die alte
  Aufgabe ist nach `install.ps1` weg; fest dunkel bei hellem Windows: Panel, Dialoge,
  Rahmen, Glow und ein neuer Agent dunkel, offene Agenten erst nach Neustart.

- PR-Feed-Mechanik echt (17.11.2026 nur gegen Attrappen und mit echten Lese-Abfragen):
  Merge per `gh pr merge --merge`, Branch löschen, Reviewer/Assignee nachtragen, Jira
  per `acli`, Hintergrund-Agent `claude -p` samt worktree anlegen, Antwort senden und
  Thread auflösen, Kommentar-Fenster über dem vordersten Agenten.

- Strg+< springt zum Wartenden, Klick auf eine Karte holt das richtige Terminal, ＋ legt
  einen Agenten an.
- Worktree-Aufräumen.
- `deck-setup remove-vscode-extension` auf dem echten Rechner.
- Installation aus dem Paket auf einem frischen Rechner (Server 2025, nur PowerShell 5.1):
  Windows-Suche findet „Agent Deck“, das Terminal lädt das Fragment.
- Papier-Look und Deinstallation auf einem frischen Rechner (17.11.2026 nur gegen
  Attrappen): Schrift ohne Neuanmeldung im Terminal, Shader greift, Theme `custom:papier`
  in den Deck-Agenten, die frühere Infozeile hinter der des Decks (Git Bash), „Apps“ ›
  Deinstallieren aus dem eigenen Fenster samt Rückfrage und Enter am Ende, eine geladene
  Schrift verschwindet nach dem nächsten Anmelden.
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
