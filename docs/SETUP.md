# Agent Deck – Einrichtung

Dashboard für Claude-Agents in VS-Code-Fenstern. Startet **ohne** Agent-Kacheln;
pro verbundenem Fenster erscheint **eine Kachel je offenem Claude-Terminal** plus
eine **„＋"-Kachel**, die einen weiteren Claude-Chat öffnet. Wächst/schrumpft
automatisch mit den tatsächlich offenen Chats.

**Architektur (recherchiert & verifiziert):**
- **STATUS** = Claude-Code-Hooks → `state/<slot>.json`, vom Panel gelesen.
- **ACTIONS + Pane-Fokus** = kleine **VS-Code-Extension** (`Terminal.sendText` / `terminal.show`) — trifft exakt den richtigen Split-Pane **ohne Fokus-Klau**. (Win32/SendInput *kann* einen einzelnen Pane prinzipiell nicht treffen.)
- **Fenster verbinden** = per Klick im Panel; das richtige Fenster wird per Win32 `SetForegroundWindow` nach vorn geholt.
- **Broker** = schlanker TCP-Server im Panel, über den Panel ↔ Extensions reden.

Panel + Broker: reine Python-Stdlib. Extension: reines JS (kein Build).

---

## Schritt 1 – `install.ps1` (das ist die Einrichtung)

Im Repo-Wurzelverzeichnis:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Das Skript erledigt in einem Lauf alles, was früher Handarbeit war:

| | |
|---|---|
| **prüft** | Python 3.12+, tkinter (fehlt bei der Store-Python!), VS Code, `claude` auf dem PATH, Anmeldung |
| **holt** | Pillow (`requirements.txt`) |
| **kopiert** | die Extension nach `~/.vscode/extensions/agent-deck-bridge` |
| **registriert** | sie in VS Codes `extensions.json` — der Ordner allein genügt nicht, geladen wird nur, was dort steht |
| **merged** | die sechs Hooks **und die `statusLine`** in `~/.claude/settings.json` — mit absolutem Pfad und `\|\| exit 0` |
| **beweist** | dass ein Hook wirklich schreibt: er wird gefeuert, und danach muss eine Datei in `state\` frisch sein |
| **startet** | das Panel (falls nicht schon eins läuft) |

Der letzte Punkt ist der wichtigste. **Exit-Code 0 beweist bei einem Hook nichts** — die
`cmd /c`-Falle (unten) endet mit 0 und sieht darum gesund aus. Bewiesen ist es erst,
wenn in `state\` eine Datei frisch wird, und genau das prüft Schritt 5 des Skripts.

Ein zweiter Lauf ist ein **Nulldurchgang**: fremde Hooks anderer Werkzeuge bleiben
stehen, eigene werden ersetzt statt verdoppelt. Man kann das Skript also jederzeit
wieder aufrufen — nach einem Repo-Umzug ist es sogar der Weg, die Pfade zu reparieren.

```powershell
.\install.ps1 -Check     # nur prüfen und berichten (der Doctor), ändert nichts
.\install.ps1 -Remove    # Hooks und Extension wieder entfernen
.\install.ps1 -Force     # auch eine FREMDE statusLine ersetzen
.\install.ps1 -NoStart   # Panel am Ende nicht starten
```

**`-Check` ist die erste Adresse, wenn etwas nicht geht.** Er nennt jeden Befund
einzeln — fehlender Hook, `cmd /c`, fehlendes `|| exit 0`, Pfad ins Leere, Pfad in ein
anderes Repo, veraltete installierte Extension (ein Fehlerbild, das schon zweimal hinter
„verbindet nicht mehr" stand) und die Extension, die zwar an ihrem Platz liegt, aber in
VS Codes `extensions.json` **nicht registriert** ist bzw. deren Eintrag auf einen
umbenannten Ordner zeigt.

Dieser letzte Befund ist am 2026-07-30 dazugekommen, weil er vorher durchfiel: der
Eintrag zeigte auf `agent-deck-bridge.testbackup`, VS Code meldete beim Start einmal
`Unable to read file '…\package.json'` und lud die Extension nicht — während `-Check`
grün „installiert und aktuell" sagte, denn Ordner und Hash stimmten ja. Geradebiegen tut
das ein gewöhnlicher `.\install.ps1`-Lauf; danach in **jedem** Fenster „Developer: Reload
Window".

> Kein `agentDeck.window`-Setting und kein Ordnername in `deck/domain/config.py` von Hand
> nötig — welches Fenster A bzw. B ist, legst du **im Panel per Klick** fest (Schritt 3).

## Schritt 2 – In jedem offenen VS-Code-Fenster: „Developer: Reload Window"

Command Palette → **„Developer: Reload Window"**. Erst danach läuft die kopierte
Extension; ein Reload gilt **pro Fenster**, alter Code läuft in den anderen weiter.

Sobald ein Fenster neu geladen ist, geht im Panel sein Punkt an (**`Extension A:●`**).

> Reload startet die Terminals **dieses** Fensters neu (inkl. laufender Claude-Session).

## Schritt 3 – Fenster verbinden / umbinden (per Klick)

Falls ein Punkt grau bleibt oder du ein anderes Fenster zuordnen willst:
1. Im Panel oben auf **„Fenster A"** (bzw. „Fenster B") klicken → es steht „… VS Code klicken".
2. Das gewünschte **VS-Code-Fenster anklicken**.
3. Der Repo-Name erscheint auf dem Button — verbunden. (Nochmal klicken = neu verbinden.)

Die Zuordnung wird in `bindings.json` gemerkt und beim nächsten Start automatisch wiederhergestellt.

## Schritt 4 – Chats anlegen & erkennen

Sobald ein Fenster verbunden ist, erscheint dort eine **„＋"-Kachel**. Ein Klick
darauf öffnet **ein** neues Claude-Terminal (`A1`, dann `A2`, … – der Index wächst
automatisch), setzt `AGENT_SLOT` und startet `claude`. Für jeden weiteren Chat
nochmal „＋" klicken.

**Automatische Erkennung schon offener Sessions:** Die Extension findet Claude-
Terminals jetzt daran, **was darin läuft**, nicht nur am Namen. Ein Terminal wird
als Claude-Chat erkannt, wenn
1. darin per Shell-Integration `claude` gestartet wurde, **oder**
2. unter seinem Shell-Prozess ein `claude`-Prozess läuft (Prozess-Scan, Windows), **oder**
3. sein Name danach aussieht (`A1`, `A2`, … oder enthält „claude").

So tauchen auch Chats auf, die ein **anderer Chat** oder du **von Hand** gestartet
hast – ohne dass sie über das Deck angelegt wurden.

> **Wichtig zum Status:** Kacheln von Chats, die das Deck **selbst** per „＋"
> angelegt hat, färben sich live (sie haben `AGENT_SLOT`). Von außen gestartete,
> nur erkannte Sessions sind **anklickbar/steuerbar** (fokussieren, Approve/Reject,
> Slash-Kommandos), bleiben aber **grau**, weil die Hooks ohne `AGENT_SLOT` keinen
> Status melden können. Wer auch dort Farbe will, startet den Chat über „＋".

> Die alte Sammel-Aktion „4 Splits auf einmal" gibt es weiter als VS-Code-Befehl
> „Agent Deck: Agent-Terminals anlegen (4 Splits)", ist aber nicht mehr im Panel.

---

## Schritt 5 – Nutzungsanzeige (läuft von allein)

Unten links zeigt das Deck die Auslastung deines Kontos als Ampel-Badge (Session,
Hover → Woche und Modell-Limits, Klick → `claude.ai/settings/usage`). Dafür ist
**nichts einzurichten**: das OAuth-Token holt sich das Deck aus der Claude-Code-CLI
(`~/.claude/.credentials.json`) oder, falls vorhanden, aus Claude Desktop. Eine der
beiden Quellen genügt, und die CLI hat jeder, der das Deck benutzt.

Wenn dort dauerhaft „—" steht, sagt der Hover-Tooltip warum:

| Tooltip | Bedeutung |
|---|---|
| `Nicht angemeldet – 'claude auth login'` | Keine Quelle hat ein Token hergegeben. Einmal `claude auth login` ausführen. |
| `Token ungueltig – 'claude auth login'` | Beide Tokens wurden mit 401/403 abgewiesen (abgelaufen oder Konto gewechselt). |
| `Rate-Limit – kurz warten` | Zu viele Abrufe. Der letzte gültige Wert bleibt stehen; der Takt fällt automatisch zurück. |

> Der Endpunkt (`api.anthropic.com/api/oauth/usage`) ist **nicht dokumentiert** und
> gehört Anthropic. Ändert er sich, zeigt das Badge „—" und sonst passiert nichts —
> die Datenschicht ist durchgehend defensiv. Abschalten mit `SHOW_USAGE = False` in
> `deck/domain/config.py`; dann wird auch keine der Token-Dateien angefasst.

---

## Anhang – was in `~/.claude/settings.json` landet

Der Installer schreibt das; hier steht es, damit man es **nachlesen und prüfen** kann
(und für den Fall, dass man es doch von Hand machen will). Global, weil die VS-Code-
Fenster verschiedene Ordner haben. `PFAD` = Repo-Wurzel.

```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "python \"PFAD\\report.py\" idle || exit 0" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "python \"PFAD\\report.py\" thinking || exit 0" }] }],
    "PreToolUse":  [{ "matcher": "*", "hooks": [{ "type": "command", "command": "python \"PFAD\\report.py\" running || exit 0" }] }],
    "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "python \"PFAD\\report.py\" thinking || exit 0" }] }],
    "Notification": [{ "hooks": [{ "type": "command", "command": "python \"PFAD\\report.py\" waiting || exit 0" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "python \"PFAD\\report.py\" done || exit 0" }] }]
  },
  "statusLine": { "type": "command", "command": "python \"PFAD\\statusline.py\"", "refreshInterval": 15 }
}
```

Die **`statusLine` ist kein Beiwerk**: sie liefert Modell, Effort, Kontext-% und Kosten
in `state\<slot>.live.json`, und nur daraus zeigt die Kachel diese Werte. Ohne sie läuft
das Deck vollständig — die Felder bleiben aber leer, ohne dass irgendwo ein Fehler
auftaucht. (Genau das war jahrelang die Lücke in dieser Anleitung.)

`refreshInterval` lässt Claude Code die Zeile alle 15 s neu zeichnen, auch wenn im Chat
nichts passiert. Sonst stünde der Countdown des Prompt-Caches (`cache 42 min`) genau im
Leerlauf still, also dann, wenn er abläuft.

Warum die Einträge so und nicht anders aussehen — jeder Punkt ist einmal wehgetan:

- **`|| exit 0` nicht weglassen.** Ein Hook, der gar nicht startet (Datei verschoben,
  `python` nicht auf dem PATH), kommt an sein eigenes Fangnetz nicht heran — und Exit ≠ 0
  gilt bei `UserPromptSubmit`/`PreToolUse` als Veto gegen Prompt bzw. Tool-Aufruf.
- **Kein `cmd /c` davorsetzen.** Claude Code führt Hooks über eine POSIX-Shell aus; deren
  Pfadkonvertierung macht aus `/c` den Pfad `C:\`, `cmd` startet interaktiv und ruft
  `python` nie auf. Der Hook endet dann mit 0 und sieht gesund aus, meldet aber nichts.
  `install.ps1 -Check` findet beides.
- **npm-`claude` (`claude.cmd`)** benutzen, nicht eine native `claude.exe` auf dem PATH ([#25577](https://github.com/anthropics/claude-code/issues/25577)).
- Hook direkt über **`python`** (nicht `bash`) → umgeht die Git-Bash-Fallen unter Windows.
- **Kein `idle_prompt`** verwenden. „wartet" = `Notification`, „fertig" = `Stop`.
- **`SessionStart`** meldet einen frisch geöffneten Agenten sofort (Kachel wird gleich „idle" statt grau/leer). Nötig für den automatischen Start-Modus neuer Agenten (`config.NEW_AGENT_MODE`) – ohne ihn schaltet der Modus erst beim ersten Prompt. Für Nicht-Deck-Sessions (ohne `AGENT_SLOT`) tut der Hook nichts.

`AGENT_SLOT` musst du **nicht** setzen — die Extension setzt es beim Anlegen der
Terminals (Schritt 4).

Beim Umbenennen oder Verschieben des Repos gilt: **erst den neuen Pfad beweisen, dann
den alten löschen**, nie umgekehrt. `install.ps1` macht genau das — es ersetzt die alten
Einträge und führt danach den Schreibbeweis.

---

## Optional – Der Wächter (hält das Panel am Leben)

```powershell
powershell -ExecutionPolicy Bypass -File .\install_watchdog.ps1
```

Registriert einen kurzen Lauf in der Aufgabenplanung (bei Anmeldung + alle 3 Minuten),
der das Panel nach einem Absturz neu startet. Braucht keine Admin-Rechte.
`-Autostart` legt stattdessen eine Verknüpfung im Autostart-Ordner an, `-Remove`
entfernt beides wieder.

---

## Optional – Visuelle Indikation (Glow um den fokussierten Chat)

Beim Fokussieren eines Chats läuft ein Lichtpunkt zweimal am Terminal-Rand entlang
und lässt einen leuchtenden Rahmen zurück (`agent-deck-glow.css`, „Border-Beam").

**Einmalige Einrichtung:**
1. Extension **„Custom CSS and JS Loader"** (`be5invis.vscode-custom-css`) installieren.
2. In `settings.json` (global) auf die CSS zeigen:
   ```json
   "vscode_custom_css.imports": [
     "file:///C:/Pfad/zu/agent-deck/agent-deck-glow.css"
   ]
   ```
3. Command Palette → **„Enable Custom CSS and JS"** → **Reload Window**.

**Nach JEDEM VS-Code-Update ist der Glow weg** – das Update ersetzt VS Codes
`workbench.html` (liegt unter Windows inzwischen in einem versionierten Hash-Ordner,
`…\Microsoft VS Code\<hash>\resources\app\out\vs\code\electron-browser\workbench\`)
und wirft damit den injizierten `<style>`-Block raus. Die `settings.json` bleibt
korrekt – es fehlt nur die Injektion. Zwei Wege, ihn zurückzuholen:

- **Extension:** Command Palette → „Enable Custom CSS and JS" → Reload Window.
- **Skript (schneller, scriptbar):**
  ```powershell
  python reenable_glow.py        # patcht die aktuelle workbench.html neu
  python reenable_glow.py --off  # Patch wieder entfernen (Backup zurück)
  ```
  Danach Command Palette → **„Developer: Reload Window"**. Das Skript findet die
  aktuelle `workbench.html` selbst (auch im Hash-Ordner), liest `vscode_custom_css.imports`
  aus der `settings.json` und injiziert byte-gleich wie die Extension (inkl. Backup).

> Reload startet die Terminals **dieses** Fensters neu (inkl. laufender Claude-Session)
> → ggf. das andere Fenster zuerst neu laden.
> VS Code zeigt danach evtl. „Your Code installation appears to be corrupt" – das ist
> bei Custom-CSS **normal** (geänderte `workbench.html`); mit „Don't Show Again" wegklicken.

---

## Bedienung

- **„Fenster A/B" klicken** → dann VS-Code-Fenster anklicken = verbinden/umbinden.
- **„＋"-Kachel klicken** → die Extension öffnet ein weiteres Claude-Terminal in dem Fenster.
- **Kachel klicken** → Fenster nach vorn + Pane fokussiert → tippen oder diktieren.
- **Über eine Kachel fahren (Hover)** → ein Tooltip zeigt **worauf sich der Chat bezieht**
  (`Ticket: PROJ-2691 · PR #62`) und darunter eine **KI-Kurzzusammenfassung** („worum es
  geht", ein Satz, aus dem Transcript per `claude --safe-mode haiku`, gecacht). Ticket und
  PR liest das Deck selbst aus dem Gespräch (Regex, kein Modell/keine Kosten):
  - **Ticket:** Keys der Form `ABC-123`, das Projekt aus `JIRA_PROJECT_KEY` auch klein
    geschrieben (z. B. in `ticket/proj-2691`), Nennungen wie „Ticket 2701".
  - **PR:** „PR #62", „pull request 62", „merge request 62", PR-URLs (GitHub `…/pull/62`,
    GitLab `…/-/merge_requests/62`) und ein bloßes „#62", wenn es mehrfach fällt.

  Beides steht zusätzlich **gedimmt auf der Karte** (`PROJ-2691 #62`), solange dort kein
  per Rechtsklick zugewiesenes Ticket (mit worktree) steht; passt es nicht in die schmale
  Zeile, gewinnt das Ticket. Abschaltbar über `TICKET_AUTODETECT` /
  `TICKET_AUTODETECT_ON_CARD` (und `HOVER_SUMMARY = False` → Tooltip zeigt wieder nur die
  zuletzt gestellte Frage) in `deck/domain/config.py`. Grau erkannte Sessions ohne Hook-Meldung zeigen
  nichts.
- **🔔 Nächster** → springt zum Agent mit Status „wartet".
- **✅ Approve / ❌ Reject** → Enter / Esc an den zuletzt gewählten Agent (ohne Fokus-Klau).
- **🧹 /clear · 🧠 /model opus[1m] (neuestes Opus, 1M-Kontext) · 📖 /model fable** → Slash-Kommando an den aktiven Agent. Die Werte sind bewusst Aliasse ohne Versionsnummer — Claude Code nimmt damit immer das jeweils neueste Modell.
- **📢 Alle** → Text an alle Agenten broadcasten.

### Bekannte Rest-Risiken (ehrlich)
- Fenster-nach-vorn (`SetForegroundWindow`) ist „best effort" — Windows blinkt evtl. nur den Taskbar-Button.
- `Notification`=„wartet" ist zuverlässig, aber leicht verzögert (interne Debounce), nicht instant.
- **Lücke:** `AskUserQuestion` / Plan-Mode-„proceed?" melden keinen sauberen Hook → Kachel zeigt dann nicht „wartet". Fix (Transcript-Tail) ist der nächste Ausbauschritt.
