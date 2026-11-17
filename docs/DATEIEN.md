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
`orphans.json`, `ui-alive.json` …) liegen in `%LOCALAPPDATA%\claude-agent-deck\`, nicht
neben dem Code. Darunter `dash-hidden-reviews.json`: JSON-Liste der PR-Adressen, deren
Review-Anfrage im Info-Fenster ausgeblendet ist; nur die App schreibt sie, kaputt = leer.
Dort auch `statusline.before.json` (`{"statusLine": <alter Wert>}`): die
fremde statusLine, die `deck-setup install` ersetzt hat. Solange sie liegt, führt
`deck-statusline` ihr `command` bei jedem Lauf mit demselben stdin aus (Git Bash wie
Claude Code, sonst `cmd`; höchstens 1 s; Exit ≠ 0, leere Ausgabe oder Zeitüberschreitung
= nichts) und hängt die Ausgabe ohne abschließenden Zeilenumbruch mit `  ·  ` an die
letzte eigene Zeile (`deck_hooks::chain`). Ein Kommando, das dem Deck selbst gehört, wird
nie aufgerufen. `deck-setup uninstall` legt sie zurück und löscht die Datei.

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

## `deck_settings.json`: Einstellungen und ihre Version

Liegt im App-Ordner, geschrieben von der App (Einstellungsfenster) und vom Installer.
Schlüssel, Vorgaben und Migration stehen in `deck_core::deck_settings`; gelesen wird
tolerant, ein Schalter mit falschem Typ gilt als fehlend.

**Version.** `settings_version` (heute `1`). Beim Laden (`BindStore::load`):

- Datei fehlt, ist leer oder `{}`: **neue Installation**. Es wird nur
  `settings_version: 1` geschrieben; jeder fehlende Schlüssel hat seine Vorgabe (unten).
- Datei mit Inhalt, aber ohne `settings_version` (auch unlesbar): **bestehende
  Installation** von vor dem 17.11.2026. Jeder fehlende Schalter wird ausdrücklich mit dem
  bisherigen Wert eingetragen, dazu `model`, `jira_prefix` und `repos_root` wie bisher;
  vorhandene Werte bleiben. Dann `settings_version: 1`, sofort zurückgeschrieben.
- Mit `settings_version`: nichts wird ergänzt. So bleiben auch die Antworten des
  Installers, wie sie sind.

| Schlüssel | Art | neue Installation | Migration (alte Datei ohne Wert) | Wirkung |
|---|---|---|---|---|
| `repos_root` | Pfad | fehlt | `terminal_home`, sonst `~/Repos`, wenn es ihn gibt | Heimat-Block, worktree-Aufräumen (`<repos_root>\wt`); ohne brauchbaren Ordner (fehlt, kein Unterordner mit `.git`) keins von beiden, das Einstellungsfenster geht beim Start auf |
| `terminal_home` | Pfad | – | wird nach `repos_root` kopiert | alter Name, nur noch Rückfall; „Ändern…“ nimmt ihn heraus |
| `jira_prefix` | Text | leer | `ORPH` | Jira-Projekt; leer = Nummern bleiben Nummern, kein ausgedachter Key |
| `model` | Text | fehlt | `opus[1m]` | `--model` neuer Agenten; fehlt = Claude Codes eigene Wahl |
| `titan` | bool | `false` | `true` | Titan-Kachel, ssh alle 60 s |
| `store_update` | bool | `false` | `true` | winget-Update von PowerShell und Windows Terminal |
| `speechmike_voice` | bool | `false` | `true` | SpeechMike-Sprechtaste samt Tastatur-Hook (ab Neustart) |
| `auto_permission_mode` | bool | `false` | `true` | neue Agenten per Shift+Tab in Auto |
| `auto_compact` | bool | `true` | `true` | `/compact` vor Ablauf des Prompt-Caches |
| `dashboard` | bool | `true` | `true` | Info-Fenster |
| `pr_automation` | bool | `false` | `true` | PR-Automatik (`deck_core::dash::automation_enabled`) |
| `pr_reviewer` | Text | leer | – | Reviewer der PR-Automatik: GitHub-Logins, mehrere mit Komma; leer = Vorgabe des Repos. Installer und Fenster nehmen Namen oder Mailadressen an und speichern die aufgelösten Logins (`deck_core::dash::reviewers`) |
| `pr_reviewer_names` | Objekt | – | – | Login → Anzeigename der Reviewer (`{"leo-mxyz": "Leo Müller"}`), nur zum Zeigen als „Name (login)“; geschrieben, wenn Namen aufgelöst wurden |
| `github_org` | Text | leer | – | GitHub-Organisation der PR-Automatik; leer und das Konto in genau einer Organisation: die Reviewer-Suche trägt sie ein |

Dazu wie bisher ohne Version: `dock_edge`, `dock_along`, `panel_pos`, `panel_zoom`, die
Tastenkürzel (`jump_hotkey` …), `glow`, `group_shadow`, `frame_season`.

## PR-Feed: `state\pr-feed.json`, `feed\runs\`, `feed\drafts\`

Das Gedächtnis des PR-Feeds (`deck_core::dash::feed_state::FeedState`), nur die App
schreibt es, atomar, kaputt = frisch. Felder (camelCase): `since` (erster Start; ältere
Kommentare und Funde lösen nichts aus), `automation` (früher der Not-Aus; seit 17.11.2026 gilt allein `pr_automation` in
`deck_settings.json`, der Not-Aus-Knopf schreibt dorthin), `hold`
(Halteliste: PR-Schlüssel `owner/repo#12`, nie mergen), `snoozed` und `notified` („Später“
bzw. vom Kommentar-Fenster gezeigt, je PR der Zeitpunkt des neuesten Kommentars),
`conflictSince`, `handled` (was die Hintergrund-Agenten und die Mechanik je PR schon
erledigt haben), `verdicts` (Job-Id → Log erklärt rote CI mit Code Security),
`activity` (Zeitleiste, 48 Std.), `runs` (Hintergrund-Agenten mit Bericht), `drafts`
(Antwort-Entwürfe), `jiraChecked` (gemergte PRs, deren Ticket schon nachgezogen ist), `seenAny` (erster
gelungener Abruf gesehen: Konflikte, die schon dabei bestehen, gelten als alt), `verdictRule`
(Regelstand der Log-Urteile; ändert sich die Regel, werden `verdicts` neu geholt).
Ein Lauf, der beim Beenden des Decks noch lief, gilt beim nächsten Start als gescheitert.

`feed\runs\<id>.jsonl` (+ `.err`): Ausgabe von `claude -p --output-format stream-json`;
der Bericht ist die letzte Zeile vom Typ `result`. `feed\drafts\<key>.md`: Antwort-Entwurf
als reiner Text; „Ändern“ öffnet die Datei, „Absenden“ liest sie frisch.

Einstellungen dazu in `deck_settings.json` (Einstellungsfenster, Bereich Automatik):
`pr_automation` (aus = keine Wirkung nach außen, der Feed zeigt nur), `pr_reviewer`
(GitHub-Logins, mehrere mit Komma, die als Reviewer nachgetragen werden; leer = keiner;
Namen dazu in `pr_reviewer_names`)
und `github_org` (Organisation für die Jira-Suche; leer = Jira wird nicht nachgezogen,
Hinweis im Feed). Hintergrund-Agenten arbeiten in `<repos_root>\<repo>` bzw.
`<repos_root>\wt\<ordner>`.
Protokoll: `state\dash-feed.log` (nur PR-Kürzel, Tickets, Ids, Dauern, Fehlercodes).

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

## `<slot>.wtpid` und die Herkunftsmarke

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
`.ticket` mehr, keine eigene Erkennung) und zeigt die Kopfzeile „B3 · ORPH-4116 · PR #347
freigegeben · Titel“; nur Geratenes gedimmt, der PR aus dem stdin vor dem des Decks, der
Titel wird um Ticket und PR gekürzt (`TITLE_MAX`). Fehlt die Datei: kein Ticket.

## Woher Ticket und PR einer Kachel kommen

Quellen mit fester Reihenfolge (`deck_core::ref_sources`): zugewiesen, Branch
(`.git/HEAD`, ohne `git`, gecacht in `git_head.rs`), worktree-Ordner des cwd, der worktree
aus den Werkzeugaufrufen im Transcript (`claude::worktree_use`: Edit/Write-Pfade,
`git -C wt/…`, `cd`; Schreiben zählt achtfach, Altes klingt ab), PR aus der Statuszeile,
Sessiontitel, zuletzt der Chat. Einen PR aus dem Chat zu raten ist verboten; nur Claude
Codes Meldung oder der Sessiontitel zählen.

Probelauf gegen die echten Transcripts (nur lesend):
`cargo test -p deck-core --test worktree_use_real -- --ignored --nocapture`.
