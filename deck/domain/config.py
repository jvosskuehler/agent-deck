"""Deine Einstellungen - das Einzige, was du anfassen musst.

WICHTIG: Trag unten die Ordnernamen deiner beiden VS-Code-Fenster ein, genau so
wie sie im Fenstertitel stehen (der Teil vor " - Visual Studio Code"). Die
brauchen wir nur noch, um beim Fokussieren das richtige der 2 Fenster nach vorn
zu holen - die Auswahl des einzelnen Terminals (Pane) macht die Extension.
"""
import os

# Fenster-Buchstabe -> Textstueck, das eindeutig im jeweiligen Fenstertitel vorkommt
# (i.d.R. der Projekt-Ordnername). Wird gross/klein-unabhaengig gesucht.
#
# LEER LASSEN ist der Normalfall und ausdruecklich richtig: welches Fenster A, B, C ist,
# legt man im Panel per Klick fest ("Fenster A" anklicken, dann das VS-Code-Fenster),
# und das Deck merkt sich das in bindings.json. Eintraege hier sind nur eine Vorbelegung
# fuer den ERSTEN Start - wer immer dieselben zwei Repos offen hat, spart sich damit die
# zwei Klicks:
#     WINDOW_MATCH = {"A": "mein-frontend", "B": "mein-backend"}
# Fremde Namen hier sind schlimmer als keine: sie binden beim ersten Start ein Fenster,
# das es nicht gibt, und die Kachelreihe bleibt ohne erkennbaren Grund leer.
WINDOW_MATCH: dict[str, str] = {}

# Tasten fuer die Aktions-Buttons (Namen aus der KEYMAP der Extension).
APPROVE_KEY = "enter"   # dem fokussierten Agent "Ja/Weiter" schicken
REJECT_KEY = "esc"      # abbrechen / ablehnen

# "⌫ Leeren"-Button: loescht den KOMPLETTEN Text im Eingabefeld des aktiven Agents,
# ohne den Chat-Verlauf anzutasten (also NICHT /clear!). Claude Code bindet dafuer
# doppeltes Esc = "Clear input draft": funktioniert cursor- und mehrzeilen-unabhaengig;
# bei bereits LEEREM Feld oeffnet es nur das Rewind-Menue -> es beendet also nichts.
# Wir schicken darum 2x Esc mit kleiner Pause dazwischen (wie zwei echte Tastendruecke).
CLEAR_INPUT_KEY = "esc"     # KEYMAP-Name der Taste (Extension: esc -> \x1b)
CLEAR_INPUT_REPEAT = 2      # doppeltes Esc = Entwurf loeschen (1 = aus)

# ── Eigene Buttons (Raster) ──────────────────────────────────────────────
# SEND_BUTTONS + PROMPT_BUTTONS sind jetzt nur noch die STANDARD-Belegung des
# frei anpassbaren Button-Rasters: Beim ersten Start werden sie in buttons.json
# uebernommen. Danach verwaltest du deine Buttons direkt im Panel:
#   • ＋-Kachel klicken          -> neuen Button anlegen (Label + Text/Kommando)
#   • Rechtsklick auf eine Kachel -> bearbeiten / loeschen
#   • Kachel ziehen (Drag&Drop)   -> im Raster umsortieren (rastet in die Zellen ein)
#   • Rechtsklick auf ＋          -> "Auf Standard zuruecksetzen" (nimmt wieder die
#                                    beiden Listen hier unten)
# Aenderungen an diesen Listen wirken sich also erst nach einem Reset (oder wenn
# buttons.json fehlt) aus.
#
# Extra-Buttons unter den Agenten: (Label, Slash-Kommando). Der Text geht an den
# GERADE AKTIVEN Agent (vorher eine Kachel anklicken) und wird direkt abgeschickt.
# Model-Aliasse hier anpassen, falls Claude Code sie anders benennt.
#
# Hinweis zu /effort: Level direkt als Argument mitgeben (z.B. "/effort xhigh"),
# dann kommt KEIN interaktiver Slider. Gilt session-weit. "ultracode" braucht
# Claude Code v2.1.203+, ein Modell mit xhigh-Support (Opus- und Fable-Reihe) und
# aktivierte Dynamic Workflows. Achtung: nach einem Modellwechsel (Opus/Fable)
# setzt Claude das Effort ggf. auf den Modell-Default zurueck -> xhigh/ultracode
# danach einmal neu klicken.
SEND_BUTTONS = [
    ("Clear", "/clear"),
    # Aliasse OHNE Version: Claude Code loest "opus"/"fable" immer auf das neueste
    # Modell der Reihe auf. "opus[1m]" = neuestes Opus mit 1M-Kontext (nicht "nur" Opus).
    ("Opus",  "/model opus[1m]"),
    ("Fable", "/model fable"),
    ("Ultracode", "/effort ultracode"),
    ("xhigh", "/effort xhigh"),
]

# Prompt-Buttons: fertige Text-Prompts (kein Slash-Kommando), die – genau wie die
# SEND_BUTTONS – an den GERADE AKTIVEN Agent geschickt und direkt abgeschickt
# werden. Eigene Leiste direkt unter den SEND_BUTTONS. Hier beliebig erweitern:
# (Label, Prompt-Text).
PROMPT_BUTTONS = [
    # Deutsch: die letzte Antwort in einfacher Sprache zusammenfassen (Ausgabe bleibt
    # Deutsch – das ist der Zweck des Buttons; nur die Anweisung ist auf Englisch).
    ("Erklären",
     "Please summarize your last message in German, in simple, clear language: short "
     "sentences, no jargon (and if it's unavoidable, briefly explained), at most three "
     "to four sentences, so that even someone with no prior knowledge understands it "
     "immediately."),

    # Git-Workflow (GitLab): je ein Klick -> der aktive Agent erledigt den Schritt.
    ("Commit",
     "Create a git commit for the current changes. Look at the diff first, stage what "
     "belongs together, and write one concise Conventional-Commits message "
     "(type(scope): summary) that captures the intent — not a file list. Leave out "
     "unrelated changes and don't push."),
    ("Push",
     "Push the current branch to origin. If it has no upstream yet, set it with "
     "`git push -u origin HEAD`. Never force-push unless I explicitly ask."),
    ("MR",
     "Push the current branch and open a GitLab merge request into the default branch. "
     "Give it a clear title and a short description of what changed and why (plus how "
     "it was tested). Use `glab mr create` if available, otherwise push with "
     "`-o merge_request.create`."),

    # Review: intelligente Multi-Agent-Review – Sonnet faechert breit auf (ein Agent je
    # Pruefaspekt), danach prueft Opus die riskanten Funde adversarial (widerlegen statt
    # bestaetigen) nach. So bleiben nur echte, verifizierte Findings uebrig.
    ("Review",
     "Rigorously review my current uncommitted changes (git diff plus staged — the "
     "work headed into this MR). Use the Task tool to fan out parallel Sonnet "
     "subagents, one per lens — correctness, security, concurrency/races, "
     "error-handling/resource-leaks, and regressions against stated intent — each "
     "reporting every candidate with file:line, severity, and confidence, filtering "
     "nothing. Then, as Opus at high reasoning effort, merge and dedupe; for each "
     "Critical/High candidate spawn a fresh adversarial subagent given only the diff "
     "and the bare claim (not the finder's reasoning), tasked to refute it — trace the "
     "real code path or run the relevant test where feasible. Mark each "
     "CONFIRMED/PLAUSIBLE/REFUTED; never drop anything silently, just tag Medium/Low. "
     "Return one severity-ranked list of verified, real issues, each with file:line "
     "and a one-line failure scenario, no style nitpicks in the ranking. If nothing "
     "significant surfaces, say so plainly."),
]

# ── Ticket -> isolierter git worktree ────────────────────────────────────
# Rechtsklick auf eine Agenten-Kachel -> "Ticket zuweisen…": das Deck merkt sich
# die Ticket-ID fuer diesen Slot (steht danach auf der Karte, unter dem Modell und
# ueber dem Status) und schickt dem laufenden Agenten EINEN Prompt, der ihn anweist,
# in einem eigenen git worktree fuer den Branch <PREFIX><ticket> zu arbeiten. So
# kommt er den anderen Agenten am selben Repo nicht in die Quere (eigenes Verzeichnis
# + eigener Branch, gemeinsames .git). Der Agent legt den worktree selbst an -> keine
# Extension-Aenderung noetig.
TICKET_BRANCH_PREFIX = "ticket/"   # Branch-Name = PREFIX + kleingeschriebener Ticket-Slug

# Jira-Projekt-Key: wird der Ticket-Nummer vorangestellt, wenn du im Dialog NUR eine
# Zahl eingibst (z.B. "2701" -> "PROJ-2701"), damit der Agent das Ticket eindeutig in
# Jira nachschlagen kann. Gibst du schon einen vollen Key ein (z.B. "PROJ-2701" oder
# "ABC-42"), bleibt der unveraendert. Fuer ein anderes/weiteres Projekt hier anpassen;
# leer lassen -> reine Nummern werden unveraendert weitergereicht.
JIRA_PROJECT_KEY = "PROJ"

# WICHTIG: EINZEILIG halten (kein \n)! Der Text wird als EINE Bracketed-Paste in den
# pty geschrieben und per separatem Enter abgeschickt (siehe extension.js); einzeilig
# bleibt er trotzdem – so bleiben Tests/Anzeige simpel und nichts wird vorne abgeschnitten.
# Platzhalter: {jira_key} = eindeutiger Jira-Issue-Key (Projekt-Praefix + Nummer, siehe
# JIRA_PROJECT_KEY), {ticket} = roh eingegebene ID, {branch} = ticket/<slug>, {slug} =
# ordner-tauglicher Slug, {wt_marker} = Pfad der worktree-Marker-Datei, {task} = deine
# (auf eine Zeile geglaettete) Aufgabe.
TICKET_PROMPT = (
    "You are assigned Jira ticket {jira_key}. Work on it in an ISOLATED git worktree so "
    "you don't get in the way of other agents in this repo. Set yourself up first: find "
    "the repo root with `git rev-parse --show-toplevel` and create a worktree for the "
    "branch `{branch}` in a location NEXT TO the repo (e.g. "
    "`<repo-root>/../<repo-name>.wt/{slug}`) — if the branch already exists, attach the "
    "worktree to it (`git worktree add <path> {branch}`), otherwise create it fresh "
    "(`git worktree add <path> -b {branch}`); if the worktree already exists, just use "
    "it. Then switch into that worktree (`cd <path>`) and from now on treat it as your "
    "working directory for EVERYTHING (reading, editing, commits, commands) — do NOT "
    "touch the main checkout under the repo root or the worktrees of other tickets. "
    "Write the ABSOLUTE path of this worktree (only the path, one line, with forward "
    "slashes, nothing else) into the file `{wt_marker}` — the dashboard uses this to "
    "detect the worktree and clean it up again when the agent is closed. Next, look up "
    "the Jira ticket {jira_key} with your Atlassian/Jira integration (e.g. the Jira MCP "
    "getJiraIssue, or search by that key) and read its summary, description and "
    "acceptance criteria. Then give me a SHORT briefing: the worktree path and active "
    "branch, plus the ticket's title, type, status and — in 2-4 bullet points — what "
    "actually needs to be done. If you cannot reach Jira, tell me so and show the ticket "
    "key instead of inventing its content. Then get started with: {task}"
)
# Dasselbe fuer einen Block, der NICHT in einem Repo laeuft, sondern im Ordner ueber
# allen Repos (Heimat-Block, config.TERMINAL_HOME). Dort gibt es kein
# `git rev-parse --show-toplevel`; der Agent muss erst am Ticket ablesen, welche Repos
# es betrifft, und legt die Worktrees nach dem Schema der CLAUDE.md in diesem Ordner an
# (`wt-<Nummer>-<Kuerzel>`, Branch nach der Regel des jeweiligen Repos).
# Bewusst OHNE Marker-Datei: das Deck raeumt markierte Worktrees beim Schliessen des
# Agenten mit `git worktree remove --force` weg - auch mit offenen Aenderungen. Die
# wt-*-Worktrees leben aber bis zum Merge und gehoeren nicht dem einzelnen Agenten.
# Platzhalter: {jira_key} {number} {root} {task}. Einzeilig, wie TICKET_PROMPT.
TICKET_PROMPT_HUB = (
    "You are assigned Jira ticket {jira_key}. You are running in {root}, the folder that "
    "holds ALL repositories; it is not a git repository itself, and its CLAUDE.md has the "
    "repo map and the worktree convention. Set yourself up first: look up {jira_key} with "
    "your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue) and read its "
    "summary, description and acceptance criteria. Decide which repositories the ticket "
    "touches. For EACH of them, work in a git worktree next to the repos named "
    "`{root}/wt-{number}-<short>` (short name as in the CLAUDE.md, e.g. api, ui, dotnet, "
    "client) on a branch that follows that repository's own convention from its "
    "CLAUDE.md (typically `<type>/{jira_key}-<slug>` from the default branch). If such a "
    "worktree or branch already exists, reuse it instead of creating a new one. From now "
    "on work ONLY inside those worktrees; never touch the main checkouts or the worktrees "
    "of other tickets, and never remove a worktree yourself. Then give me a SHORT "
    "briefing: worktree path(s) and branch(es), the ticket's title, type, status and, in "
    "2-4 bullet points, what actually needs to be done. If you cannot reach Jira, tell me "
    "so and show the ticket key instead of inventing its content. Then get started with: "
    "{task}"
)
# Fallback-Task, wenn du kein Aufgaben-Text mitgibst (nur Ticket-ID eingegeben).
TICKET_TASK_FALLBACK = "Then wait for my next instruction."

# Zweiter Weg im Ticket-Dialog ("🔎 Im Chat suchen"): das Deck kennt die ID NICHT
# vorher – der Agent soll sie im bisherigen Chat finden. Damit sie trotzdem auf der
# Karte landet, schreibt der Agent die gefundene ID in die Marker-Datei {marker}
# (das Deck liest sie von dort). Ebenfalls EINZEILIG halten. Platzhalter: {prefix} =
# Branch-Praefix, {marker} = absoluter Pfad der Ticket-Marker-Datei, {wt_marker} =
# Pfad der worktree-Marker-Datei, {task} = geglaettete Aufgabe.
TICKET_SEARCH_PROMPT = (
    "Look through OUR previous chat and find the ticket number/ID this is about (e.g. "
    "ABC-123 / PROJ-42). Then work on that ticket in an ISOLATED git worktree so you "
    "don't get in the way of other agents in this repo: find the repo root "
    "(`git rev-parse --show-toplevel`) and create a worktree for the branch "
    "`{prefix}<lowercased-id-with-hyphens>` in a location NEXT TO the repo (if the "
    "branch/worktree already exists, use it), switch into it (`cd`) and from now on "
    "work EXCLUSIVELY there — do NOT touch the main checkout or other worktrees. Write "
    "the ticket ID you found (only the ID, nothing else, no line breaks) into the file "
    "`{marker}` and the ABSOLUTE path of the worktree (only the path, one line, forward "
    "slashes) into the file `{wt_marker}` — with this the dashboard shows the ticket "
    "and cleans up the worktree again when the agent is closed. Then look up that Jira "
    "ticket with your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue) and "
    "read its summary, description and acceptance criteria. Give me a SHORT briefing: "
    "which ticket you found, the worktree path, and the ticket's title, type, status "
    "and — in 2-4 bullet points — what actually needs to be done. Then get started "
    "with: {task}. If you find NO unambiguous ticket number in the chat, ask me instead "
    "of guessing."
)

# Permission-Mode-Buttons. Claude Code hat KEINE Slash-Kommandos fuer die Modi und
# schaltet sie nur ZYKLISCH per Shift+Tab weiter. MODE_CYCLE = die Reihenfolge in
# DEINER Version (per Shift+Tab beobachtet). Das Deck merkt sich pro Chat den zuletzt
# gesetzten Modus (Start = MODE_START) und schickt genau so viele Shift+Tab, um vom
# angenommenen aktuellen zum Ziel-Modus zu kommen -> die Buttons setzen gezielt.
# ZUVERLAESSIG, solange du den Modus NUR ueber diese Buttons aenderst (nicht
# zusaetzlich von Hand per Shift+Tab im Terminal) und der Chat mit MODE_START startet.
# Steht Bypass als Startmodus in settings.json, schiebt das Deck "bypass" selbst
# hinter "plan" ein (status_model.mode_cycle); hier also NICHT eintragen.
MODE_CYCLE = ["manual", "accept", "plan", "auto"]
MODE_START = "manual"   # Modus, in dem ein frischer Chat startet
MODE_BUTTONS = [        # (Label, Ziel-Modus aus MODE_CYCLE)
    ("Plan", "plan"),
    ("Auto", "auto"),
]

# Automatischer Startmodus fuer NEU per ＋ angelegte Agenten: sobald der frische
# Chat sein erstes Hook-Event meldet (mit SessionStart-Hook = TUI bereit, sonst
# spaetestens beim ersten Prompt), schaltet das Deck ihn EINMALIG hierauf – genau
# wie ein Klick auf den passenden Mode-Button. Muss in MODE_CYCLE stehen. None oder
# "" schaltet die Automatik ab (dann startet jeder Chat wie gehabt in MODE_START).
# Fuer den sofortigen Wechsel gleich beim Oeffnen den SessionStart-Hook einrichten
# (siehe SETUP.md, Schritt 2) – ohne ihn greift der Wechsel erst beim ersten Prompt.
NEW_AGENT_MODE = "auto"

# Der Wechsel geschieht per Shift+Tab (es gibt kein absolutes "geh in Modus X" im
# Terminal). Der SessionStart-Hook feuert aber sehr frueh – oft BEVOR die Claude-TUI
# die Back-Tab-Sequenz verarbeitet, sonst verschluckt sie einzelne Taps und der Agent
# "bleibt auf dem Weg haengen" (accept/plan statt auto). Zwei Puffer dagegen:
AUTO_READY_GRACE = 1.5   # s nach dem 1. Hook warten (TUI-Eingabe warmlaufen lassen), dann erst tippen
AUTO_MAX_TRIES   = 3     # so oft nachtreiben, wenn ein Ist-Hook zeigt, dass er kurz gelandet ist

# ── Claude-Nutzung im Header ─────────────────────────────────────────────
# Links im Header zeigt das Deck die aktuelle Session-Auslastung (die laufenden
# 5 Stunden) als Ampel-Badge. Hover -> Woche + weitere Limits + Reset-Zeiten.
# Klick -> oeffnet claude.ai/settings/usage.
#
# Das OAuth-Token dafuer kommt aus der Claude-Code-CLI (~/.claude/.credentials.json)
# ODER aus Claude Desktop; es reicht EINE der beiden Quellen, und die CLI hat
# ohnehin jeder (siehe claude_usage.py). False -> das Modul wird gar nicht erst
# geladen, keine der Dateien wird angefasst.
SHOW_USAGE = True
# Abfrage-Takt in Sekunden. Nutzung aendert sich langsam; ein groesserer Wert
# schont das API-Rate-Limit – wichtig, falls parallel ein zweiter Usage-Anzeiger
# pollt (beide teilen sich dasselbe Limit). Untergrenze 30 s.
USAGE_POLL_SECONDS = 120

# ── Hover-Zusammenfassung ────────────────────────────────────────────────
# Hover ueber eine Agent-Kachel zeigt einen KI-generierten Kurzsatz, WORUM es in
# dem Chat geht (statt der zuletzt gestellten Frage). Das Deck liest dafuer das
# Claude-Code-Transcript der Session und ruft `claude` HEADLESS im safe-mode auf
# (CLAUDE.md/Skills/Hooks/MCP aus -> schnell, OAuth-Auth bleibt). Das Ergebnis wird
# pro Session gecacht und nur bei echtem Zuwachs neu erzeugt.
#  HOVER_SUMMARY = False -> zurueck zur bisherigen Anzeige ("Letzte Frage").
HOVER_SUMMARY = True
# Modell fuer die Zusammenfassung: haiku ist guenstig + schnell und voellig
# ausreichend. Alias ("haiku"/"sonnet"/…) oder voller Name moeglich.
HOVER_SUMMARY_MODEL = "haiku"
# Ein einzelner claude-Aufruf braucht ~8-13 s (fast nur CLI-Startup). Damit der
# Hover trotzdem SOFORT etwas zeigt, erzeugt das Deck die Zusammenfassungen offener
# Agenten schon im Hintergrund VOR – dann ist beim Hovern meist alles gecacht.
#  True  -> schnellster Hover (kostet je Session einmal einen kleinen Haiku-Aufruf,
#           auch wenn du die Kachel nie hoverst).
#  False -> erst beim ersten Hover erzeugen (spart Aufrufe, dafuer ~10 s Wartezeit
#           mit Platzhalter beim allerersten Hover einer Session).
HOVER_SUMMARY_PREFETCH = True

# ── Ticket- und PR-Nummer aus dem Chat lesen ─────────────────────────────
# Unabhaengig davon, ob du einem Agenten per Rechtsklick ein Ticket zugewiesen hast:
# das Deck durchsucht dasselbe Transcript (reine Regex, KEIN Modell/Kosten) danach,
# WORAUF sich der Chat bezieht, und zeigt das im Hover-Tooltip ueber der
# Zusammenfassung ("Ticket: PROJ-2691 · PR #62"). Erkannt werden:
#  - Ticket: Key-Form ABC-123 (gross geschrieben), das konfigurierte
#    JIRA_PROJECT_KEY-Projekt zusaetzlich klein (z.B. in Branch-/Pfadnamen) und
#    "Ticket 2701"-Nennungen mit blosser Nummer.
#  - Pull Request: "PR #62" / "pull request 62" / "merge request 62", PR-URLs
#    (github …/pull/62, gitlab …/-/merge_requests/62) und ein blosses "#62", wenn es
#    mehrfach faellt (einmal reicht nicht – das koennte auch eine Issue-Nummer sein).
#  TICKET_AUTODETECT = False -> Erkennung ganz aus.
TICKET_AUTODETECT = True
# Das ERKANNTE zusaetzlich auf der Karte zeigen (gedimmtes Violett, "PROJ-2691 #62"),
# solange dort keine zugewiesene ID mit worktree steht. Passt beides nicht in die
# schmale Zeile, gewinnt das Ticket. False -> die Karte bleibt wie bisher (nur
# zugewiesene Tickets MIT worktree), der Hover zeigt die Bezuege trotzdem.
TICKET_AUTODETECT_ON_CARD = True

# Fenster-Transparenz (Windows).
#  TRANSPARENT_BG = True  -> der Hintergrund wird KOMPLETT durchsichtig (man schaut
#                           durch; Karten/Buttons/Text bleiben sichtbar). Leere
#                           Flaechen sind dann durchklickbar; ziehen per Titelleiste.
#  WINDOW_ALPHA  < 1.0    -> ganzes Fenster halbtransparent (0.0..1.0), inkl. Karten.
# Beides kombinierbar. Fuer „nur durchschauen" reicht TRANSPARENT_BG = True.
TRANSPARENT_BG = False
WINDOW_ALPHA = 1.0

# Fenstertitel-Zusatz, damit wir nur VS-Code-Fenster treffen.
VSCODE_MARKER = "Visual Studio Code"

# Broker: hier verbinden sich die VS-Code-Extensions. Muss zu den
# agentDeck.host / agentDeck.port-Settings der Extension passen.
BROKER_HOST = "127.0.0.1"
BROKER_PORT = 8765

# Der Heimat-Block: ein Windows-Terminal-Block ueber dem Ordner, in dem alle Repos
# liegen (dort steht eine CLAUDE.md mit der Landkarte). Er ist immer da und steht immer
# oben, auch ohne offenen Agenten; erst ＋ oeffnet das Fenster mit dem ersten Agenten.
# VS-Code-Fenster kommen wie gehabt darunter dazu. Ueberschreibbar per
# deck_settings.json ("terminal_home"); ein Ordner, den es nicht gibt, schaltet ihn ab.
TERMINAL_HOME = os.path.join(os.path.expanduser("~"), "Repos")

# Sprung-Taste: holt von ueberall das Pane nach vorn, das am laengsten auf dich wartet -
# erst Rueckfragen, dann ungelesene Antworten, nochmal druecken = der naechste
# (domain/attention.py). Ueberschreibbar per deck_settings.json ("jump_hotkey"),
# leerer Text schaltet sie ab. Strg+<: linke Strg-Taste und die <-Taste (links neben Y
# auf der deutschen Tastatur) liegen direkt uebereinander, einhaendig mit links, ohne
# Windows-Taste. NICHT Strg+Y: das ist in fast jedem Programm "Wiederholen", eine
# globale Taste naehme es ueberall weg. Frei geprueft 2026-11-06 (belegt waren
# Strg+Alt+Leertaste und Win+Alt+Leertaste).
JUMP_HOTKEY = "ctrl+<"

# Unterstuetzte VS-Code-Fenster (je ein Buchstabe), bewusst mit Reserve: ist KEIN
# Buchstabe mehr frei, erscheint ein neu geoeffnetes Fenster STILL nicht im Deck
# (_sync_bindings bricht ohne Fehler ab) – ein Fehlerbild, das schwer zu erkennen
# ist, weil das Fenster ganz normal mit dem Broker verbunden ist. Nur gebundene/
# verbundene Fenster werden gezeichnet, ungenutzte Buchstaben kosten also nichts;
# fuer mehr als 15 Fenster hier einfach weitere Buchstaben anhaengen (bis Z).
# WINDOW_MATCH darf fuer jedes davon einen Auto-Default-Ordnernamen enthalten;
# den Rest bindest du per Klick.
WINDOWS = list("ABCDEFGHIJKLMNO")          # 15 Fenster

# Spalten des eigenen Button-Rasters (siehe SEND_BUTTONS/PROMPT_BUTTONS oben).
# Die Buttons fliessen zeilenweise in dieses Raster; mehr Buttons -> mehr Zeilen.
GRID_COLS = 4

# ── Feinschliff (selten noetig) ──────────────────────────────────────────
# Interne Tuning-Werte. Der Standard passt fast immer; hier nur aendern, wenn du
# das Verhalten bewusst justieren willst. Fruere lagen diese Werte fest in
# agent_deck.py – jetzt sind sie hier zentral. Die Werte werden DIREKT gelesen
# (cfg.POLL_MS), nicht mehr per getattr mit Ersatzwert kopiert: ein Ersatzwert
# verbirgt einen Tippfehler im Schlüsselnamen und lässt das Deck stillschweigend
# mit einer anderen Zahl laufen als hier steht.
POLL_MS = 400            # Takt, in dem Status/Verbindungen neu eingelesen werden (ms)
UI_PUMP_MS = 80          # Takt, in dem Ergebnisse aus Arbeits-Threads abgeholt werden
HOVER_TIP_MS = 250       # Verzoegerung, bis der Hover-Tooltip erscheint (ms)
TIP_LEAVE_MS = 80        # verzoegertes Ausblenden - ueberbrueckt Tks Leave+Enter
                         # zwischen zwei Items DERSELBEN Kachel (sonst flackert er)
PENDING_AUTO_TTL = 300   # s: so lange auf den 1. Hook eines neuen Agenten warten,
                         # dann den Auto-Startmodus aufgeben
STALE_S = 900            # so lange (s) ohne Update -> "denkt" gilt als eingeschlafen (idle)
# So lange (s) darf ein gebundenes Fenster getrennt UND ohne offenes VS-Code-Fenster
# sein, bevor das Deck seine Bindung automatisch abraeumt (Kachel verschwindet). Klein
# halten -> schnelles Aufraeumen beim Schliessen; nur als Puffer gegen den kurzen
# Socket-zu/Fenster-noch-da-Moment und Titel-Aussetzer. Ein Reload trifft das NIE, weil
# das native Fenster dabei offen bleibt (Titel sichtbar).
STALE_WINDOW_S = 3.0
# So lange (s) darf ein worktree-Marker OHNE lebenden Agenten (Slot nicht mehr unter
# den Terminals seines verbundenen Fensters) verwaisen, bevor das Deck den worktree im
# Hintergrund abraeumt. Puffer gegen kurze Terminal-Listen-Aussetzer (Split/Sync);
# ein Reload trifft das nie, weil das Fenster dabei gebunden-aber-getrennt bleibt und
# der Sweep solche Faelle _cleanup_closed_windows ueberlaesst.
WT_ORPHAN_GRACE_S = 20.0
# Zweiter, MARKER-UNABHAENGIGER worktree-Sweep: das Deck durchsucht regelmaessig die
# '<repo>.wt/'-Ordner der bekannten Repos direkt auf der Platte und raeumt worktrees ab,
# an denen kein lebender Agent mehr haengt (Besitz = worktree-Marker eines lebenden
# Slots ODER dessen Ticket-Slug == Ordnername). Faengt Reste, die _sweep_orphan_worktrees
# nicht sieht, weil kein Marker (mehr) auf sie zeigt (Agent extern geschlossen, Deck war
# beim Schliessen aus, Loeschen scheiterte an Windows-Dateisperre, halb abgeraeumt).
WT_DISK_SWEEP_INTERVAL_S = 60.0   # so oft (s) die '<repo>.wt'-Ordner absuchen ("jede Minute")
# So lange (s) muss ein '.wt'-worktree ununterbrochen OHNE zugehoerigen Agenten gesehen
# werden, bevor er faellt. Puffer gegen einen gerade erst angelegten worktree, dessen
# Agent Marker/Ticket noch nicht gemeldet hat, und gegen kurze Verbindungs-Aussetzer.
# Bei INTERVAL=60 heisst 90 s: erst nach ~2 unabhaengigen Sichtungen wird abgeraeumt.
WT_DISK_ORPHAN_GRACE_S = 90.0
ANIM_MS = 55             # Frame-Takt der Karten-Animation (Glow/Crossfade), ms
FILL_EASE = 0.30         # Anteil, um den sich die Kartenflaeche je Frame ihrer Zielfarbe naehert
BLOOM_ON_CHANGE = 0.90   # Staerke des kurzen Aufleuchtens bei Statuswechsel
BLOOM_DECAY = 0.82       # Abklingfaktor dieses Aufleuchtens je Frame

# Button-Raster-Geometrie (Kachelmasse in Pixel).
GRID_CW = 96             # Kachelbreite
GRID_CH = 42             # Kachelhoehe
GRID_GAP = 8             # Abstand zwischen den Kacheln
GRID_R = 10              # Eckenradius
