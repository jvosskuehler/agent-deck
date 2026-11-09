# Gestaltung des Agent Decks

Verbindlich. Aufbau und Aufteilung gewählt von Jorrit am 06.11.2026 im Design-Canvas
(Seite „Runde 3“): **Aufteilung der Karte aus Variante 05.** Der Look ist seit 09.11.2026
der **Papier-Look, Variante „02 Farbfeld“** (Canvas
https://claude.ai/artifact/HDeLhN3i7DADxWqu4EoPzz), passend zu den Terminal-Agenten
(Windows-Terminal-Profil „Agent Deck“: Rosé Pine Dawn / Rosé Pine, IBM Plex Mono). Die
Vorlagen liegen als HTML in `docs/design-refs/` und enthalten die exakten Werte:

| Datei | Wofür |
|---|---|
| `papier-farbfeld02.html` | Farben, Kartenfläche, Atmen, Panel, Blockkopf, Leiste, Schrift (hell und dunkel) |
| `karte-aufteilung-deck05.html` | wo was auf der Karte steht |
| `karte-farbe-deck01.html` | Maße von Panel, Schiene, Leiste und Griff (die Farben daraus gelten nicht mehr) |
| `einstellungen-glas.html` | Aufbau der Einstellungen (rechte Spalte `aside`), in den Papier-Look übertragen |

Grundsatz: **Papier statt Schwarz, die ganze Karte ist ein Feld in der Statusfarbe, ohne
Rand und ohne Leuchten.** Kein Neon-, Raumschiff- oder Glas-Look, keine Deko-Effekte. Der
Aufbau ist derselbe wie im Python-Deck: oben angedockt, Repo-Blöcke, vier Karten je Reihe,
＋ am Reihenende, unten Nutzung und Einstellungen, eingeklappt nur der Griff.

**Hell oder dunkel folgt dem System** (`prefers-color-scheme`; die Fenster setzen kein
festes Thema), wie das Terminal-Profil. Hell = Rosé Pine Dawn, dunkel = Rosé Pine. Die
festen Farben stehen als CSS-Variablen in `app/src/styles/base.css`, die je Zustand
gerechneten in `app/src/logic/palette.ts`.

## Farben

Statusflächen nach Variante „01 Klar“ (09.11.2026): jede Fläche ist je Modus fest gemischt, nicht mehr Pigment mit Alpha über dem Kartengrund (auf Graphit wurde Gold so braun, Grün oliv). Pigmente fürs Atmen, Aufblühen, Griff und Punkte: hell `#e8813f` `#4f86c6` `#4fa36a` `#c4536f`, dunkel `#f5a27c` `#8db9ee` `#86d2a3` `#f08a9e`. Früher hatte jede Statusfarbe zwei Stärken: **Pigment** für Flächen, Griff und Punkte, **Ink** für
Text in dieser Farbe. Auf Creme ist das Pigment als Schrift zu hell, Ink ist dort dunkler
(Kontrast ≥ 4,5 : 1 auf der getönten Karte); dunkel trägt das Pigment den Text selbst.
Hell ist Rosé Pine Dawn wie das Terminal-Schema „papier-ebenen“; Rosé Pine hat kein Grün, ungelesen bekommt ein ergänztes Salbeigrün. Dunkel ist „Graphit“ (seit 09.11.2026) wie „papier-ebenen-nacht“: warmes Graphit statt Lila, Schrift in Papierfarbe.

| Zustand | hell Fläche / Schrift | dunkel Fläche / Schrift | Text DE / EN | Fläche |
|---|---|---|---|---|
| Rückfrage (`waiting`) | `#fce2cf` / `#a4441c` | `#4b2b1f` / `#f5a27c` | Rückfrage / needs input | getönt, atmet |
| denkt (`thinking`) | `#dce7f6` / `#2d5d95` | `#213349` / `#8db9ee` | denkt / thinking | getönt, atmet |
| ungelesen (`done`) | `#d8eedd` / `#2c7247` | `#1c392d` / `#86d2a3` | ungelesen / unread | getönt, ruhig |
| getrennt (`lost`) | `#f5e0e4` / `#a13d59` | `#3a2528` / `#f08a9e` | getrennt / disconnected | getönt, ruhig |
| idle | `#efe7df` / `#6e6a86` | `#2b2927` / `#a9a39b` | idle / idle | Overlay |
| unbekannt (`unknown`, Remote) | wie idle, Karte matt | wie idle, Karte matt | unbekannt / unknown | Overlay |

| Rolle | hell | dunkel |
|---|---|---|
| Panel | `#faf4ed` | `#1d1c1b` |
| Kartengrund | `#fffaf3` | `#242220` |
| Overlay (ruhige Karte, Segmente) | `#f2e9e1` | `#2b2927` |
| Text | `#575279` | `#e6e0d6` |
| Modell · Effort, Modus | `#575279` | `#cfc9bf` |
| gedämpft (Host, Leiste) | `#6e6a86` | `#8e8a84` |
| Ticket/PR (nur erkannt: gedimmt) | `#6a5590` | `#bda2e0` |
| Modus Bypass | `#8f3f58` | `#f497a8` |
| Haarlinien (hl-med / hl-high) | `#dfdad9` / `#cecacd` | `#3a3734` / `#4a4744` |

## Karte

- Breite 197, Abstand 10, vier je Reihe. Radius 14, kein Rand.
- **Windows-Terminal-Karte** 197 × 118, Innenabstand 10 12 9:
  oben links Statustext (11 px, fett, Ink), oben rechts „Modell · Effort“ (10,5 px,
  normal); darunter der **Sessiontitel** (IBM Plex Sans 14 px, halbfett, Zeilenhöhe 1,3,
  Textfarbe, höchstens zwei Zeilen, sonst Auslassung); unten links Ticket/PR (10,5 px,
  fett), unten rechts Modus (10,5 px, mittel).
- **VS-Code-Karte** 197 × 58 (flach, kein Titel), Innenabstand 7 12: Zeile 1 wie oben,
  Zeile 2 Ticket/PR links, Modus rechts.
- Fläche: Pigment mit Hex-Alpha `38` (dunkel `30`) über dem Kartengrund; idle, none und
  unbekannt liegen stattdessen auf dem Overlay. Zustandswechsel blenden in 400 ms über.
- Atmen als eigene Ebene: bei Rückfrage und denkt Pigment `24` (dunkel `1e`), Deckkraft
  0,15 ↔ 1 in 3 s, sanft, alle Karten im selben Takt. Ungelesen, getrennt, idle: ruhig.
- Statuswechsel nach Rückfrage, ungelesen oder getrennt: die Fläche blüht einmal kurz auf
  (Pigment `66`, dunkel `55`, ≈ 1 s abklingend). Kein Aufblühen beim Wechsel nach idle.
- Ausgewählt (zuletzt angeklickt): Ring mit Abstand `0 0 0 2px Panel, 0 0 0 3.5px Text`.
- Hover: Karte minimal dunkler (dunkel: heller); oben rechts erscheint ein kleines rundes
  ✕ zum Schließen (Rand hl-high, Grund Kartengrund; Hover rot).
- Klick fokussiert den Agenten; Rechtsklick öffnet das native Kontextmenü (Modell, Ticket,
  Effort, Modus, Schließen); Ziehen sortiert um.

## ＋-Karte

Genau so groß wie eine Karte ihres Blocks (197 × 118 bzw. 197 × 58), gestrichelter Rand
1,5 px hl-high (dunkel hl-med), Radius 14, „+“ mittig 18 px `#9893a5` (dunkel `#6f6b66`).
Hover: Rand und „+“ eine Stufe dunkler (dunkel heller). Sie steht als letzte Karte der Reihe.

## Blockkopf

Repo-Name 12,5 px halbfett in Textfarbe (ist zugleich der Griff zum Umsortieren der
Blöcke; Hover in der Ticketfarbe), daneben klein der Host („Windows Terminal“ /
„VS Code“, 11 px gedämpft). Links eine 2 px breite Schiene (hl-med), die Kopf und Karten
zu einer Gruppe bindet; beim Hover über den Block wird sie kräftiger. Heimat-Block
(`Repos`) steht immer oben.

Der Kopf meint das ganze Fenster: Rechtsklick auf den Kopf öffnet das native Menü
(vergessen, mit einem VS-Code-Fenster verbinden). Ein ✕ für das ganze Fenster gibt es
bewusst nicht. Solange verbunden wird, steht statt des
Hosts „Klicke jetzt ein VS-Code-Fenster an“ in Bernstein (Ink) mit atmendem Punkt.

## Panel, Leiste, Griff

- Panel in Panelfarbe, angedockt an der gewählten Kante, freie Ecken Radius 18, Rand 1 px
  (hl-med, dunkel Overlay; nicht an der Bildschirmkante), weicher Schatten (hell violettes
  Grau, dunkel Schwarz).
- Untere Leiste 36 px, Linie oben `#ece4dc` (dunkel `#2b2927`), Schrift 11 px gedämpft:
  links Ampelpunkt + „Session 34 %“ und „Woche 61 %“ (Prozent fett in Textfarbe), alt =
  matt; Hover zeigt Resets, weitere Limits, Stand, Fehler; Klick öffnet
  claude.ai/settings/usage. Rechts das **normale Zahnrad-Symbol** (Lucide `settings`),
  öffnet die Einstellungen. Links vom Zahnrad steht, nur wenn es welche gibt, leise
  „2 ohne Fenster“ (gedämpft, Hover wie die Nutzung); Hover oder Klick öffnet darüber
  rechtsbündig die Liste **„Läuft noch ohne Fenster“** (Canvas
  https://claude.ai/artifact/6U2pYMKFAPm2tfN1zckQZG, Zeichenfläche „Läuft noch ohne
  Fenster“): Fläche Panel, Rand hl-med, Radius 12; Kopf 12 px fett mit „Alle beenden“
  rechts; je Prozess eine Zeile auf dem Overlay (Radius 10): Port fett (ohne Port
  „kein Port“ gedämpft), Programm, Befehl mit Auslassung (Hover zeigt die ganze Zeile),
  Herkunft „A2 · zu seit 14 min“ gedämpft, Knopf „Beenden“ im Kartengrund (Hover rot);
  ein Fehler steht rot unter seiner Zeile; unten klein, was nie angeboten wird. Die
  Ampelfarben aus Rust (`usage.rs`) werden zu den Pigmenten
  von ungelesen, Rückfrage, getrennt und idle.
- Eingeklappt bleibt nur der **Griff**: Kapsel 120 × 7, Radius 7, im Pigment des
  dringlichsten Zustands mit leichtem Schein, atmet bei Rückfrage/denkt; alles ruhig =
  `#9893a5` (dunkel `#6f6b66`). Zeiger auf den Griff klappt das Panel aus, Zeiger weg
  klappt es ein.
- Aus-/Einklappen gleitet senkrecht zur Kante (≈ 220 ms raus, 180 ms rein), nie halb offen
  stehen bleiben.
- Passt das Panel nicht auf den Monitor, wird es als Ganzes kleiner gezoomt, nie
  abgeschnitten. Frei schwebend lässt es sich an leerer Fläche verschieben; unten rechts
  sitzt eine kleine Ecke (hl-high, Hover gedämpft), an der man es zoomt.

## Tooltip

Nach ≈ 500 ms Hover über einer Karte: Fläche im Kartengrund, Rand hl-med, Radius 10,
max. 360 px. Ganz oben klein (11 px, gedämpft) die Herkunft „Repo · Fenster A · A2“, bei
einem worktree darunter „↳ wt/<Ordner>“; dann Ticket/PR (Ticketfarbe), darunter die
KI-Zusammenfassung (IBM Plex Sans 13 px), solange sie rechnet die letzte Frage gedämpft.
Bleibt auf dem Monitor der Karte.

## Einstellungen

Eigenes Fenster im Aufbau von `einstellungen-glas.html`, im Papier-Look: Grund in
Panelfarbe, Segmente auf dem Overlay (gewählt hell als Blatt im Kartengrund, dunkel eine
Stufe heller). Zwei Bereiche, beide nur aus Segmenten: **Neue Agenten** (Modell, Modus,
Effort) und **Deck** (Sprache, Andocken). „Panel neu starten“ steht als kleiner, leiser
Knopf rechts in der Kopfzeile von **Deck**, so hoch wie der Hinweis neben „Neue Agenten“.
Kein Speichern-Knopf: jede Änderung gilt sofort, ein kurzes „Gespeichert“ bestätigt.

Bewusst **nicht** (seit 09.11.2026, Jorrit arbeitet nur noch in Windows Terminal): kein
Warnsatz und kein Rot bei Bypass, kein Ring um den Chat, kein Jira-Präfix, kein
Heimatordner, keine Werkzeuge für VS Code und kein „Repo öffnen“. Jira-Präfix
(`jira_prefix`) und Heimatordner (`terminal_home`) liest das Deck weiter aus
`deck_settings.json`, wenn sie dort von Hand stehen.

## Schrift und Texte

IBM Plex Mono für alles (mitgeliefert, `@fontsource/ibm-plex-mono` 400–700), IBM Plex Sans
für Prosa: Sessiontitel und Zusammenfassung (`@fontsource/ibm-plex-sans` 400, 600).
Deutsch und Englisch, je nach Spracheinstellung. Keine Gedankenstriche („—“, „–“) in
sichtbaren Texten.
