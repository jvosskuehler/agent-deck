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
neben dem Code.

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
