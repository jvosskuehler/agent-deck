# Bauen, Installieren, Betrieb

## Bauen

Neben `agent-deck.exe` müssen `deck-console`, `deck-report`, `deck-statusline` und
`deck-wsl` liegen. Sie gehören zum selben Workspace, landen also im selben
`target/<profil>/`; `app/scripts/helpers.mjs` baut sie (`pnpm helpers` bzw.
`pnpm helpers:release`) und legt Kopien mit Zielsystem-Anhängsel nach
`app/src-tauri/binaries/` (nicht eingecheckt). `pnpm tauri dev` und `pnpm tauri build`
rufen das selbst auf (`beforeDevCommand`/`beforeBuildCommand`). `pnpm tauri build` baut nur
die Exe; das Installationspaket macht `pnpm bundle` (zusätzlich `tauri.bundle.conf.json`
mit `externalBin`). `wt_agent.ps1`, `wt_agent.sh` (immer mit LF) und `agent-taskbar.ico` (in `resources/`)
sind in die Exe eingebaut und werden beim Start in den App-Ordner geschrieben.

## Paket

`tools\package.ps1` (`-SkipBuild` nimmt `target\release`) legt
`dist\agent-deck-<version>-win-x64.zip` an, flach: die sechs Programme (`agent-deck`,
`deck-report`, `deck-statusline`, `deck-console`, `deck-setup`, `deck-wsl`), unter
`linux\` `deck-report` und `deck-statusline` als statische
Linux-Programme (`x86_64-unknown-linux-musl`, gelinkt mit `rust-lld` laut
`.cargo\config.toml`, also ohne WSL, gcc oder zig; das Skript ruft
`rustup target add x86_64-unknown-linux-musl` selbst), `install.ps1`, `install-wsl.ps1`, `uninstall.ps1`, `LICENSE`, `version.txt`, dazu der Ordner
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
   `~\.local\bin\claude.exe`) sind Pflicht. Jeder `winget`-Hinweis trägt
   `--source winget`: auf einem frischen Server 2025 scheitert die msstore-Quelle am
   Zertifikat (0x8a15005e).
2. Nur wenn es `deck_settings.json` noch nicht gibt: Fragen nach Repos-Ordner und
   Autostart; daraus eine neue Datei mit `settings_version: 1`, `repos_root`,
   `auto_permission_mode: false` und `auto_compact: true`. Jede Frage ist eine eigene
   Funktion (`Resolve-ReposRoot`, `Resolve-Autostart`), gesammelt in
   `Get-FirstRunAnswers`; eine neue Frage kommt dort dazu. Eine vorhandene Datei wird nie
   überschrieben, ein Update fragt nichts.
3. Deck beenden (ein alter, noch laufender `deck-watchdog` zuerst, sonst startet er es
   wieder), Programme nach `%LOCALAPPDATA%\claude-agent-deck\bin`, dazu `uninstall.ps1`
   und `version.txt`; eine alte `deck-watchdog.exe` aus `bin` löschen; Herkunftsmarke
   entfernen (`Unblock-File`), sonst fragt SmartScreen.
4. `deck-setup install` und `check`, `look --from <papier>`, `wt-profile` (nach `look`,
   weil das Profil den Shader nennt), `shortcut` (beim ersten Mal, danach nur, wenn die
   Verknüpfung noch da ist), `autostart` bzw. `autostart --remove`, `apps-entry`.
5. Deck starten und prüfen, dass es läuft.

`-StatusLine keep` (nicht in der Hilfe) lässt eine fremde Infozeile allein stehen.

`tools\uninstall.ps1` (liegt nach der Installation in `bin`, gestartet meist über
„Apps“): liegt es in `bin`, kopiert es sich nach `%TEMP%` und läuft dort weiter (mit
`-Relaunched`), damit `bin` gelöscht werden kann; hat es ein eigenes Konsolenfenster
(`GetConsoleProcessList` = 1, also aus „Apps“ gestartet), wartet es am Ende auf Enter.
Dann das Deck beenden (samt einem alten `deck-watchdog`), `deck-setup uninstall`, nach
Rückfrage (`-KeepData`, `-RemoveData`; `-Quiet` fragt nichts und behält die Daten) den
ganzen Ordner `%LOCALAPPDATA%\claude-agent-deck`, sonst nur `bin`.

### deck-setup

Jeder Befehl verzeichnet, was er angelegt hat, in
`%LOCALAPPDATA%\claude-agent-deck\installed.json` (`deck_services::installed`, Format in
`docs/DATEIEN.md`). Was Registratur und Schriften der Sitzung anfasst, steckt hinter
`deck_services::user_system::UserSystem`; Tests nehmen `MemSystem`, die echte Fassung
schreibt in Tests nur unter `HKCU\Software\AgentDeckTest`.

| Befehl | Wirkung |
|---|---|
| `install --bin <ordner> --wsl <distro>` | dasselbe in Claudes `settings.json` der WSL-Distro (`deck_services::install_wsl`), Programme über ihren Linux-Pfad, Sicherung einer fremden Infozeile in `statusline.before.wsl.json`, verzeichnet unter `wsl` in `installed.json` |
| `wsl-distros`, `wsl-probe --wsl <distro>` | für den Installer: Distros als JSON; Home, Einhängepunkt und Pfad von `claude` einer Distro |
| `install --bin <ordner> [--statusline replace\|keep]` | sieben Hooks (`deck_core::claude::hook_setup::HOOKS`, mit `PermissionRequest`; ein Update trägt ihn nach, `uninstall` nimmt ihn mit heraus) und die Infozeile (`statusLine`) in Claudes `settings.json` (`CLAUDE_CONFIG_DIR`, sonst `~\.claude`). Eine fremde Infozeile kommt vorher nach `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`; `deck-statusline` ruft sie weiter auf und hängt ihre Ausgabe an (`deck_hooks::chain`: Git Bash wie Claude Code, sonst `cmd`, höchstens 1 s, Fehler fallen still weg). `keep` lässt sie allein stehen. Fehlen Modus oder Effort: Bypass und high, verzeichnet. Vor jeder Änderung `settings.json.bak-<zeit>` daneben. |
| `check --bin <ordner>` | Befunde: fehlende Einträge, `cmd /c`, Pfade ins Leere |
| `look --from <ordner> [--remove]` | Papier-Look (`deck_services::look`): IBM Plex Mono nach `%LOCALAPPDATA%\Microsoft\Windows\Fonts`, angemeldet unter `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts` und in der Sitzung geladen (`AddFontResource`, `WM_FONTCHANGE`); übersprungen, wenn es die Datei oder den Namen beim Benutzer oder im System schon gibt. `themes\papier.json` und `agent-deck-wt.settings.json` in Claudes Ordner, `wt-faint-hell.hlsl` im App-Ordner. Eine vorhandene Datei, die nicht vom Deck stammt oder geändert wurde, bleibt. |
| `shortcut [--bin <ordner>] [--remove]` | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk` auf `bin\agent-deck.exe`, Arbeitsordner `bin`, Symbol aus der Exe, `System.AppUserModel.ID` = `de.idm.agentdeck` |
| `wt-profile [--remove]` | Fragment `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\AgentDeck\agent-deck.json`: verstecktes Profil „Agent Deck“ genau wie das des Autors (`closeOnExit: always`, `pwsh.exe -NoLogo` bzw. `powershell.exe -NoLogo`, IBM Plex Mono 10.5, Shader mit absolutem Pfad, wenn er da ist) und die Schemata `papier-ebenen`, `papier-ebenen-nacht`. Kein Fragment, wenn die `settings.json` des Terminals schon ein eigenes Profil „Agent Deck“ hat. |
| `autostart [--bin <ordner>] [--remove]` | Wert `AgentDeck` unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| `apps-entry --bin <ordner> --version <v> [--remove]` | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentDeck`: Name, Version, Herausgeber „Agent Deck“, Symbol, `UninstallString` (`powershell.exe -NoProfile -ExecutionPolicy Bypass -File "<bin>\uninstall.ps1"`), `QuietUninstallString` (dasselbe mit `-Quiet -KeepData`), `NoModify`, `NoRepair`, `EstimatedSize` |
| `uninstall` | alles heraus (`deck_services::teardown`): Hooks und Infozeile, die gesicherte Infozeile zurück (nicht, wenn inzwischen eine andere eingetragen ist; dann bleibt die Sicherung liegen), Vorgaben nur mit unverändertem Wert, Schriften (eine geladene wird beim nächsten Anmelden über `RunOnce` gelöscht), Theme- und Einstellungsdateien nur mit unverändertem Inhalt, Verknüpfung, Fragment, Autostart, Eintrag unter „Apps“. Ohne `installed.json` nur, was am Namen dem Deck gehört. |

Das Deck läuft aus `bin`: eine Änderung ist erst nach Build und Installation sichtbar.

Eine neue Installation startet ohne Automatik mit Wirkung nach außen (kein Shift+Tab in
den Auto-Modus, kein erzwungenes Modell; nur Auto-Compact ist an, es schreibt allein in
den eigenen wartenden Agenten) und ohne Repos-Ordner: das Einstellungsfenster geht beim ersten Start auf. Eine
bestehende Installation behält beim ersten Start der neuen Fassung alles, wie es war
(Versionierung in `docs/DATEIEN.md`, „`deck_settings.json`“). Claude Codes Ordner kommt
überall aus `CLAUDE_CONFIG_DIR`, sonst `~/.claude` (`deck_core::paths::claude_dir`).

## Agenten in WSL

Die Team-Ausgabe: Claude Code und Repos in einer WSL-Distro, nur das Deck unter Windows.
Ein neuer `install.ps1`-Lauf richtet das ein (`-AgentEnv windows` für den bisherigen
Weg; ein Update behält, was in `deck_settings.json` steht). Die WSL-Teile des Installers
stehen in `tools/install-wsl.ps1`:

1. Distro wählen (`deck-setup wsl-distros`, Vorschlag: Standard-Distro; ohne Distro ein
   Hinweis auf `wsl --install -d Ubuntu`), Home und Einhängepunkt (`deck-setup wsl-probe`).
2. `claude` in der Distro? (`bash -lic 'command -v claude'`, also wie im Startskript.)
   Sonst die Installation mit dem offiziellen Skript anbieten
   (`curl -fsSL https://claude.ai/install.sh | bash`).
3. Repos-Ordner als Linux-Pfad (Vorschlag `~/repos`, `~/src` …).
4. `deck-setup install --wsl <Distro>`: kopiert die Linux-Fassungen der Hooks aus
   `bin\linux` nach `~/.local/share/agent-deck/bin` der Distro (`install -m 755`) und trägt
   Hooks und Infozeile in `~/.claude/settings.json` ein, zuerst das Linux-Programm, die
   `.exe` nur, wenn es fehlt:
   `"/home/<name>/.local/share/agent-deck/bin/deck-report" idle 2>/dev/null || "/mnt/c/…/bin/deck-report.exe" idle || exit 0`
   (`sh` startet die `.exe` nur bei Exit 127, stdin ist dann noch unberührt),
   Vorgaben (Bypass, Effort high) wie unter Windows. `look --wsl` legt Theme und
   Agenten-Einstellungen in Claudes Ordner der Distro, Schrift und Shader unter Windows.

**Start.** `wt.exe … new-tab -p "Agent Deck" … deck-wsl.exe <spec>`; `deck-wsl.exe`
schreibt `<slot>.wtpid` mit seiner PID, setzt die Umgebung (`docs/DATEIEN.md`) und startet
`wsl.exe -d <Distro> --cd <Linux-Ordner> -e bash -li <…>/wt_agent.sh <claude-Argumente>`.
`bash -li` lädt `.profile`/`.bashrc` (claude unter `~/.local/bin` oder über nvm). Modell,
Effort, Modus, `--resume` und der Anstoß beim Fortsetzen gehen als Argumente durch.
Endet claude, bleibt eine Login-Shell im Fenster (wie `-NoExit`).

**Was das Deck in der Distro liest:** Claudes Einstellungen (Modus, Effort, Sprache im
Einstellungsfenster), Transcripts (Zusammenfassung, Titel, worktree-Erkennung),
`.credentials.json` und `.claude.json` (Usage) über `\\wsl.localhost\<Distro>\<home>\…`.
Die Zusammenfassung läuft mit dem `claude` der Distro (`summary::WslClaude`). Wer als
Erweiterung git in den Repos eines WSL-Agenten aufruft, tut das in der Distro
(`wsl_cli::WslCli`), nicht mit Windows-git über die Freigabe: dort ist git langsam, sieht
andere Dateirechte und Zeilenenden, und ein worktree, den der Agent in Linux angelegt
hat, nennt Linux-Pfade.

**Gemessen (24.11.2026, Titan-VM, Server 2025, WSL 3.0.5, Ubuntu 24.04, je 20 Läufe):**

| Aufruf aus WSL | Mittel | Min |
|---|---|---|
| `deck-report.exe` über Interop | 110 ms | 90 |
| `deck-report` Linux, aus `~` | 73 ms | 51 |
| `deck-report` Linux, von `/mnt/c` gestartet | 116 ms | 89 |
| `deck-statusline.exe` über Interop | 121 ms | 101 |
| `deck-statusline` Linux, aus `~` | 88 ms | 65 |
| Linux-Programm ohne Slot (nur Start) | 17 ms | 9 |
| Linux-Programm, Zustand auf ext4 statt `/mnt/c` | 15 ms | 10 |

Der Rest sind die Dateizugriffe auf `/mnt/c` (drvfs über 9P, je Zugriff 10 bis 20 ms;
`deck-report` liest die alte Zustandsdatei, schreibt `.tmp` und benennt um). Schneller
ginge es nur mit dem Zustandsordner im Linux-Dateisystem; das hieße, Vertrag 1 zu ändern
(das Deck läse dann über `\\wsl.localhost`). Atomares Ersetzen per Umbenennen klappt auf
drvfs.
`\\wsl.localhost` ist nur in der angemeldeten Sitzung erreichbar, nicht aus einer
SSH-Sitzung („The network name cannot be found“): Proben über SSH lesen die Distro mit
`wsl -e cat`.

**Update bei laufenden WSL-Agenten:** `deck-wsl.exe` steht im Fenster jedes Agenten und
ist damit belegt. `install.ps1` benennt eine belegte Exe in `bin` um
(`deck-wsl.exe.old-<ticks>`) und legt die neue daneben; laufende Agenten behalten die alte,
neue nehmen die neue. Der nächste Lauf räumt Reste weg, die niemand mehr benutzt.

**Was nicht geht:** „Läuft noch ohne Fenster“ sieht nur Windows-Prozesse; Dev-Server, die
ein WSL-Agent startet, tauchen dort nicht auf.

## Diagnose

Protokoll: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`. Erste Stellen je Fehlerbild:

| Fehlerbild | Im Log |
|---|---|
| Leiste weg, Neustart tut nichts | „Oberfläche hängt: …“ (sagt Hauptthread oder Webview), „Vorgänger-Deck … hing“ |
| Agentenfenster plötzlich zu | „… ohne das Deck beendet …“; schweigt das Log, im Ereignisprotokoll nachsehen (AppXDeploymentServer/Operational, „Application Hang pwsh.exe … was closed“) |
| Kachel steht auf „Rückfrage“, obwohl der Agent an der Eingabezeile steht | „Rückfrage A2 abgelehnt (ohne Hook, aus dem Transcript)“ fehlt: dann stand im Transcript nach der Rückfrage weder Ablehnung noch Abbruch noch Compact, oder die Meldung trug noch kein `transcript` (Hook einer älteren Fassung). Gemessen 24.11.2026 (2.1.331, WSL): Rückfrage 0,2 s nach dem Werkzeugaufruf (`PermissionRequest`, vorher 6,2 s über `Notification`), Ablehnung 0,2 s nach Esc im Transcript |
| Agent ging auf oder zu | „Agent A2: wird geöffnet“, „Agent A2 läuft (pid …, 1.8 s nach dem Öffnen)“, „Agent A2: wird geschlossen“, „Agent A2 beendet (vom Deck geschlossen)“, beim Start „Agenten beim Start: …“; ein Ende ohne das Deck steht in der Zeile von `crash_resume` |
| Neuer Agent braucht wieder Sekunden, Fenster blitzt beim Nichtstun auf (nur mit einer Erweiterung, die den Reserve-Agenten will) | „Reserve-Agent …“: wird angelegt / bereit nach / übernommen / beendet: Grund; „nicht angelegt, es war nicht mehr ruhig“ und „Eingabe während des Anlegens“ = der Schutz vor verlorenen Tasten hat gegriffen; „unfertig vom letzten Lauf“ = das vorige Deck endete mitten im Anlegen (Update, Absturz), seine halbe Reserve wird beendet; „zeigte sich wieder“ = Windows Terminal hat das versteckte Fenster nochmal gezeigt (`app/src-tauri/src/wt_reserve.rs`) |

### Hängendes Deck (11.11.2026)

Ein Deck, das lebt, aber nichts zeigt, blockiert jeden Neustart: die
Einzelinstanz-Sperre reicht das Signal an die tote Instanz weiter. Der Herzschlag merkt
das nicht, ihn schreibt der Poll-Thread. Darum `ui_watch` (Hauptthread und Seite des
sichtbaren Fensters, `ui-alive.json`, Selbst-Neustart nach 60 s) und `instance_guard`
(ein neuer Start beendet eine hängende Instanz).

### Store-Update beendet alle Agenten (15.11.2026, 04:28)

Ein Store-Update von PowerShell beendet jede `pwsh.exe` und damit jeden Agenten. Abhilfe
am Rechner: automatische Store-Updates per Richtlinie aus
(`HKLM\SOFTWARE\Policies\Microsoft\WindowsStore`, `AutoDownload` = 2). Diese Ausgabe
zieht PowerShell und Windows Terminal nicht selbst nach; das liegt in einer eigenen
Ausgabe, angebunden über `extension.rs`.

Stirbt eine Pane-Shell, ohne dass das Deck sie schloss, steht das im Log, und war seit
15 s niemand an Maus oder Tastatur, setzt das Deck die Session mit `claude --resume` fort,
mit einem Anstoß, wenn sie gerade arbeitete (`deck_core::crash_resume`, App:
`crash_resume.rs`).

## Messstand für Fensterabläufe

`tools/probe/README.md`: Test-VM auf dem Titan, Attrappen-Agenten, Ereignisprotokoll
(WinEvent-Hook) und Video von außen (QEMU-VNC), Kennzahlen je Ablauf (neuer Agent,
schließen, Fokuswechsel, alle holen/verstecken, umordnen, neue Auflösung), auch unter
Last. Ergebnisse und Vorher/Nachher-Videos in `docs/messung/`. Für die Befehle der
Oberfläche öffnet das Deck mit `AGENT_DECK_DEVTOOLS_PORT=<port>` den DevTools-Port der
WebView2 (nur im Messstand gesetzt).

Ändert ein Monitor Auflösung oder Skalierung, kachelt das Deck die Blöcke neu (Log:
„Anzeige geändert …“); kommt ein Monitor dazu oder fällt einer weg, bleibt alles liegen.

## Hell/Dunkel

Einstellung `appearance` (`docs/DATEIEN.md`). Im Log steht jede Änderung der Einstellung
(`Hell/Dunkel: sun (Zeitzone Mitteleuropa)`) und bei Sonnenstand jedes Umschalten von
Windows mit Ziel und nächstem Wechsel in UTC (`Hell/Dunkel nach Sonnenstand (Zeitzone
Mitteleuropa): Windows auf dunkel,
nächster Wechsel 2026-11-18 06:50:12Z`), ein Fehler beim ersten Mal und bei der Erholung.

- **Sonnenstand:** NOAA-Näherung aus dem früheren Skript (`deck_core::sun`), genau auf
  2 min um die Sonnenwenden, bis 3 min dazwischen. Der Ort kommt grob aus der Zeitzone
  von Windows (`deck_win::time_zone::current`, Tabelle Zeitzonen-Schlüssel → Ort in
  `deck_core::sun_zone`; unbekannte Zone: Längengrad aus dem UTC-Abstand, Breite 45°).
  Innerhalb einer Zone verschiebt sich der Sonnenuntergang um höchstens eine halbe
  Stunde, eine Ortsfrage wäre das nicht wert. Eine Erweiterung kann den Ort ersetzen
  (`sun_place`). Geschaltet werden
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

## Reserve-Agent

Der Kern kann einen Agenten versteckt vorwärmen, damit ein neuer (＋, Strg+Alt+N) sofort
da ist (`deck_core::reserve`, `wt_reserve.rs`). Die Team-Ausgabe schaltet ihn nie ein und
zeigt keinen Schalter; eine Erweiterung bittet darum (`DeckExtension::reserve_agent`).

Windows Terminal öffnet jedes Fenster aktiv. Damit niemand in die Reserve tippt, legt das
Deck sie nur an, wenn seit 60 s weder Taste noch Maus kam und kein Terminal vorn ist
(Windows Terminal, Konsole, mintty, ConEmu), fragt das direkt vor dem Start noch einmal,
gibt den Fokus gleich nach dem Verstecken zurück und verwirft die Reserve, wenn während
des Anlegens doch eine Eingabe kam (24.11.2026; vorher reichten 5 s Ruhe, und Getipptes
landete im versteckten Agenten). Bei Agenten in WSL startet die Reserve wie jeder Agent
über `deck-wsl.exe` in der eingestellten Distro.

## Im Betrieb noch zu prüfen

Nur gegen Attrappen getestet:

- Hell/Dunkel echt: Sonnenstand schaltet Windows um (und nach dem Aufwachen); fest dunkel bei hellem Windows: Panel, Dialoge,
  Rahmen, Glow und ein neuer Agent dunkel, offene Agenten erst nach Neustart.

- Strg+< springt zum Wartenden, Klick auf eine Karte holt das richtige Terminal, ＋ legt
  einen Agenten an.
- Installation aus dem Paket auf einem frischen Rechner (Server 2025, nur PowerShell 5.1):
  Windows-Suche findet „Agent Deck“, das Terminal lädt das Fragment.
- Papier-Look und Deinstallation auf einem frischen Rechner (17.11.2026 nur gegen
  Attrappen): Schrift ohne Neuanmeldung im Terminal, Shader greift, Theme `custom:papier`
  in den Deck-Agenten, die frühere Infozeile hinter der des Decks (Git Bash), „Apps“ ›
  Deinstallieren aus dem eigenen Fenster samt Rückfrage und Enter am Ende, eine geladene
  Schrift verschwindet nach dem nächsten Anmelden.
- `crash_resume` bei einem echten Absturz.
- Agenten in WSL mit angemeldetem Claude (24.11.2026 nur mit einer claude-Attrappe in der
  Distro und mit nachgestellten Hook-Aufrufen geprüft): echte Hook-Ereignisse samt
  Notification, Auto-Compact und Shift+Tab an die TUI, Ticket-Prompt mit Marker unter
  `/mnt/c/…`, Zusammenfassung mit dem `claude` der Distro, Usage aus deren
  `.credentials.json`, worktree-Aufräumen per git in der Distro.
- Ob `live.json` `cwd` bei Agenten im `wt/`-Ordner den worktree trägt.
- Reserve-Agent mit echtem Claude (20.11.2026 nur in der VM ohne Claude geprüft: anlegen
  unter dem Vorhang, verstecken, Fokus zurück, Übernahme in 0,1 bis 0,5 s, Neustart des
  Decks übersteht die Reserve): ob Claudes Oberfläche nach dem Größenwechsel beim
  Übernehmen sauber neu zeichnet; Reserve in WSL.
- „Ohne Fenster“ echt (startet node/pwsh/cmd versteckt):
  `cargo test -p deck-services --test orphans_real -- --ignored --nocapture`.

## Offen

- **Taskleisten-Abzeichen:** der dringlichste Status als Overlay auf dem Agenten-Knopf
  (`ITaskbarList3::SetOverlayIcon`, wirkt auch auf fremde Fenster, gemessen 09.11.2026).
- **Makropad:** 6 Tasten mit Drehregler und ARGB je Taste (Taste = Agent, Licht = Status,
  Druck = zum Agenten springen, Halten = Aufnahme mit Claude `/voice`). Die Tasten
  (F13 bis F21 ohne Modifier) kommen schon an; ARGB-Licht je Status fehlt.
