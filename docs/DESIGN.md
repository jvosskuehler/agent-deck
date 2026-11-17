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
eigenes Thema), wie das Terminal-Profil. Hell = Rosé Pine Dawn, dunkel = Rosé Pine. Die
Einstellung „Hell/Dunkel“ (seit 18.11.2026) kann Deck und Agenten fest hell oder dunkel
stellen: dann gibt Rust allen Fenstern das Thema vor (`AppHandle::set_theme`), und
`prefers-color-scheme` folgt ihm; das CSS bleibt dasselbe. Die
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

| Rolle | hell | dunkel |
|---|---|---|
| Panel | `#faf4ed` | `#1d1c1b` |
| Kartengrund | `#fffaf3` | `#242220` |
| Overlay (ruhige Karte, Segmente) | `#f2e9e1` | `#2b2927` |
| Text | `#575279` | `#e6e0d6` |
| Modell · Effort, Modus | `#575279` | `#cfc9bf` |
| gedämpft („nicht verbunden“, Leiste) | `#6e6a86` | `#8e8a84` |
| Ticket/PR (nur erkannt: gedimmt) | `#6a5590` | `#bda2e0` |
| Modus Bypass | `#8f3f58` | `#f497a8` |
| Haarlinien (hl-med / hl-high) | `#dfdad9` / `#cecacd` | `#3a3734` / `#4a4744` |

## Karte

- Breite 197, Abstand 10, vier je Reihe. Radius 14, kein Rand.
- **Karte** 197 × 118, Innenabstand 10 12 9:
  oben links Statustext (11 px, fett, Ink), oben rechts „Modell · Effort“ (10,5 px,
  normal); darunter der **Sessiontitel** (IBM Plex Sans 14 px, halbfett, Zeilenhöhe 1,3,
  Textfarbe, höchstens zwei Zeilen, sonst Auslassung); unten die Fußzeile „A“ (Vorlage
  „A: 01 mit Ticket und PR“, 09.11.2026), drei Plätze mit `space-between`, 10,5 px, ohne
  Umbruch: links Ticket (fett) und PR als `#<n>` (mittel), beide in der Ticketfarbe, nur
  geraten (Titel, Chat, ein nur gelesener worktree) gedimmt; in der Mitte die Restzeit
  des Prompt-Caches (warm gedämpft `#6e6a86`/`#8e8a84` normal, knapp Textfarbe fett,
  kalt „kalt“ gedämpft mit Deckkraft 0,55; solange der Agent arbeitet leer); rechts
  Modus (mittel). Breitester
  Fall „ORPH-4116 #1234“ · „42 min“ · „bypass“ passt in 173 px gerade.
- Fläche: Pigment mit Hex-Alpha `38` (dunkel `30`) über dem Kartengrund; idle und none
  liegen stattdessen auf dem Overlay. Zustandswechsel blenden in 400 ms über.
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

Genau so groß wie eine Karte (197 × 118), gestrichelter Rand
1,5 px hl-high (dunkel hl-med), Radius 14, „+“ mittig 18 px `#9893a5` (dunkel `#6f6b66`).
Hover: Rand und „+“ eine Stufe dunkler (dunkel heller). Sie steht als letzte Karte der Reihe.

## Blockkopf

Repo-Name 12,5 px halbfett in Textfarbe (ist zugleich der Griff zum Umsortieren der
Blöcke; Hover in der Ticketfarbe); ist der Block nicht verbunden, steht daneben klein
„nicht verbunden“ (11 px gedämpft). Einen Host nennt der Kopf nicht: alle Agenten laufen
in Windows Terminal. Eine Schiene links neben dem Block gibt es seit 11.11.2026 nicht
mehr: sie stammte aus der Zeit, als die Blöcke VS-Code-Fenster waren. Heimat-Block
(der Repos-Ordner) steht immer oben; ohne brauchbaren Repos-Ordner gibt es keinen.

Der Kopf meint den ganzen Block: Klick auf den Repo-Namen holt seine Fenster nach vorn,
Rechtsklick öffnet das native Menü (vergessen; beim Heimat-Block kein Menü). Ein ✕ für
den ganzen Block gibt es bewusst nicht.

## Info-Fenster (Dashboard in der freien Fensterkachel)

Seit 15.11.2026, neu gebaut am 16.11.2026 nach den Entwürfen im Canvas „Info-Fenster
Entwürfe“ (Tafel „T1 mit echten Kosten“). Das Deck kachelt die Agentenfenster eines Blocks
auf dem Monitor (`arrange::tile_grid`). Geht das Raster nicht auf (5 Fenster als 3 + 2),
bleibt am Ende der letzten Reihe Platz. Dort steht das Info-Fenster, genau so groß wie die
freie Fläche (`wt_gap.rs`, `dash_window.rs`). Ohne Lücke holt die Info-Taste
(Strg+Alt+D, `dash_hotkey`) es mitten auf den Monitor unter dem Zeiger, immer oben; nochmal
oder × oben rechts klappt es zu. Ein Schalter in den Einstellungen nimmt es ganz heraus.

- **Fenster:** rahmenlos, Panelfarbe, Ecken 8 px, nie aktiv, nicht in der Taskleiste. In
  der Lücke direkt hinter dem vordersten Agentenfenster (geht mit den Agenten nach hinten).
- **Oben, Tokenkosten:** große Zahl, darunter die Zusammensetzung (Cache lesen, Cache
  schreiben, Output, Input) und die Modelle. Gestapelte Fläche je Ticket über die Zeit,
  Zeitraum 1 Std. / 24 Std. / 7 Tage / 30 Tage (merkt sich die Oberfläche) in Minuten,
  Viertelstunden, Stunden und Vierteltagen. Gezeichnet wird geglättet (Gauß, Breite 14 px
  auf dem Bildschirm, nicht in Eimern: im schmalen Fenster glättet es also stärker) als
  monotone Kurve, damit ein Verlauf statt eines Kamms entsteht; Tooltip und Summen zeigen
  die echten Werte je Eimer. Ist das Fenster schmaler als 760 px (die freie Kachel neben
  vielen Agenten), steht die Rangliste als zweispaltige Legende ohne Balken unter dem
  Graphen, damit er die volle Breite bekommt. Fadenkreuz und Tooltip beim Überfahren. Rechts die Rangliste (sieben größte, „+n weitere“, „ohne
  Ticket“); Klick zeigt nur dieses Ticket im eigenen Maßstab. Einheit: Kosten in
  Input-Tokens von Opus 5.5 mit den API-Preisen je Modell (`deck_core::dash::costs`).
- **Farben:** acht Plätze der geprüften Dataviz-Palette, je Thema eigene Stufen
  (`dash.css`, `--c1` … `--c8`). Ein Ticket behält seine Farbe in jedem Zeitraum
  (`CostView.colored`); weitere Tickets und „ohne Ticket“ sind grau.
- **Unten, Leiste:** Kacheln PRs (offen, bereit, bei dir, warten), Reviews für dich, Titan
  (nur mit eingeschaltetem Schalter „Titan“ und verwaisten Läufen, rot getönt), Worktrees
  (nur mit Entfernbarem). Klick auf eine Kachel zeigt ihre Liste statt des Graphen;
  nochmal, Esc oder „Tokenkosten“ in den Brotkrumen zurück.
- **Listen:** PRs mit Status nach „wer ist am Zug“ (mergebereit, CI rot, Änderungen offen,
  Reviewer anfragen, wartet auf Re-Review, wartet auf Review, Draft), Klick öffnet GitHub.
  Reviews mit „Ausblenden“, „Ansehen“ und „Approve“. Ausgeblendete wandern in einen
  zugeklappten Bereich „Ausgeblendet (n)“ unter der Liste, matt, mit „Einblenden“; die
  Kachel zählt sie nicht mit. Wartet der PR nicht mehr auf mich (gemergt, geschlossen,
  Anfrage zurückgezogen), vergisst das Deck das Ausblenden. Titan-Läufe mit Alter und „Beenden“, „Alle
  beenden“. Worktrees in drei Spalten mit „Alle n entfernen“. Jede Aktion fragt mit
  einem zweiten Klick nach; Rust prüft danach selbst noch einmal. Hinter „PRs offen“ steht
  seit 17.11.2026 der PR-Feed (unten); die bisherige PR-Liste ist dort der Reiter „Liste“.
- **Schrift:** Plex Sans, Tickets und Ordner in Plex Mono. Keine Gedankenstriche.

### PR-Feed (Entwurf „F1 Zeitleiste“, Canvas https://claude.ai/artifact/6KMzZA3qNnWmiTeMSEjAnZ)

Liste hinter der Kachel „PRs offen“ (`FeedPane.tsx`, `feed.css`). Kopf: Brotkrumen
„Tokenkosten › Deine PRs“, rechts der Schalter „Automatik“ (Spur 28 × 16, an = Pigment
gemergt, Not-Aus: aus = nur anzeigen) und das Segment „Feed | Liste“ wie der Zeitraum.

- **Abschnittsköpfe** 11 px, halbfett, versal, Sperrung 0,06 em, gedämpft: „Braucht dich“,
  „Hintergrund-Agenten“ (nur wenn welche laufen oder warten), „Was passiert ist“,
  „Automatik“ (nur mit eigenen offenen PRs).
- **Braucht dich:** je PR eine Karte auf dem Overlay, Radius 14, Innenabstand 12 14. Drafts erscheinen nie
  (geparkt; die Automatik und die Hintergrund-Agenten lassen sie ebenfalls in Ruhe).
  Avatar-Kreis 28 px mit Kürzel (hl-med), Login fett, dahinter gedämpft „kommentiert ·
  vor 4 Min.“; Zeile 2 PR-Kürzel (Plex Mono, Ticketfarbe, klickbar) und Titel; Zeile 3
  der Auszug in Meta-Farbe, eine Zeile mit Auslassung, „und n weitere“ gedämpft. Rechts
  die Knöpfe: „Claude soll umsetzen“ über die ganze Breite (gefüllt in Textfarbe, Schrift
  Kartengrund, zweiter Klick bestätigt), darunter „Öffnen“ (hl-med) und „Später“ (leise).
  Ein laufender Agent steht violett als „◐ …“ unter dem Auszug. Ein fertiger Entwurf steht
  eingerückt (40 px) unter der Karte im Kartengrund mit 3 px violetter Kante links, Kopf
  „Entwurf von Claude“, höchstens 4 Zeilen, Knöpfe „Absenden und auflösen“ (bzw.
  „Absenden“ ohne Thread, mit Rückfrage) und „Ändern“ (öffnet die Datei im Editor; das
  Fenster nimmt nie Tastatureingaben an).
- **Hintergrund-Agenten:** Zeile 30 px hoch, violetter Punkt 8 px (atmet; wartend grau,
  ruhig), PR-Kürzel, Aufgabe, rechts gedämpft „seit 3 Min.“ bzw. „wartet, Session aktiv“
  und „Abbrechen“ (Rückfrage).
- **Zeitleiste:** Raster 52 px Zeit (Plex Mono 12 px, `--muted`), 22 px Punkt (10 px),
  Rest Text 13 px; Haarlinie (Overlay) zwischen den Zeilen. Kürzel in Plex Mono und
  Ticketfarbe, Detail gedämpft hinter „·“, „Bericht“ in Ticketfarbe klappt die letzte
  Antwort des Agenten darunter auf (Overlay, Radius 8, max. 160 px hoch).
- **Punktfarben:** gemergt = Pigment ungelesen (hell `#4fa36a`, dunkel `#86d2a3`), Claude
  hat gearbeitet = `--c7` (hell `#6250d6`, dunkel `#9085e9`), Review = Pigment denkt (hell
  `#4f86c6`, dunkel `#8db9ee`), Problem = Pigment getrennt (hell `#c4536f`, dunkel
  `#f08a9e`), Kleinkram = `--muted`. Die Legende steht unten über die ganze Kartenbreite
  unter einer Haarlinie.
- **Automatik je PR:** PR-Kürzel, Titel, rechts gedämpft was die Automatik vorhat
  („mergt, sobald alles grün ist“, „wartet auf api#214“, gehalten in Achtung-Farbe) und
  „nicht mergen“ (leise) bzw. „wieder freigeben“.
- **Leer:** „Nichts offen.“, „Keine Ereignisse in den letzten 24 Std.“; `feed.note`
  gedämpft am Ende.

### Kommentar-Fenster (Entwurf „N1“)

Eigenes kleines Fenster (`#/comment`, `CommentView.tsx`, `comment.css`), etwa 420 × 190,
rahmenlos, nie aktiv: Kartengrund, Rand 1 px hl-high, Ecken 8 px, Innenabstand 12 14 14 16.
Kopf: Avatar 24 px, Login fett, „auf <PR-Kürzel>“ gedämpft, × rechts (28 px, schließt nur,
der Kommentar bleibt im Feed). Darunter der Kommentar in Meta-Farbe, höchstens drei
Zeilen. Unten „Claude soll umsetzen“ (gefüllt), „Öffnen“ (öffnet und schließt), „Später“.
Liegt ein Entwurf vor, steht er statt der Knöpfe da (Kommentar dann eine Zeile) mit
„Absenden und auflösen“ und „Ändern“.

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
einem worktree darunter „↳ wt/<Ordner>“; dann Ticket und PR (Ticketfarbe, je mit der
Herkunft klein dahinter: zugewiesen, Branch, Ordner, Claude Code, Titel, Chat), darunter die
KI-Zusammenfassung (IBM Plex Sans 13 px), solange sie rechnet die letzte Frage gedämpft.
Bleibt auf dem Monitor der Karte.

## Einstellungen

Eigenes Fenster im Aufbau von `einstellungen-glas.html`, im Papier-Look: Grund in
Panelfarbe, Segmente auf dem Overlay (gewählt hell als Blatt im Kartengrund, dunkel eine
Stufe heller). Die Höhe folgt dem Inhalt (`useDialogFit`), höchstens bis zur
Arbeitsfläche, darüber rollt es.

**Kompakt und erweitert** (seit 17.11.2026, Wunsch von Jorrit): Das Fenster geht kompakt
auf wie vor den Einstellungen für Kolleginnen und Kollegen, eine Spalte, 480 breit, nur
„Neue Agenten“ (Modell, Modus, Effort) und „Deck“ (Sprache, Andocken, Info-Fenster,
Hell/Dunkel, Auto-/compact, die drei kleinen Knöpfe). Unten links ein leiser Knopf „Erweitert“
(`tool-btn`) zieht das Fenster auf 960 und zwei Spalten mit allen vier Bereichen auf,
dort heißt er „Weniger“. Die Wahl merkt sich die Oberfläche (`localStorage`). Taugt der
Repos-Ordner nicht, stehen Hinweis und Ordnerzeile auch kompakt über „Neue Agenten“.
Erweitert sind es vier Bereiche, durch Haarlinien getrennt (links Konto und Ordner, Neue
Agenten; rechts Deck, Automatik):

- **Konto und Ordner** („gilt für dieses Konto“): Repos-Ordner als Pfad in Plex Mono auf
  dem Overlay (Radius 9, zu lang: vorn gekürzt) mit dem leisen Knopf „Ändern…“ rechts
  darin, der den Ordner-Dialog von Windows öffnet; darunter das Jira-Projekt als
  Textfeld (Kartengrund, Rand hl-med, Radius 9, Plex Mono 12 px; gespeichert beim
  Verlassen oder mit Enter, Escape verwirft).
- **Neue Agenten** (Modell mit „Standard“ = ohne `--model`, Modus, Effort als Segmente;
  erweitert dazu der Schalter „Neue Agenten in Auto schalten“).
- **Deck** (Sprache, Andocken, Hell/Dunkel als Segmente; Schalter Info-Fenster und
  Auto-Compact). Hell/Dunkel hat vier Segmente „System | Hell | Dunkel | Sonnenstand“
  (englisch „System | Light | Dark | Sunrise/sunset“) und steht auch kompakt. Darunter klein
  und gedämpft, was die Wahl über das Deck hinaus tut: bei Hell und Dunkel „Nur Deck und
  Agenten. Offene Agentenfenster wechseln beim nächsten Start.“, bei Sonnenstand „Schaltet
  Windows bei Sonnenaufgang hell und bei Sonnenuntergang dunkel (Hamburg).“; bei System
  nichts. Den Ort gibt es nur erweitert und nur bei Sonnenstand, als Auswahlliste „Ort für
  Sonnenstand“ (wie das Textfeld: Kartengrund, Rand hl-med, Radius 9, Plex Mono 12 px,
  Pfeil gedämpft), so wie die GitHub-Organisation erst mit der PR-Automatik erscheint. „Panel neu starten“
  steht als kleiner, leiser Knopf rechts in der Kopfzeile, so hoch wie der Hinweis neben
  „Neue Agenten“.
- **Automatik** („läuft nur, was du einschaltest“): PowerShell und Terminal
  aktualisieren, SpeechMike, Titan, PR-Automatik. Darunter das Reviewer-Feld (immer) und
  bei eingeschalteter PR-Automatik die GitHub-Organisation. Ins Reviewer-Feld kommen
  Namen, Mailadressen oder Logins; nach Enter oder Verlassen sucht das Deck sie in der
  Organisation und zeigt sie als „Leo Müller (leo-mxyz)“. Darunter klein (11 px): bei
  mehreren Treffern „„Anna“: wen meinst du?“ mit je einem leisen Knopf pro Person
  (Plex Mono), bei mehreren Organisationen dieselbe Wahl für die Organisation, ohne
  Treffer in Achtung-Farbe „nicht in <org> gefunden, als Login übernommen“ bzw.
  „… übersprungen“, ohne gh gedämpft „Namen nicht nachgeschlagen (…)“.

Ein Schalter ist eine Zeile: links der Name (12 px, Textfarbe) und darunter klein und
gedämpft, was er nach außen tut; rechts „An | Aus“ als schmale Segmente (118 px, 26 px
hoch). So bleiben viele Schalter kurz, ohne eine zweite Bedienform einzuführen.

Taugt der Repos-Ordner nicht (nicht gewählt, nicht gefunden, kein git-Repo darin), steht
unter der Kopfzeile ein Hinweis in der Farbe von „Rückfrage“ (Fläche Pigment 22 % über
Panel, Schrift Ink, halbfett, Radius 10), und die Ordnerzeile trägt einen Ring im
Pigment, ihr Name die Ink-Farbe. Das Deck öffnet das Fenster dann beim Start selbst.
Kein Speichern-Knopf: jede Änderung gilt sofort, ein kurzes „Gespeichert“ bestätigt.

**Kein Dialog zeigt von sich aus einen Rollbalken** (Einstellungen, Tastenkürzel, Ticket,
Glow und jeder neue): das Fenster ist so hoch wie sein Inhalt (`useDialogFit` +
`fit_dialog`). Gerollt wird nur, wenn der Inhalt höher als der Bildschirm ist. Wer einem
Dialog Zeilen hinzufügt oder einen neuen baut, prüft das am Screenshot.

Bewusst **nicht** (seit 09.11.2026): kein Warnsatz und kein Rot bei Bypass, kein Ring um
den Chat, kein „Repo öffnen“. Jira-Projekt und Repos-Ordner waren vom 09.11. bis
17.11.2026 ebenfalls draußen, weil nur der Autor das Deck benutzte. Diese Entscheidung
ist zurückgenommen: seit Kolleginnen und Kollegen das Deck installieren, darf nichts
Persönliches fest eingebaut sein, und beides muss sich ohne Handarbeit an
`deck_settings.json` einstellen lassen.

## Schrift und Texte

IBM Plex Mono für alles (mitgeliefert, `@fontsource/ibm-plex-mono` 400–700), IBM Plex Sans
für Prosa: Sessiontitel und Zusammenfassung (`@fontsource/ibm-plex-sans` 400, 600).
Deutsch und Englisch, je nach Spracheinstellung. Keine Gedankenstriche („—“, „–“) in
sichtbaren Texten.
