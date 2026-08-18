# Agent Deck

> Ein am Bildschirmrand angedocktes Dashboard für parallel laufende **Claude-Code-Agents**
> in VS Code. Eine Kachel je Chat, farbig nach Zustand — man sieht auf einen Blick,
> wer denkt, wer fertig ist und wer eine Rückfrage hat.

![Plattform](https://img.shields.io/badge/Plattform-Windows-0078d4)
![Python](https://img.shields.io/badge/Python-3.12%2B-3776ab)
![Tests](https://img.shields.io/badge/Tests-191%20passing-2ea44f)
![Lizenz](https://img.shields.io/badge/Lizenz-MIT-blue)

---

## Das Problem

Wer mehrere Claude-Code-Agents gleichzeitig laufen lässt, verliert den Überblick:
Welcher Chat wartet auf eine Freigabe? Welcher ist stehengeblieben? VS Code zeigt
Terminal-Tabs, aber keinen Zustand — und ein Klick auf den falschen Tab schickt
Text an den falschen Agenten.

Agent Deck löst das mit einem schmalen Panel, das am Bildschirmrand klebt und sich
bei Nichtgebrauch wegklappt. Jeder Chat ist eine Kachel; die Farbe ist der Zustand.
Klick auf die Kachel fokussiert **genau** dieses Terminal-Pane — ohne Fokus-Klau
und ohne Rätselraten.

## Architektur

Drei Prozesse, die sich über Dateien und einen TCP-Socket verständigen:

```mermaid
flowchart LR
    subgraph VSC["VS-Code-Fenster (A, B, …)"]
        T1["Terminal A1 · claude"]
        T2["Terminal A2 · claude"]
        EXT["Extension<br/>agent-deck-bridge (JS)"]
    end

    subgraph PANEL["Agent Deck · Panel (Python/Tk)"]
        BR["Broker<br/>TCP 127.0.0.1:8765"]
        UI["Kacheln · Glow · Edge-Dock"]
    end

    HOOKS["Claude-Code-Hooks<br/>report.py · statusline.py"]
    STATE[("%LOCALAPPDATA%\claude-agent-deck\state<br/>&lt;slot&gt;.json")]

    T1 -- "Hook-Event" --> HOOKS
    T2 -- "Hook-Event" --> HOOKS
    HOOKS -- "atomar schreiben" --> STATE
    STATE -- "Poll alle 400 ms" --> UI
    UI -- "Kommando" --> BR
    BR <-- "JSON-Zeilen" --> EXT
    EXT -- "sendText / terminal.show" --> T1
    EXT -- "sendText / terminal.show" --> T2
```

Warum diese Aufteilung:

| Aufgabe | Weg | Begründung |
|---|---|---|
| **Status** lesen | Claude-Code-Hooks → JSON-Datei | Hooks sind die einzige Quelle, die den echten Agent-Zustand kennt |
| **Pane** fokussieren | VS-Code-Extension | Win32/`SendInput` kann prinzipiell kein *einzelnes* Split-Pane treffen |
| **Fenster** nach vorn | Win32 `SetForegroundWindow` | die Extension kann ihr eigenes Fenster nicht aktivieren |
| Panel ↔ Extension | TCP, newline-getrennte JSON-Zeilen | kein Build-Step, keine Abhängigkeiten |

Das Wire-Vokabular liegt in [`protocol.py`](deck/domain/protocol.py). Es gibt bewusst keinen
Build-Step, darum spiegelt [`extension/extension.js`](extension/extension.js) die
Strings von Hand — wer dort etwas ändert, muss beide Seiten anfassen.

## Was drin steckt

- **Kachel je Chat**, Farbe = Zustand (idle · denkt · wartet auf dich · fertig · Verbindung verloren)
- **Ein Block je Repo**: Kopfzeile und Kachelreihe hängen sichtbar an derselben Schiene,
  und der Hover auf einer Karte lässt ihre Gruppe aufleuchten — bei mehreren offenen
  Repos die Frage, die man zuerst hat. Bewusst *keine* Farbe je Repo: der Farbkanal
  gehört dem Status
- **Am Rand andocken + Auto-Hide**: fährt auf Hover heraus, animiert über eine
  kritisch gedämpfte Feder (kein Overshoot bei randverankerten Panels)
- **Griff-Balken als Neon-Kapsel**: leuchtet in der Farbe des dringlichsten Status —
  man sieht bei zugeklapptem Deck, ob jemand etwas von einem will
- **Hover-Tooltip** mit KI-Kurzzusammenfassung des Chats (ein Satz, gecacht) plus
  selbst erkanntem Bezug: **Ticket** und **PR** per Regex aus dem Transcript, ohne Modellkosten.
  Darüber die Herkunft — Repo · Fenster · Slot, und bei Ticket-Arbeit der `worktree`, in
  dem der Agent wirklich sitzt (der ist sonst nirgends sichtbar)
- **Ticket → Worktree**: Ticket per Rechtsklick zuweisen, der Agent legt sich selbst
  einen `git worktree` an; beim Schließen der Kachel wird er wieder abgeräumt
- **Usage-Anzeige** des Kontos in der Bottom-Bar (Session · Woche · Modell-Woche),
  das Token wahlweise aus der Claude-Code-CLI oder aus Claude Desktop
- **Per-Monitor-DPI-V2**, damit auf 150 %-Displays nichts verwaschen wirkt
- **Drag & Drop** in beide Richtungen (VS Code gibt keine seiner Reihenfolgen preis,
  also führt das Deck seine eigene): Kacheln waagerecht in ihrer Reihe, ganze
  **Repo-Blöcke** senkrecht — angefasst am Repo-Namen, der als Klick weiterhin das
  VS-Code-Fenster nach vorn holt

## Voraussetzungen

| | |
|---|---|
| **Windows 10/11** | Das Deck ist Windows-only und wird es bleiben — es hängt an Win32 (Fensteridentität, `SetForegroundWindow`, Layered Windows, Per-Monitor-DPI). |
| **Python 3.12+** mit tkinter | Der Installer von python.org bringt tkinter mit; bei einer Store-Python kann es fehlen. |
| **VS Code** | Die Agenten laufen in dessen Terminals; das Deck spricht über eine kleine Extension mit ihnen. |
| **Claude Code CLI** | Angemeldet (`claude auth login`). Ohne sie gibt es nichts zu überwachen. |

Eine einzige Paket-Abhängigkeit: **Pillow** (Kachelflächen und Griff-Kapsel werden als
RGBA-Bilder komponiert). Alles andere ist Standardbibliothek — das ist Absicht und
soll so bleiben.

## Einrichtung

```powershell
git clone <repo> agent-deck; cd agent-deck
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Das ist alles. Der Installer prüft die Voraussetzungen, holt Pillow, kopiert die
Extension, merged die sechs Hooks **und die `statusLine`** in `~/.claude/settings.json`
— und **beweist** danach, dass ein Hook wirklich schreibt, indem er einen feuert und in
`state\` nach einer frischen Datei sieht. Dieser Beweis ist der Punkt: die
`cmd /c`-Falle endet mit Exit 0 und sieht darum gesund aus, meldet aber nichts.

Zwei Handgriffe bleiben, weil sie niemandem abgenommen werden können:

1. In jedem offenen VS-Code-Fenster: **„Developer: Reload Window"**
2. Im Panel oben auf **„Fenster A"** klicken, dann das VS-Code-Fenster anklicken

```powershell
.\install.ps1 -Check     # der Doctor: prüft alles, ändert nichts — erste Adresse bei Problemen
.\install.ps1 -Remove    # Hooks und Extension wieder entfernen
```

Ein zweiter Lauf ist ein Nulldurchgang: fremde Hooks anderer Werkzeuge bleiben stehen,
eigene werden ersetzt statt verdoppelt. Nach einem Repo-Umzug ist der Installer darum
auch der Weg, die Pfade zu reparieren.

Ausführlich — inklusive dem, was in `settings.json` landet und warum jeder Teil so
aussehen muss — in **[docs/SETUP.md](docs/SETUP.md)**.

Anfassen muss man sonst nur [`domain/config.py`](deck/domain/config.py) — und auch das nur, wenn man die
Vorgaben ändern will (Jira-Projekt-Key, Startmodus neuer Agenten, Abschalter für
Tooltip-Zusammenfassung und Ticket-Erkennung).

## Was das Deck nach außen tut

Ein Dashboard, das fremde Prozesse beobachtet, sollte offenlegen, was es anfasst.
Vollständig:

| Was | Wann | Warum |
|---|---|---|
| Liest `~/.claude/.credentials.json` und Claude Desktops `config.json` | einmal beim Start, danach nur, wenn die API das Token abweist | um das eigene OAuth-Token für den Usage-Abruf zu bekommen (siehe unten) |
| Ruft `https://api.anthropic.com/api/oauth/usage` ab | alle 2 Min., solange die Usage-Anzeige an ist | die Prozentzahlen in der Bottom-Bar |
| Startet `claude -p --safe-mode --model haiku` als Unterprozess | je offener Session einmal **im Voraus** (nicht erst beim Hovern — sonst wartet man 10 s), danach nur bei echtem Zuwachs, frühestens alle 45 s | die Ein-Satz-Zusammenfassung im Tooltip. **Kostet Tokens** auf deinem Konto — Haiku und gecacht, also Cent-Beträge. `HOVER_SUMMARY_PREFETCH = False` erzeugt sie erst beim Hovern, `HOVER_SUMMARY = False` schaltet sie ganz ab |
| Öffnet einen TCP-Listener auf `127.0.0.1:8765` | solange das Panel läuft | Panel ↔ VS-Code-Extension. Nur localhost, keine Authentifizierung — wer lokal Code ausführen kann, kann darüber Text in deine Terminals schicken |
| Legt `git worktree`s an und löscht sie | nur bei Ticket-Zuweisung per Rechtsklick | damit sich parallele Agenten am selben Repo nicht in die Quere kommen |

Sonst geht nichts nach draußen: keine Telemetrie, kein Update-Check, keine Konten
außer deinem eigenen.

## Usage-Anzeige: woher die Zahlen kommen

Die Prozentzahlen in der Bottom-Bar (Session, Woche, Modell-Woche) kommen von
`https://api.anthropic.com/api/oauth/usage` — demselben Endpunkt, den auch die
Claude-Oberflächen benutzen. Zwei Dinge sollte man dazu wissen:

**Der Endpunkt ist nicht dokumentiert.** Er gehört Anthropic, nicht diesem Projekt,
und kann sich jederzeit ändern. Bricht er, zeigt das Badge „—" und sonst passiert
nichts — [`claude_usage.py`](deck/claude/usage.py) ist durchgehend defensiv, ein Ausfall
kostet nie das Deck.

**Das Token kommt aus zwei Quellen**, beide werden gelesen und der Reihe nach
probiert:

1. **Claude Code CLI** — `~/.claude/.credentials.json`, Klartext-JSON. Der Normalfall:
   wer das Deck benutzt, hat die CLI zwingend installiert.
2. **Claude Desktop** — dessen `config.json`, verschlüsselt als Chromium-`v10`-Blob
   (AES-256-GCM, Schlüssel per Windows-DPAPI aus `Local State`). Entschlüsselt wird
   über Windows CNG (`bcrypt.dll`), damit keine Krypto-Abhängigkeit nötig ist.

Ist eins abgelaufen oder wird es mit 401 abgewiesen, trägt das andere weiter. Fehlen
beide, steht im Tooltip „Nicht angemeldet – `claude auth login`". **Claude Desktop ist
also nicht nötig** — die CLI allein genügt.

Das Token wird ausschließlich für diesen einen Abruf verwendet, nirgends
hingeschrieben und an niemanden sonst gesendet — es steht nur im Arbeitsspeicher des
Panels. Wer das nicht will, setzt `SHOW_USAGE = False` in
[`domain/config.py`](deck/domain/config.py); dann wird keine der beiden Dateien je angefasst und das
Deck läuft ansonsten vollständig.

## Projektstruktur

Der Code liegt im Paket `deck/`, geschnitten in Schichten — 73 Module, keines über 400
Zeilen. Abhängigkeiten zeigen **nur nach unten**, und das ist keine Absichtserklärung,
sondern [getestet](tests/test_architecture.py): ein Import nach oben macht die Suite rot
und nennt Datei und Zeile.

Im Wurzelverzeichnis liegen nur fünf Einsprungpunkte mit je einem `runpy`-Aufruf — sie
sind **kein** `python -m`-Layout, weil `restart()` sich über `sys.argv[0]` neu startet.
Ihre Namen sind Verträge: `report.py` und `statusline.py` stehen mit absolutem Pfad in
`~/.claude/settings.json`, `agent_deck.py` in `start.bat` und im Wächter, `watchdog.py`
in der Windows-Aufgabenplanung, `reenable_glow.py` in dieser Doku. Dass es sie gibt und
dass sie nur den `runpy`-Aufruf enthalten, ist ebenfalls
[getestet](tests/test_architecture.py) — ein umbenannter Einsprungpunkt bricht sonst
still jede bestehende Installation.

Dazu **`install.ps1`** als einziger Einstieg von außen (siehe [Einrichtung](#einrichtung)).
Was es in `settings.json` schreibt, rechnet [`claude/hook_setup.py`](deck/claude/hook_setup.py)
aus — damit die Merge-Regeln und die Prüfurteile unter Test stehen und nicht in einem
Shell-Skript wohnen.

<details>
<summary><b>deck/domain</b> — anzeigefreier Kern, ohne tkinter/Pillow/ctypes</summary>

| Datei | Aufgabe |
|---|---|
| `status_model.py` | reine Statusinterpretation |
| `paths.py` | Repo-Wurzel, State-Ordner, atomares JSON |
| `protocol.py` | Wire-Vokabular (die eine Quelle der Wahrheit) |
| `slot_state.py` | Slot-Zustände lesen |
| `binding.py` | Bindings, Settings, Tickets, Reihenfolge |
| `ordering.py` | Reihenfolge zusammenführen · Zielposition und Lücke beim Ziehen |
| `config.py` | Vorgaben und Schalter |

</details>

<details>
<summary><b>deck/platform</b> · <b>deck/render</b> — Win32 und Zeichnerei</summary>

| Datei | Aufgabe |
|---|---|
| `platform/win32.py` | DLL-Handles und **typisierte** ctypes-Signaturen (Grundlage) |
| `platform/focus.py` | Fenster finden, nach vorn holen, Titelleiste stylen |
| `platform/layered.py` | Per-Pixel-Alpha — schiebt das Kapselbild ins Fenster |
| `platform/clip.py` | Fenster an der Bildschirmkante beschneiden |
| `platform/timing.py` | Timer-Auflösung und Bildwiederholrate |
| `platform/dpi.py` · `platform/monitor.py` | DPI-Skalierung · Fenster im Monitor halten |
| `render/kit.py` | Palette, Zeichen-Primitive, pure Farb-/Text-Helfer |
| `render/card.py` | Kachelfläche und Halo (Pillow, Masken-Cache) |
| `render/capsule.py` | Griff-Kapsel einfärben und zusammensetzen |
| `render/capsule_masks.py` | Lage, Maße und Masken der Kapsel |
| `render/fluid.py` | Schwappen im Kern der Kapsel |
| `render/glow.py` | Puls, Crossfade, Bloom, Press-Pop |

</details>

<details>
<summary><b>deck/claude</b> · <b>deck/net</b> — Claude-Code-Spezifisches und die Brücke</summary>

| Datei | Aufgabe |
|---|---|
| `claude/hooks/report.py` · `statusline.py` | die Hooks (schreiben den Slot-Status) |
| `claude/hooks/resolve.py` | Slot-Auflösung über die Prozesskette |
| `claude/usage.py` | Usage-Abruf und Hintergrund-Poller |
| `claude/usage_token.py` | Token aus CLI oder Claude Desktop (DPAPI + AES-GCM über Windows CNG, dependency-frei) |
| `claude/usage_view.py` | Antwort auswerten, Ampelfarben, Tooltip-Text (pur) |
| `claude/summarize.py` | Kurzzusammenfassung des Chats |
| `claude/refs.py` | Ticket- und PR-Nummer aus dem Chat (reine Regex) |
| `claude/settings.py` | `~/.claude/settings.json` lesen |
| `claude/hook_setup.py` | dieselbe Datei **einrichten**: Hooks mergen, prüfen, entfernen (was `install.ps1` aufruft) |
| `net/broker.py` · `net/commands.py` | TCP-Server · Fassade über die Wire-Dicts |
| `extension/` | VS-Code-Extension (reines JS, kein Build) |

</details>

<details>
<summary><b>deck/dock</b> — Andocken, Griff, Animation</summary>

| Datei | Aufgabe |
|---|---|
| `controller.py` | Zustand und öffentliche API des Randdocks |
| `metrics.py` | Maße, Farben, Takte + Umrechnungen |
| `animation.py` | Slide und Landung — **hier liegen die drei Sicherungen** |
| `handle.py` | Griff-Fenster: Zonen, Hover, Ziehen, zeichnen |
| `wave.py` | Schwappen und Atmen des Neons |
| `poll.py` · `reveal.py` · `frameless.py` · `clipping.py` · `geometry.py` | Zeiger-Poll · Auf-/Zuklappen · rahmenlos · Kanten-Clip · Rechnerei |

</details>

<details>
<summary><b>deck/ui</b> · <b>deck/ops</b> — Panel und Betrieb</summary>

| Datei | Aufgabe |
|---|---|
| `ui/panel.py` | Fenster, Aufbau, Hauptschleife (mischt 13 Mixins) |
| `ui/theme.py` | Farben, Timings, Anzeigetexte |
| `ui/layout.py` | Fenstergröße und Skalierung |
| `ui/tiles.py` · `ui/tile_draw.py` | Kacheln anordnen · eine Kachel zeichnen |
| `ui/reorder.py` · `ui/reorder_blocks.py` | Kacheln umsortieren (waagerecht) · Repo-Blöcke (senkrecht) |
| `ui/refresh.py` · `ui/windows.py` | Poll-Takt · Bindungen und Fenster-Sync |
| `ui/hover.py` · `ui/connect.py` · `ui/actions.py` | Tooltip · Fenster binden · Klick-Wirkungen |
| `ui/ticket.py` · `ui/worktree_sweep.py` | Ticket zuweisen · worktrees abräumen |
| `ui/settings_dialog.py` | Einstellungen |
| `ui/bottombar.py` | Dauer-UI: Usage links, Einstellungen rechts |
| `ui/uithread.py` | Rückweg vom Daemon-Thread auf den Tk-Thread |
| `ops/log.py` | Logging (`pythonw` hat kein stderr) |
| `ops/instance.py` · `ops/watchdog.py` | Zweitstart-Guard · Neustart-Wächter |
| `ops/worktree.py` | verwaiste `git worktree`s abräumen |
| `ops/vscode_glow.py` | Custom-CSS nach einem VS-Code-Update neu injizieren |
| `deck/i18n.py` | Deutsch/Englisch (Querschnitt, liegt auf der Paketwurzel) |

</details>

## Tests

Die anzeigefreie Logik ist unit-getestet — Statusmodell, Ticket-/PR-Erkennung,
Farb- und Text-Helfer, Worktree-Parsing, Usage-Auswertung, Watchdog-Urteile:

```powershell
python tests/run.py                   # alle 191 Tests, kein pytest nötig
python tests/test_dock_animation.py   # eine Datei allein läuft auch
python -m pytest tests/               # geht ebenfalls
```

Stand: **191/191** in 23 Dateien, die `deck/` spiegeln. Läuft in der
[CI](.github/workflows/ci.yml) bei jedem Push, dort zusammen mit einer Syntaxprüfung
aller Module (`python -m compileall`) und einem Parse-Lauf über die `.ps1`-Dateien —
ein Tippfehler in `install.ps1` fällt sonst erst dem auf, der das Repo gerade zum
ersten Mal klont, also genau dem falschen.

Eine Datei prüft nicht Verhalten, sondern die **Struktur**
([`test_architecture.py`](tests/test_architecture.py)): dass kein Import nach oben
zeigt, dass jeder `from deck.x import y` ein existierendes `y` trifft, dass `domain/`
ohne `tkinter`/`PIL`/`ctypes` bleibt, dass die Hooks nicht die Anzeige nachziehen (sie
starten bei jedem Tool-Aufruf neu), dass die fünf Einsprungpunkte da sind und dass im
Wurzelverzeichnis kein Streumüll liegt. Eine Schichttabelle in der Doku veraltet still;
diese Tests werden rot und nennen Datei und Zeile.

Der Hook-Merge hat seine eigene Datei
([`test_claude_hook_setup.py`](tests/test_claude_hook_setup.py)) — er fasst die eine
Stelle an, an der ein Fehler den **Agenten blockiert**: ein kaputter Eintrag unter
`UserPromptSubmit` gilt Claude Code als Veto gegen den Prompt. Geprüft wird deshalb
nicht nur, dass er das Richtige schreibt, sondern auch, dass er fremde Hooks stehen
lässt und dass ein zweiter Lauf nichts verändert.

## Bekannte Grenzen

Ehrlicher als eine Feature-Liste — das hier ist der Stand, nicht ein Versprechen:

- **Nur Kacheln von Chats, die das Deck selbst angelegt hat, färben sich.** Die Farbe
  kommt von den Hooks, und die brauchen `AGENT_SLOT` in der Umgebung des Terminals.
  Von Hand gestartete Sessions erkennt die Extension zwar und man kann sie anklicken
  und steuern — sie bleiben aber grau.
- **`AskUserQuestion` und die Plan-Mode-Rückfrage melden keinen sauberen Hook.**
  Die Kachel zeigt dann nicht „wartet", obwohl der Agent wartet. Das ist die
  ärgerlichste offene Lücke.
- **Fenster-nach-vorn ist „best effort".** Windows verweigert `SetForegroundWindow`
  je nach Fokus-Situation und blinkt stattdessen nur den Taskbar-Button.
- **Der optionale Glow um das fokussierte Terminal patcht VS Codes `workbench.html`.**
  Deshalb warnt VS Code danach „Your Code installation appears to be corrupt", und
  jedes VS-Code-Update wirft den Patch wieder heraus (`python reenable_glow.py` holt
  ihn zurück). Wer das nicht will, lässt den Schritt in SETUP.md einfach aus — er ist
  optional und hat mit dem Deck selbst nichts zu tun.
- **Der Broker lauscht ohne Authentifizierung auf `127.0.0.1:8765`.** Für localhost
  ist das die übliche Abwägung, aber sie sei genannt.

## Ein verworfener .NET-Port

Der Commit `3fcddbc` enthält unter `src/` einen Portierungsversuch nach C#/.NET 9 mit
WPF. Er ist am 2026-07-29 verworfen worden, und die Lehre daraus ist notiert, weil sie
sich leicht wiederholen lässt: portiert und golden-getestet war die *Mathematik*
(Statusmodell, Andock-Rechnerei, Feder, Broker, Hooks) — es fehlte die *Zeichnerei*,
also genau das, was man sieht. Das Ergebnis sah entsprechend aus.

Python ist die einzige produktive Fassung.

## Lizenz

[MIT](LICENSE)
