# Agent Deck

Dashboard für parallel laufende Claude-Code-Agents in VS Code. Eine Kachel je Chat,
Farbe = Zustand; dockt am Bildschirmrand an. Windows-only, Python 3.12+ / tkinter.

## Kommandos

```powershell
.\install.ps1                  # EINRICHTUNG: prüft, holt Pillow, kopiert Extension UND
                               #   registriert sie, merged Hooks+statusLine, beweist den
                               #   Schreibvorgang
.\install.ps1 -Check           # der Doctor — erste Adresse, wenn Kacheln stumm bleiben
python tests/run.py            # alle Unit-Tests, immer vor dem Commit
python tests/test_dock_animation.py   # eine Datei allein läuft auch
python -m compileall -q .      # Syntaxprüfung aller Module
start.bat                      # Panel leise starten (pythonw, keine Konsole)
start_debug.bat                # Panel mit Konsole — für den ersten Start und bei Fehlersuche
```

Einzige Pflicht-Abhängigkeit ist **Pillow** (`requirements.txt`); alles andere kommt aus
der Standardbibliothek.

**Die Einrichtung ist Code, keine Anleitung.** Was in `~/.claude/settings.json` landet,
schreibt `deck/claude/hook_setup.py` — chirurgisch (fremde Hooks bleiben stehen), am
Dateinamen wiedererkennend (ein zweiter Lauf ist ein Nulldurchgang, ein verschobenes
Repo wird repariert statt verdoppelt) und getestet (`tests/test_claude_hook_setup.py`).
Wer die Hook-Einträge ändert, ändert sie **dort**, nicht in der Doku: `docs/SETUP.md`
beschreibt nur noch, was das Skript tut.

Dasselbe gilt für VS Codes Extension-Registratur: `deck/ops/vscode_ext.py` schreibt sie,
nach denselben Regeln und mit denselben Tests (`tests/test_ops_vscode_ext.py`). Beide
Module werden aus `install.ps1` über `Invoke-DeckTool` aufgerufen und liefern ihre
Bilanz als `## fails=N warns=N` zurück — das Skript **zeigt** Urteile an, es fällt keine.
Wer eine neue Prüfung braucht, schreibt sie in Python, wo sie getestet ist. Die Bilanzzeile
ist Pflicht auf **jedem** Rückgabeweg: kommt ein Modul ohne sie zurück (Traceback,
Tippfehler im Modulnamen, halbe Installation), ist das selbst ein Befund — sonst zählt der
Lauf null Probleme und meldet am Ende grün.

## Aufbau

Der Code liegt im Paket `deck/`. Abhängigkeiten zeigen **nur nach unten** — wer eine
Datei anlegt, entscheidet zuerst, in welche Schicht sie gehört:

| Schicht | Enthält | Darf importieren |
|---|---|---|
| `deck/domain/` | Anzeigefreie Domäne: Statusmodell, Pfade, Protokoll, Slot-Zustand, Zuordnung, Reihenfolge, Konfiguration | — |
| `deck/platform/` | Win32: Fokus, DPI, Monitor-Arbeitsbereich | — |
| `deck/render/` | Zeichnerei (Pillow/Canvas): Kachel, Kapsel, Welle, Glow | `domain`, `platform` |
| `deck/net/` | Broker (TCP) und Kommando-Vokabular zur Extension | `domain` |
| `deck/claude/` | Claude-Code-Spezifisches: Usage, Zusammenfassung, Settings, **Hooks** | `domain`, `i18n` |
| `deck/ops/` | Betrieb: Log, Zweitstart-Guard, Wächter, Worktrees, VS-Code-Patch, Extension-Registratur | `domain`, `platform`, `i18n` |
| `deck/dock/` | Andocken am Rand, Griff-Fenster, Slide-Animation | `domain`, `platform`, `render` |
| `deck/ui/` | Panel-Fenster, Kacheln, Interaktion — die oberste Schicht | alle |
| `deck/i18n.py` | Deutsch/Englisch. Querschnitt, liegt auf der Paketwurzel und ist die **einzige** erlaubte Abhängigkeit nach oben (der Sprachregler steht in Claudes `settings.json`) | `claude` |

**Diese Tabelle ist getestet**, nicht behauptet: `tests/test_architecture.py` liest die
echten Importe und wird rot, sobald einer nach oben zeigt — mit Datei und Zeile. Die
Erlaubnisliste dort ist knapp gehalten und nennt nur, was heute wirklich importiert
wird. Ein neuer Import macht den Test also auch dann rot, wenn er die Ordnung einhält;
dann trägt man ihn ein und hat einmal darüber nachgedacht. Mitgetestet wird außerdem,
dass `domain/` ohne `tkinter`/`PIL`/`ctypes` bleibt und die Hooks nicht die Anzeige
nachziehen — sie starten bei jedem Tool-Aufruf neu.

**Faustregel:** Rechnen gehört nach `domain/` und wird getestet; Zeichnen gehört nach
`render/` oder `ui/` und wird angeschaut. Wenn eine Methode in `ui/` etwas ausrechnet,
das man auf Papier nachprüfen könnte, gehört sie nach `domain/`.

### Wo fasse ich was an?

| Ich will … | … dann hierhin |
|---|---|
| wie eine Kachel aussieht | `ui/tile_draw.py`, Werte in `ui/theme.py` |
| wo Kacheln liegen, Reihenfolge | `ui/tiles.py`, Ziehen in `ui/reorder.py` |
| Reihenfolge der Repo-Blöcke | `ui/reorder_blocks.py` (Griff = der Repo-Name), Rechnung in `domain/ordering.py` |
| Statusfarbe oder -text ändern | `ui/theme.py` (`GLOW_STYLE`, `STATUS_LABEL`) |
| wann ein Status kippt | `domain/status_model.py` |
| Poll-Takt, Kacheln nachziehen | `ui/refresh.py` |
| Bindungen, geschlossene Fenster, Auto-Startmodus | `ui/windows.py` |
| Fenstergröße und Skalierung | `ui/layout.py` |
| Tooltip-Inhalt | `ui/hover.py`, Text in `claude/summarize.py`, Ticket/PR in `claude/refs.py` |
| Ein-/Ausklappen, Animation | `dock/animation.py` — **die drei Sicherungen dort lassen** |
| Griff: Aussehen | `render/capsule.py`, Maße und Masken in `render/capsule_masks.py` |
| Griff: Verhalten | `dock/handle.py`, Schwappen in `dock/wave.py` |
| ein neues Kommando an die Extension | `domain/protocol.py` **und** `extension/extension.js` |
| wen `Ctrl+Alt+K` zum Abschießen anbietet | `extension/killable.js` (getestet), Dialog in `extension/extension.js` |
| Hook-Verhalten (was gemeldet wird) | `claude/hooks/report.py` |
| welche Hooks **eingetragen** werden | `claude/hook_setup.py` (`HOOKS`) — und `docs/SETUP.md`-Anhang nachziehen |
| ob VS Code die Extension überhaupt **lädt** | `ops/vscode_ext.py` (Eintrag in `extensions.json`) — der Ordner allein beweist nichts |
| Einrichtung, Voraussetzungs-Prüfung | `install.ps1` — Rechnen und Urteile aber in `claude/hook_setup.py` bzw. `ops/vscode_ext.py`, damit sie getestet sind |
| Usage: Zahlen holen | `claude/usage.py`, Token in `claude/usage_token.py` |
| Usage: Anzeige und Ampelfarben | `claude/usage_view.py`, Balken in `ui/bottombar.py` |
| Usage: Poll-Takt, 429-Backoff | `claude-usage-shared/usage_poller.py` (**außerhalb** des Repos, geteilt mit dem Tray-Monitor) — Diagnose: `doctor.py` |
| Ticket zuweisen | `ui/ticket.py` |
| worktrees abräumen | `ui/worktree_sweep.py`, Git-Teil in `ops/worktree.py` |
| irgendetwas mit Win32 | `platform/` — neue Funktion? Signatur in `platform/win32.py` typisieren |

### Die Einsprungpunkte im Wurzelverzeichnis sind Verträge

Fünf Dateien liegen bewusst **außerhalb** von `deck/` und enthalten nur einen
`runpy.run_module`-Aufruf. Ihre Namen dürfen sich nicht ändern:

| Datei | Wer nagelt den Namen fest |
|---|---|
| `report.py` · `statusline.py` | `~/.claude/settings.json` — **mit absolutem Pfad, auf jedem Rechner** |
| `agent_deck.py` | `start.bat`, `start_debug.bat`, `deck/ops/watchdog.py` (`PANEL`) |
| `watchdog.py` | `start_watchdog.bat` und die **Windows-Aufgabenplanung** (`install_watchdog.ps1`) |
| `reenable_glow.py` | dokumentierter Handaufruf in README und SETUP |

`run_module` statt eines Funktionsaufrufs, weil in den `__main__`-Blöcken Logik sitzt
(das Fangnetz der Hooks, die Log-Installation des Panels).

## Verträge, die man nicht raten kann

1. **Das Wire-Protokoll existiert doppelt** — `deck/domain/protocol.py` und
   `extension/extension.js`. Es gibt bewusst keinen Build-Step, der sie koppelt (reines
   JS/Python, die Extension kann die Python-Datei nicht importieren). Wer einen String
   ändert, ändert **beide**.

2. **Das Slot-JSON-Format ist ein Vertrag.** `%LOCALAPPDATA%\claude-agent-deck\state\<slot>.json`
   wird von den Hooks geschrieben und vom Panel gelesen — zwei getrennte Prozesse.
   Feldnamen sind snake_case, `ts` sind Unix-Sekunden als Fließkommazahl. Immer atomar
   schreiben (`.tmp` + ersetzen), nie mit Sperre lesen.

3. **Ein Hook darf NIEMALS mit Fehler enden.** Er blockiert sonst den Agenten: bei
   `UserPromptSubmit` und `PreToolUse` liest Claude Code Exit ≠ 0 als Veto gegen Prompt
   bzw. Tool-Aufruf. Jeder Pfad in `deck/claude/hooks/` hat ein Fangnetz und Exit-Code 0.

   Das reicht aber nicht: **ein Hook, der nicht startet, kommt an sein Fangnetz nicht
   heran.** Fehlt die Datei, urteilt der Prozessstarter. Darum endet jeder Eintrag in
   `settings.json` auf `|| exit 0` — die äußere Schale, die auch einen fehlenden
   Einsprungpunkt in Exit 0 verwandelt:

   ```
   python "C:\…\agent-deck\report.py" thinking || exit 0
   ```

   **KEIN `cmd /c` davorsetzen.** Claude Code führt Hooks über eine POSIX-Shell aus, und
   deren MSYS-Pfadkonvertierung macht aus `/c` den Pfad `C:\`. `cmd` startet dann ohne
   Schalter, also interaktiv: es gibt seinen Banner aus, liest das Hook-JSON von stdin als
   Befehl — und ruft `python` nie auf. Der Hook endet mit 0 und sieht darum gesund aus,
   meldet aber keinen Status mehr; die Kacheln bleiben stumm grau. Genau das ist am
   2026-07-29 passiert, und es war am Exit-Code nicht zu erkennen, sondern nur daran, dass
   in `state\` keine Datei mehr frisch wurde.

   Beim Umbenennen gilt: **erst den neuen Pfad beweisen, dann den alten löschen** — nie
   umgekehrt. Und „bewiesen" heißt: eine Datei in `state\` ist danach frisch, nicht bloß
   Exit-Code 0.

   Diese drei Regeln sind seit 2026-07-30 **ausführbar**: `install.ps1` schreibt die
   Einträge (statt einer Anleitung, der man von Hand folgt), `install.ps1 -Check` findet
   `cmd /c`, ein fehlendes `|| exit 0` und Pfade ins Leere — und Schritt 5 des Skripts
   führt genau den Beweis, den dieser Absatz verlangt: Hook feuern, dann in `state\`
   nach einer frischen Datei sehen.

4. **Dateien neben dem Code werden über `paths.REPO_ROOT` gefunden**, nie über
   `__file__` des eigenen Moduls. Betroffen sind `bindings.json` und die übrigen
   Laufzeit-JSONs, `assets/robot.ico` und `agent-deck-glow.css`. Rechnet ein Modul selbst
   mit `__file__`, zeigt jede Verschiebung ins Leere — und das fällt nicht auf: die
   Laufzeitdateien entstehen einfach neu am falschen Ort, während die alten mit allen
   Fenster-Zuordnungen unsichtbar liegenbleiben.

5. **Hook-stdin roh als UTF-8 dekodieren** (`sys.stdin.buffer`), nie über `sys.stdin`.
   Sonst kommen Umlaute unter Windows als cp1252-Mojibake an.

6. **Die VS-Code-Extension ist JavaScript** — VS Code lädt nur JS-Extensions. Das ist
   keine offene Aufgabe.

7. **Der Extension-Ordner ist nicht die Installation.** Geladen wird, was in VS Codes
   Registratur `~/.vscode/extensions/extensions.json` steht — der Ordner
   `agent-deck-bridge\` daneben ist nur die Ware. Beides kann auseinanderlaufen: wird der
   Ordner umbenannt, bleibt der Eintrag stehen und zeigt ins Leere. VS Code meldet dann
   beim Start einmal `Unable to read file '…\package.json'` und lädt die Extension **gar
   nicht**, während der richtige Ordner unregistriert daneben liegt.

   Das ist die Extension-Fassung von Falle 3: am Ordner ist nichts zu sehen. Genau
   deshalb hat `install.ps1 -Check` am 2026-07-30 grün „Extension installiert und
   aktuell" gemeldet — es prüfte Datei und Hash, und beide stimmten — während die Brücke
   zum Panel tot war. Seither urteilt `deck/ops/vscode_ext.py` darüber, und zwar
   getestet. Wer die Registratur anfasst, beachtet zwei Dinge: dort stehen **alle**
   Extensions des Nutzers (also ergänzen, nie neu schreiben — und eine Datei, die nicht
   als JSON-Array liest, gar nicht anfassen), und eine Reparatur wirkt erst nach
   „Developer: Reload Window", pro Fenster.

   Derselbe blinde Fleck eine Ebene tiefer: die Extension ist **mehr als eine Datei**
   (`extension.js`, `detect.js`, `killable.js`). Fehlt eines der Nebenmodule, ist das
   kein Teilausfall — `require` wirft beim Aktivieren, und VS Code lädt die Extension
   **gar nicht**, samt Panel-Brücke. `install.ps1 -Check` vergleicht darum seit
   2026-08-03 **jede** Datei des Ordners, nicht mehr nur `extension.js`; die
   Kopier-Bilanz („war schon aktuell") zählt ebenfalls über alle.

8. **Was `Ctrl+Alt+K` anbietet, ist eine Sicherheitsfrage.** Der Befehl beendet
   Prozesse — und im Prozessbaum unter VS Code laufen die Claude-Sessions des Decks.
   Die Auswahlregeln stehen darum in `extension/killable.js`, getrennt vom Dialog und
   getestet (`tests/test_extension_killable.py`, über `node`). Drei Sperren, die
   bleiben müssen: **Claude** (auch als `node …\claude\cli.js` — der Name allein
   genügt nicht), **Sitzung 0** (dort hält `lsass.exe` einen Port, und es zu beenden
   ist ein Bluescreen) und der **Broker-Port**. Angeboten wird nur, wer wirklich einen
   TCP-Port hält; alles andere wäre eine Liste mit 100 Zeilen, in der man den einen
   Blocker sucht. Wer die Regeln lockert, macht aus einem Werkzeug einen Fußschuss —
   ein zu enger Filter kostet nur einen Blick in den Task-Manager.

## Fallen, die schon einmal wehgetan haben

- **Keine `"` in ein `python -c`-Snippet in `install.ps1`.** Windows PowerShell 5.1
  entfernt sie beim Weitergeben an ein natives Programm — aus `print("%d")` wird
  `print(%d)`, Python wirft einen SyntaxError, und der Doctor meldete daraufhin
  „python.exe startet nicht (Microsoft-Store-Platzhalter?)", obwohl Python tadellos lief.
  Unter `pwsh` 7 lief dasselbe Snippet durch; kaputt war ausgerechnet die Aufrufform aus
  der Doku (`powershell -File install.ps1 -Check`). Wer prüft, prüft unter **5.1**.

  Die Fassung mit `"` (Commit `227acb5`) lief noch am selben Tag bei einem Kollegen auf,
  und die Meldung log dabei **zweimal**: sie behauptete „startet nicht", obwohl Python
  tadellos lief — und sie zeigte nur die *erste* stderr-Zeile (`File "<string>", line 1`),
  weil `$ErrorActionPreference = 'Stop'` aus einem `2>&1` bei einem nativen Programm einen
  terminierenden Fehler macht: im `catch` bleibt der NativeCommandError, und der ist eine
  Zeile lang. Genau der Satz mit dem `SyntaxError` fehlte also, und darum las sich ein
  Fehler *im Aufruf* wie ein fehlendes Python. `Try-Run` senkt die Preference seither
  lokal ab und die Meldung gibt den Wortlaut mehrzeilig aus. Merksatz: **die ganze
  Ausgabe zeigen** — die Diagnose steht selten in Zeile 1.
- **Ein gecachter Usage-Wert hinter seinem Reset ist keine Zahl mehr, sondern eine
  Behauptung.** Der optionale gemeinsame Poller (siehe `deck/claude/usage.py`, `_shared`)
  liefert bei HTTP 429 bewusst den **letzten** Wert weiter und vermerkt den Fehler
  daneben — er gibt dazu `data_ts` und `error` heraus. `poll_once` las beides nicht:
  es setzte `ts=time.time()` (das ist der Cache-*Lese*zeitpunkt, nicht der des Abrufs)
  und `error=None`. Damit zeigte das Deck am 2026-08-05 stundenalte 48 % als frisch und
  grün, während das laufende Fenster bei 13 % stand; der Tooltip hat für diesen Fall
  sogar eine Zeile („letzter Wert – …"), die nie erschien. **Eine Zahl, die plausibel
  aussieht, meldet sich nicht selbst** — hier war die Anzeige die letzte Instanz.
  Seither entwertet `usage_view.expire_limits` jedes Limit, dessen `resets_at` vorbei
  ist (Prozent → `None`, also graues „—"), und `_apply` reicht Datenzeit und Fehler
  durch. Prüffaden ist der **Reset-Zeitpunkt**, nicht das Datenalter: ein 20 Minuten
  alter Wochenwert ist brauchbar, ein Session-Wert hinter dem Reset nicht.

  Zwei Anschlussregeln: Die Entwertung sitzt **nicht** in `parse_usage` (das übersetzt
  nur — gleiche Eingabe, gleiche Ausgabe, keine Uhr), sondern beim Übernehmen in den
  Snapshot, wo die Frage an die Uhr hingehört. Und das Deck **pollt bei altem Cache
  nicht selbst nach**: genau dagegen existiert der gemeinsame Poller. Der Endpoint ist
  eng limitiert — gemessen am 2026-08-05 gingen 3 Abrufe im 90-s-Takt durch, danach
  kippte er für rund eine Stunde auf 429 (220 von 399 Abrufen im Log). Wer hier einen
  Direktabruf „zur Sicherheit" einbaut, verlängert die Sperre, statt sie zu heilen.

- **Der Poll-Takt des Usage-Endpoints ist keine Konstante — er wird gelernt.** Am
  2026-08-18 stand das Badge stundenlang auf grünen „0 %", während live 7 % anlagen.
  Der Reset lag noch in der Zukunft, `expire_limits` griff also gar nicht: der Wert
  war nicht *hinter seinem Fenster*, nur **alt**. Derselbe Trugschluss wie am
  2026-08-05, eine Ebene tiefer — und wieder sah die Anzeige gesund aus.

  Die Messung war der Wendepunkt. Bis zum 2026-08-17, 18 Uhr liefen **35 Abrufe
  pro Stunde mit 100 % Erfolg**; ab 19 Uhr scheiterte bei **unverändertem** Takt fast
  die Hälfte, auch nachts ohne Last. Das Limit hatte sich verschoben, nicht unsere
  Last. Daraus folgen vier Regeln, alle in `claude-usage-shared/usage_poller.py` und
  getestet (`test_usage_poller.py`, 25 Fälle, `python test_usage_poller.py`):

  1. **Der Grundtakt lernt sich selbst** (`next_interval`, AIMD): 429 streckt ihn
     ×1,5, ein Erfolg verkürzt ihn nur ×0,9, Grenzen 120–900 s. Die Asymmetrie ist
     der Trick — von unten an die Grenze kriechen, beim ersten Nein deutlich
     abrücken. Er steht im **gemeinsamen** Cache, also lernen Tray und Deck zusammen.
     Jede feste Zahl an dieser Stelle wäre eine Wette auf den Stand von gestern.
  2. **`Retry-After` schlägt die geratene Kurve** (`parse_retry_after`) — auch wenn
     die Auskunft *kürzer* ist. Von Frist und Takt gewinnt aber immer die
     vorsichtigere (`max`): eine abgelaufene Sperre ist keine Einladung zum Pollen,
     unser Budget ist damit nicht erneuert.
  3. **Ein Netzfehler ist keine Entwarnung.** Er setzte `n429` auf 0 und die
     Wartezeit auf 25 s — also mitten in eine laufende Sperre zurück. Im Log stehen
     22 solcher `getaddrinfo failed`, jeder hat das Limit weiter angeheizt.
  4. **429 probiert die nächste Token-Quelle** (CLI → Desktop), bevor der Backoff
     greift. Achtung, das ist die *kleinere* Hilfe: gemessen wurden beide Tokens im
     selben Moment abgewiesen — das Limit hängt am **Konto**, nicht am Token. Es
     rettet nur den Fall, in dem eine Quelle allein klemmt.

  Und die Anzeige zieht nach: `usage_view.badge_view` zeigt einen alten Wert weiter,
  aber **matt** (`_dim`), ab derselben Schwelle, ab der der Tooltip den Stand nennt
  (`STALE_AFTER`) — eine Regel, zwei Orte. Weiterzeigen statt ausblenden, weil „—"
  nichts zu fragen gibt, eine matte Zahl aber zum Hover einlädt, wo der Grund steht.
  Wichtig dabei: die Frische muss **mit in die Redraw-Signatur** von
  `ui/bottombar.py`, sonst filtert der Signaturvergleich das Mattwerden weg (Zustand,
  Prozent und Ampel ändern sich ja nicht — der Wert altert nur still).

  Was schiefgeht, sieht man ohne Rätselraten mit
  `python claude-usage-shared/doctor.py`: Erfolgsquote je Stunde, gelernter Takt,
  Cache-Alter, wer wirklich gepollt hat. **Kein** Netzabruf — ein Doktor, der die
  Sperre verlängert, die er untersucht, ist keiner.

- **`SO_REUSEADDR` ist in `deck/net/broker.py` schädlich.** Unter Windows erlaubt die
  Option zwei Listener auf demselben Port; „Port belegt → still deaktiviert" greift dann
  nicht, und Extensions landen beim toten Panel. Der Guard dagegen ist
  `deck/ops/instance.py` (Lockfile + Handoff), nicht der Port.
- **Kachelliste in place aktualisieren**, nie neu aufbauen — ein `delete('all')`-Vollneubau
  setzt Farbe und Statuswert zurück, und dann blitzen beim Auf-/Zuklappen alle Kacheln neu
  auf. `_carry_tile_anim` vererbt den Animationszustand überlebender Kacheln.
- **Animationen an die Bildperiode hängen**, nicht an ein festes Timer-Intervall. Ein
  Timer läuft gegen die Bildrate und stottert sichtbar; dazu gehören
  `timeBeginPeriod(1)` und `perf_counter` statt der grob getakteten Tk-Uhr.
- **Ein halb ausgefahrenes Deck ist der eine unzulässige Zustand** (angedockt gibt es
  keine Titelleiste, man kommt an nichts mehr heran). Deshalb hat `deck/dock/` genau
  einen Ausgang aus der Animation (`_anim_finish`), eine Deadline als Notbremse und einen
  Watchdog. Diese drei nicht wegoptimieren.
- **Der „gesehen"-Merker muss über den Poll hinaus halten** — in der State-Datei steht
  weiterhin `done`.
- **Deko-Effekte fliegen auf Nachfrage ganz raus**, nicht „nur leiser gestellt". Und ein
  Effekt-Timer, der einen Redraw überlebt, verschiebt Kachel-Text dauerhaft.

## Konventionen

- **Eine Datei = ein Konzept, < 400 Zeilen.** Das gilt inzwischen für **jedes** Modul —
  die größte Datei ist `ui/panel.py` mit 375 Zeilen. Wer eine Datei über die Grenze
  wachsen lässt, hat meist zwei Konzepte darin; der Ausweg ist ein neues Modul, nicht
  eine Ausnahme.
- **Die zwei großen Klassen sind Mixin-Kompositionen.** `AgentDeck` (103 Methoden) setzt
  sich aus 11 Mixins in `deck/ui/` zusammen, `EdgeDock` (97) aus 8 in `deck/dock/`. Eine
  neue Methode gehört in das Mixin ihres Themas — und wenn es keines gibt, in ein neues.
  Der Klassenkopf in `panel.py` bzw. `controller.py` ist die Übersicht.

### Mixin oder eigenes Objekt?

Die Frage entscheidet **eine Zahl**: wie viele `self`-Attribute liest ein Modul, die es
nicht selbst setzt? Gemessen wird das mit demselben AST-Durchlauf wie in
`tests/test_ui_collaborators.py`.

| fremde Attribute | Form | warum |
|---|---|---|
| 0–6 | **eigenes Objekt** mit Konstruktor-Abhängigkeiten | die Liste ist lesbar und das Teil einzeln baubar. Herausgelöst: `SettingsDialog` (4 Werte + 3 Rückrufe), `TileRenderer` (2 + 6), `TileDrag` (5 + 4) |
| ab ~10 | **Mixin** | `tiles` (20), `actions` (12), `refresh` (11) *orchestrieren* den Panel-Zustand — das ist ihre Aufgabe, nicht ein Mangel. In Objekte gepresst ergäben sie 20 Konstruktor-Argumente und gewönnen nichts |

Die Zahl entscheidet auch gegen einen Umbau. `layout` sah nach der ersten Messung mit 6
fremden Attributen wie ein Kandidat aus; genau nachgezählt braucht es **11**
Konstruktor-Argumente (6 Werte, 5 Rückrufe) — und alle fünf Rückrufe dirigieren das
Neuzeichnen (`_render_agents`, `_render_agents_slim`, `_update_tiles`, `_layout_sig`,
`_dragging`). Es hat also keine abgrenzbare Verantwortung, sondern koordiniert die
anderen. Deshalb bleibt es ein Mixin.

Wo ein Kollaborateur von außen befragt wird, bleibt eine **schmale Fassade** auf
`AgentDeck` stehen: `_dragging()` ruft `self.drag.dragging()`. Vier Stellen fragen danach
— darunter das Dock über `app._dragging()` —, und keine soll wissen müssen, wo der
Zustand liegt.

Wer ein Mixin herauslöst, prüft danach dreierlei: dass die Signatur die Abhängigkeiten
**nennt**, dass sich das Teil **mit Attrappen** bauen lässt (ohne Tk, Broker, BindStore),
und dass `AgentDeck` es nicht mehr einmischt — ein zusätzliches Mixin in einer Liste von
elf sieht sonst niemand.
- **Kommentare auf Deutsch**, wie der Rest des Repos. Sie erklären das *Warum* — das
  *Was* steht im Code.
- **Tests spiegeln `deck/`** — eine Datei je Modulbereich, benannt nach ihm
  (`test_dock_animation.py`, `test_claude_usage.py`). Sie fassen nur anzeigefreie
  Logik an und laufen **ohne pytest**: `tests/run.py` sammelt alle `test_*.py` ein,
  jede Datei ist aber auch einzeln aufrufbar. `tests/helpers.py` legt die Repo-Wurzel
  auf den `sys.path` und nagelt die Deck-Sprache auf Deutsch — ohne das hingen die
  Anzeige-Tests am echten `~/.claude/settings.json`. Darum importiert **jede**
  Testdatei `helpers`, auch wenn sie nichts daraus benutzt.
- Ein Testname beschreibt die Regel, nicht die Methode
  (`test_explizites_window_null_loescht_die_zuordnung`).
- **Keine neuen Abhängigkeiten** ohne Not. Außer Pillow kommt das Deck mit der
  Standardbibliothek aus; das ist Absicht und soll so bleiben.

## Der .NET-Port wurde verworfen

Es gab einen Portierungsversuch nach C#/.NET 9 mit WPF. Er ist am **2026-07-29**
vollständig verworfen worden: die Rechen-Schicht war portiert und gegen Python
golden-getestet, aber ausgerechnet die Module, die das Aussehen machen, fehlten — das
Ergebnis sah entsprechend aus.

Der Code liegt weiterhin im Commit `3fcddbc` unter `src/`. **Python ist die einzige
produktive Fassung.** Wer den Port wiederbeleben will, fängt bei der Zeichnerei an,
nicht bei der Mathematik.
