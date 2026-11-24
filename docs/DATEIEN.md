# Dateiverträge Hooks ↔ Deck

Hooks (`deck-report`, `deck-statusline`) und App sind getrennte Prozesse und reden nur
über Dateien. Die Kurzfassung der Regeln steht in `CLAUDE.md` („Verträge“); hier die
Einzelheiten. Wer ein Feld ändert, ändert beide Seiten und diese Datei.

## Zustandsdateien

`%LOCALAPPDATA%\claude-agent-deck\state\<slot>.json`: Hooks schreiben, App liest.
Feldnamen snake_case, `ts` Unix-Sekunden als Float. Immer atomar schreiben (`.tmp` +
ersetzen, kompaktes UTF-8-JSON über serde), nie mit Sperre lesen. Gelesen wird tolerant:
ein Feld mit falschem Typ gilt als fehlend (`deck_core::field`), kein Hook scheitert
daran. Ebenso `<slot>.live.json`, `.ticket`, `.worktree`, `.wtpid`.

Laufzeitdateien der App (Bindungen, Reihenfolge, Einstellungen, Tickets, WT-Hosts,
`orphans.json`, `ui-alive.json`, `heartbeat.json` …) liegen in
`%LOCALAPPDATA%\claude-agent-deck\`, nicht neben dem Code. `heartbeat.json` liest nur
noch der nächste Start (`instance_guard`, Format in `deck_services::heartbeat`).
`statusline.before.wsl.json`: wie `statusline.before.json` (unten), für die
Claude-Einstellungen in der WSL-Distro (`deck-setup install --wsl`); `deck-statusline`
führt sie in der Distro aus (`wsl.exe -d <Distro> -e sh -c`), wenn der Agent dort läuft.
Dort auch `statusline.before.json` (`{"statusLine": <alter Wert>}`): die fremde statusLine, die `deck-setup install` ersetzt hat. Solange sie liegt, führt
`deck-statusline` ihr `command` bei jedem Lauf mit demselben stdin aus (Git Bash wie
Claude Code, sonst `cmd`; höchstens 1 s; Exit ≠ 0, leere Ausgabe oder Zeitüberschreitung
= nichts) und hängt die Ausgabe ohne abschließenden Zeilenumbruch mit `  ·  ` an die
letzte eigene Zeile (`deck_hooks::chain`). Ein Kommando, das dem Deck selbst gehört, wird
nie aufgerufen. `deck-setup uninstall` legt sie zurück und löscht die Datei.

`wt_reserve.json` (`deck_core::reserve`): der versteckt wartende Reserve-Agent
(`slot`, `recipe` = Block, Ordner, claude-Argumente, Farbschema, bei Agenten in WSL
`wsl` = Distro; `since`, `ready`). Nur die App schreibt sie; sie überlebt einen Neustart
des Decks, damit die Reserve danach nicht als Agent ohne Fenster erscheint. Ihre `.wtpid`
liegt wie bei jedem Agenten im State-Ordner (die Herkunftsmarke gilt also auch für ihre
Kinder), aber die App zählt den Slot nicht als lebend, bis sie ihn übernimmt. Kaputt oder
fehlend = keine Reserve. Eine Reserve gibt es nur, wenn eine Erweiterung sie will; ohne
räumt die App eine gemerkte ab.

`installed.json` (`deck_services::installed`): was die Einrichtung hinzugefügt hat,
damit `deck-setup uninstall` genau das wieder herausnimmt. Nur `deck-setup` schreibt
sie (atomar, hübsch formatiert); fehlt sie oder ist sie kaputt, gilt sie als leer; ist
nichts mehr verzeichnet, verschwindet sie.

| Feld | Inhalt | Deinstallation |
|---|---|---|
| `claude_defaults` | `[{key, value}]`, z. B. `permissions.defaultMode` = `bypassPermissions`, `effortLevel` = `high`: nur Vorgaben, die vorher fehlten | nur, wenn der Wert noch gleich ist; ein dadurch leerer `permissions`-Block geht mit |
| `statusline_backup` | Pfad von `statusline.before.json`, wenn eine fremde Infozeile gesichert wurde | zurückgelegt (siehe oben) |
| `fonts` | `[{file, name, path}]`: Schriften, die das Deck pro Benutzer installiert hat | nur, wenn `HKCU\…\Fonts\<name>` noch auf `path` zeigt; eine geladene Datei löscht `RunOnce` beim nächsten Anmelden |
| `files` | `[{path, hash}]`: angelegte Dateien (Theme, Einstellungen der Deck-Agenten, Shader), `hash` = `fnv1a64:<hex>` des Inhalts | nur, wenn der Inhalt noch zum `hash` passt |
| `shortcut`, `wt_fragment` | Pfade | immer (gehören am Namen dem Deck) |
| `autostart` | geschriebener Wert unter `HKCU\…\Run\AgentDeck` | immer |
| `apps_entry` | `true`, wenn der Eintrag unter „Apps“ angelegt ist | immer |
| `wsl` | `{distro, settings, claude_defaults, statusline_backup, linux_bin}` (`linux_bin`: Ordner der Linux-Fassungen der Hooks in der Distro, bei der Deinstallation per `wsl.exe` gelöscht): Hooks und Infozeile in Claudes Einstellungen der WSL-Distro (`settings` als `\\wsl.localhost\…`) | wie `claude_defaults`/`statusline_backup`, in der Distro; ist sie nicht erreichbar, bleibt der Eintrag |

## `deck_settings.json`: Einstellungen und ihre Version

Liegt im App-Ordner, geschrieben von der App (Einstellungsfenster) und vom Installer.
Schlüssel, Vorgaben und Migration stehen in `deck_core::deck_settings`; gelesen wird
tolerant, ein Schalter mit falschem Typ gilt als fehlend.

**Version.** `settings_version` (heute `1`). Beim Laden (`BindStore::load`):

- Datei fehlt, ist leer oder `{}`: **neue Installation**. Es wird nur
  `settings_version: 1` geschrieben; jeder fehlende Schlüssel hat seine Vorgabe (unten).
- Datei mit Inhalt, aber ohne `settings_version` (auch unlesbar): **bestehende
  Installation** von vor dem 17.11.2026. Jeder fehlende Schalter wird ausdrücklich mit dem
  bisherigen Wert eingetragen, dazu `model`, `appearance` und `repos_root` wie bisher;
  vorhandene Werte bleiben. Dann `settings_version: 1`, sofort zurückgeschrieben.
- Mit `settings_version`: nichts wird ergänzt. So bleiben auch die Antworten des
  Installers, wie sie sind.

| Schlüssel | Art | neue Installation | Migration (alte Datei ohne Wert) | Wirkung |
|---|---|---|---|---|
| `repos_root` | Pfad | fehlt | `terminal_home`, sonst `~/Repos`, wenn es ihn gibt | Heimat-Block, Ticket-Prompt; ohne brauchbaren Ordner (fehlt, kein Unterordner mit `.git`) kein Heimat-Block, das Einstellungsfenster geht beim Start auf |
| `terminal_home` | Pfad | – | wird nach `repos_root` kopiert | alter Name, nur noch Rückfall; „Ändern…“ nimmt ihn heraus |
| `model` | Text | fehlt | `opus[1m]` | `--model` neuer Agenten; fehlt = Claude Codes eigene Wahl |
| `auto_permission_mode` | bool | `false` | `true` | neue Agenten per Shift+Tab in Auto |
| `auto_compact` | bool | `true` | `true` | `/compact` vor Ablauf des Prompt-Caches |
| `appearance` | Text | fehlt = `system` | `sun` | Hell/Dunkel (`deck_core::appearance`): `system` Deck und Agenten folgen Windows; `light`/`dark` nur Deck und Agenten fest (Webviews per `AppHandle::set_theme`, Rahmen und Glow per `deck_win::theme::force`, neue Agenten mit `--colorScheme papier-ebenen` bzw. `papier-ebenen-nacht`; offene Agentenfenster wechseln erst beim nächsten Start); `sun` das Deck schaltet Windows selbst bei Sonnenauf- und -untergang (`deck_services::sun_switch`). Unbekannt = `system` |
| `agent_env` | Text | Installer: `wsl` | fehlt = `windows` | Wo neue Agenten laufen (`deck_core::wsl::AgentEnv`): `windows` (PowerShell, `wt_agent.ps1`) oder `wsl` (`deck-wsl.exe`, `wt_agent.sh`). `repos_root` ist dann ein Linux-Pfad. Ein Wechsel gilt ab dem nächsten Start des Decks |
| `wsl_distro` | Text | Installer | – | Distro der Agenten; ohne sie gilt `windows` |
| `wsl_home`, `wsl_mount_root` | Text | Installer | – | `$HOME` und Einhängepunkt der Laufwerke (`/mnt/`) in der Distro; fehlen sie, fragt das Deck beim Start die Distro (`agent_env.rs`) und trägt sie ein |

Dazu wie bisher ohne Version: `dock_edge`, `dock_along`, `panel_pos`, `panel_zoom`, die
Tastenkürzel (`jump_hotkey` …), `glow`, `group_shadow`, `frame_season`.

Schlüssel, die diese Ausgabe nicht kennt (etwa die einer Erweiterung, `extension.rs`),
bleiben in der Datei stehen und werden nie angefasst; so verliert niemand Einstellungen,
der zwischen Ausgaben wechselt. Der Ort für den Sonnenstand ist keine Einstellung, er
kommt aus der Zeitzone von Windows (`docs/BETRIEB.md`, „Hell/Dunkel“).

## Chat-Titel: `summaries\<session>.json`, `areas.json`

Das Deck gibt jedem Chat einen Titel im Muster `Bereich: Thema` (`deck_core::claude::title`).
Das Lagebild-Modell schreibt bei jedem Lauf eine Zeile `Titel:` mit; die App schickt sie
durch die Bremse (erste Prüfung ab der 3. getippten Nachricht, dann alle 3, ein Wechsel
gilt erst nach 2 gleichen Vorschlägen hintereinander) und legt den Stand im Summary-Cache
ab: `title`, `title_cand`, `title_hits`, `title_checked_at`. Kein Ticket, kein PR im
Titel, die Kopfzeile zeigt beide schon.

Die Statuszeile (`deck-statusline`) liest `title` und trägt ihn als `session_name` in
`<slot>.live.json` ein, solange Claude Code keinen eigenen Namen meldet (`/rename` geht
vor). So kommt er in die Kopfzeile und auf die Kachel. An Claude Code geht er bewusst
nicht (`hookSpecificOutput.sessionTitle`): ein Sessionname steht dort oben rechts im
Eingabefeld.

`summaries\areas.json`: Bereich → zuletzt benutzt (Unix-Sekunden). Das Modell bekommt die
Bereiche der letzten 30 Tage, damit derselbe Bereich gleich heißt; es darf neue bilden.

## Agenten in WSL: Pfade und Umgebung

Läuft ein Agent in WSL (`agent_env` = `wsl`), meldet Claude Linux-Pfade. **In die
Zustandsdateien kommen trotzdem nur Windows-Pfade:** `deck-report` und `deck-statusline`
rechnen `cwd`, `transcript_path`, `agent_transcript_path` und `workspace.current_dir`/
`project_dir` des Hook-Eingangs um, bevor sie etwas lesen oder schreiben
(`deck_core::wsl::Wsl::localize_hook_input`): `/mnt/c/x` → `C:\x`, sonst
`\\wsl.localhost\<Distro>\…`. Woran sie den WSL-Agenten erkennen und wohin sie schreiben,
steht in der Umgebung, die `deck-wsl.exe` setzt und per `WSLENV` durch die Distro zurück
in die Windows-Programme reicht:

| Variable | Inhalt |
|---|---|
| `AGENT_SLOT` | Slot (Vertrag 4) |
| `AGENT_DECK_SHELL` | Herkunftsmarke `<pid>.<created>` von `deck-wsl.exe` |
| `AGENT_DECK_DISTRO`, `AGENT_DECK_MNT` | Distro und Einhängepunkt: daran rechnen die Hooks Pfade um |
| `AGENT_DECK_STATE` | Zustandsordner (Windows-Pfad); die Hooks nehmen ihn vor `LOCALAPPDATA` |
| `AGENT_DECK_CLAUDE_DIR` | setzt `wt_agent.sh` mit `/p` (WSL übersetzt ihn in `\\wsl.localhost\…`): Claudes Ordner in der Distro, für die Sprache der Statuszeile |

Der worktree-Marker `<slot>.worktree` enthält bei WSL-Agenten einen Linux-Pfad; das Deck
rechnet ihn beim Lesen um (`Deck::read_files`, `WorktreeSweeper`). Der Ticket-Prompt nennt
dem Agenten den Marker unter `/mnt/c/…` und den Heimat-Block mit seinem Linux-Pfad.
Ein `gitdir:` mit Linux-Pfad (worktree von git in WSL) liest `git_head` über
`deck_core::wsl::beside`.

## `<slot>.wtpid` und die Herkunftsmarke

Bei Agenten in WSL schreibt `deck-wsl.exe` die Datei mit seiner eigenen PID und Startzeit
(gleiches Format, `deck_core::wsl_launch::plan`): Windows sieht den Linux-Prozess nicht,
`deck-wsl.exe` lebt aber genau so lange wie `wsl.exe` im Fenster. Beenden des
Prozessbaums nimmt `wsl.exe` mit.

`wt_agent.ps1` schreibt vor claude atomar `{"pid", "created", "slot"}` (die Shell im Pane;
`created` als FILETIME, über DateTime nur auf etwa eine Sekunde genau) und setzt neben
`AGENT_SLOT` die Umgebungsvariable `AGENT_DECK_SHELL=<pid>.<created>` mit denselben
Werten. Jeder Nachkomme erbt sie, auch wenn das Zwischenglied sofort endet (Claudes
Werkzeug-Shell mit `Start-Process npm run dev`).

„Läuft noch ohne Fenster“ ordnet über die Eltern-Kette zu und, wo die gerissen ist, über
die Marke in der Umgebung des Prozesses (`deck_core::orphans::marker`, gelesen von
`deck_win::proc_env`, je Prozess einmal, nur für Prozesse jünger als die älteste bekannte
Shell). Das Deck nimmt die Marke beim Start aus seiner eigenen Umgebung, damit nichts,
was es selbst startet, einem Agenten zugerechnet wird.

Ein Job-Objekt je Fenster taugt dafür nicht: auf dem Zielrechner steckt jeder Prozess
schon in einem Job, und Kinder erben einen darunter angelegten Job nicht (gemessen
09.11.2026).

## `<slot>.refs.json`

Schreibt das Deck (atomar, nur bei Änderung, weg mit der Kachel): Ticket und PR genau so,
wie die Kachel sie zeigt (`deck_core::tile_refs`):
`{"ticket", "ticket_source", "ticket_detected", "pr", "pr_source", "pr_detected"}`, alle
dürfen `null` sein, Quellen wie `RefSource` (`assigned`, `branch` …).

`deck-statusline` liest nur diese Datei für das Ticket (kein `tickets.json`, kein
`.ticket` mehr, keine eigene Erkennung) und zeigt die Kopfzeile „B3 · ABC-4116 · PR #347
freigegeben · Titel“; nur Geratenes gedimmt, der PR aus dem stdin vor dem des Decks, der
Titel wird um Ticket und PR gekürzt (`TITLE_MAX`). Fehlt die Datei: kein Ticket.

## Woher Ticket und PR einer Kachel kommen

Quellen mit fester Reihenfolge (`deck_core::ref_sources`): zugewiesen, Branch
(`.git/HEAD`, ohne `git`, gecacht in `git_head.rs`), worktree-Ordner des cwd, der worktree
aus den Werkzeugaufrufen im Transcript (`claude::worktree_use`: Edit/Write-Pfade,
`git -C wt/…`, `cd`; Schreiben zählt achtfach, Altes klingt ab), PR aus der Statuszeile,
Sessiontitel, zuletzt der Chat. Einen PR aus dem Chat zu raten ist verboten; nur Claude
Codes Meldung oder der Sessiontitel zählen.

Ein Ticket ist in dieser Ausgabe ein ausgeschriebener Key `ABC-123`: Kürzel groß
geschrieben, nicht in der Stoppliste (`KEY_STOP`: UTF, SHA, ISO …), mindestens zwei
Ziffern; in einem `ticket/`-Branch zählt er auch klein. Ein Projektkürzel, mit dem eine
reine Nummer (`4116-ui`) zum Ticket wird, gibt es nur über eine Erweiterung
(`ticket_prefix`); es steht in `DeckInput::ticket_prefix` (`Deck::ticket_prefix()`).
Ohne Kürzel bleibt eine Nummer eine Nummer, ein Key wird nie ausgedacht.

Probelauf gegen die echten Transcripts (nur lesend):
`cargo test -p deck-core --test worktree_use_real -- --ignored --nocapture`.
