# Schnittstelle Oberfläche ↔ Rust

Die Typen stammen aus `crates/deck-core/src/view.rs` und liegen erzeugt in
`app/src/bindings/`. Diese Datei legt fest, welche Fenster es gibt, welche Ereignisse
Rust schickt und welche Befehle die Oberfläche aufruft. Beide Seiten halten sich daran;
wer etwas ändert, ändert es hier zuerst.

## Fenster

Alle Fenster laden dieselbe Seite (`index.html`) und wählen ihre Ansicht über den Hash
(`app/src/start.tsx`). Ein Hash, den der Kern nicht kennt, gehört zu einer Erweiterung
(unten), sonst zeigt die Seite das Panel.

| Label | Hash | Inhalt | Eigenschaften |
|---|---|---|---|
| `panel` | `#/panel` | das Deck: Blöcke, Karten, untere Leiste | rahmenlos, transparent, immer oben, nicht in der Taskleiste |
| `handle` | `#/handle` | der Griff (Kapsel) im eingeklappten Zustand | rahmenlos, transparent, immer oben, klein |
| `tooltip` | `#/tooltip` | Hover-Tooltip einer Karte | rahmenlos, transparent, immer oben, nie aktiv |
| `settings` | `#/settings` | Einstellungen | normales Fenster mit dunkler Titelleiste |
| `ticket` | `#/ticket` | Dialog „Ticket zuweisen“ (Kontextmenü einer Karte) | normales Fenster, klein, immer oben |

Das Panel misst seinen natürlichen Inhalt und meldet ihn mit `panel_content_size`; Rust
setzt daraufhin die Fenstergröße. Ein- und Ausklappen entscheidet Rust (Zeigerposition),
die Oberfläche animiert das Gleiten im Fenster (`deck://dock`).

## Ereignisse (Rust → Oberfläche)

| Name | Nutzlast | Wann |
|---|---|---|
| `deck://view` | `DeckView` | bei jeder Änderung (Poll-Takt 400 ms, nur wenn sich etwas geändert hat) |
| `deck://dock` | `DockView` | Ein-/Ausklappen, Kantenwechsel |
| `deck://tooltip` | `TooltipView` | Tooltip zeigen bzw. Zusammenfassung nachgeliefert |
| `deck://tooltip-hide` | – | Tooltip ausblenden |
| `deck://settings` | `SettingsView` | Einstellungen haben sich geändert (auch von außen) |
| `deck://ticket` | `TicketView` | Ticket-Dialog für einen Slot geöffnet |
| `deck://glow` | `GlowView` | Fenster „Glow einstellen“ geöffnet oder Glow geändert |
| `deck://ping` | `null` | alle 5 s an Panel und Griff; die Seite antwortet mit `ui_pong` (`ui_watch.rs`) |

Beim Start ruft jede Ansicht zusätzlich einmal den passenden `get_*`-Befehl, damit sie
nicht auf das erste Ereignis warten muss.

## Befehle (Oberfläche → Rust, `invoke`)

| Befehl | Argumente | Rückgabe | Wirkung |
|---|---|---|---|
| `get_view` | – | `DeckView` | aktueller Stand |
| `get_dock` | – | `DockView` | aktueller Andock-Zustand |
| `get_settings` | – | `SettingsView` | aktuelle Einstellungen |
| `panel_content_size` | `width`, `height` (CSS-Pixel) | – | Fenstergröße anpassen |
| `focus_slot` | `slot` | – | Agent nach vorn holen (sein Windows-Terminal-Fenster), Auswahl setzen |
| `close_slot` | `slot` | – | Agent schließen; Erweiterungen hören davon (`on_slot_released`), diese Ausgabe löscht keinen worktree |
| `create_agent` | `letter` | – | neuen Agenten im Block anlegen |
| `show_tile_menu` | `slot` | – | natives Kontextmenü an der Zeigerposition |
| `reorder_tiles` | `letter`, `slots: string[]` | – | neue Kartenreihenfolge im Block |
| `reorder_blocks` | `letters: string[]` | – | neue Blockreihenfolge |
| `hover_tile` | `slot` oder `null`, `rect` (Bildschirm-Pixel der Karte) | – | Tooltip anfordern bzw. verbergen |
| `open_usage_page` | – | – | claude.ai/settings/usage im Browser |
| `open_settings` | – | – | Einstellungsfenster zeigen |
| `set_setting` | `key` (Feldname aus `SettingsView` oder Schlüssel einer Erweiterung), `value` | `SettingsView` | sofort speichern; einen Schlüssel, den der Kern nicht kennt, bekommt die Erweiterung (`DeckExtension::set_setting`) |
| `pick_folder` | `start` (Ordner, in dem der Dialog aufgeht) | Pfad oder `null` (abgebrochen) | „Ändern…“ am Repos-Ordner: Ordner-Dialog von Windows (`deck_win::folder_pick`), modal zum fragenden Fenster; gespeichert wird danach per `set_setting` |
| `restart_panel` | – | – | „Panel neu starten“: frische Instanz mit denselben Argumenten, diese endet |
| `show_block_menu` | `letter` | – | Rechtsklick auf den Kopf eines Blocks: natives Menü (vergessen) |
| `panel_hold` | `hold: boolean` | – | Karte oder Block wird gezogen: Panel bleibt offen (höchstens 60 s) |
| `panel_drag` | `mode`: `move` \| `zoom` \| `slide`, `x`, `y` (Stelle des Drucks, CSS-Pixel) | – | Rust folgt dem Zeiger, bis die Taste los ist, und merkt sich das Ergebnis. `move`/`zoom` nur frei schwebend (verschieben bzw. zoomen), `slide` nur angedockt: der Streifen an der Bildschirmkante schiebt das Deck die Kante entlang (`dock_along`) |
| `focus_block` | `letter` | – | Klick auf den Repo-Namen: Fenster des Blocks nach vorn (wie `show_window` in Python) |
| `tooltip_content_size` | `width`, `height` (CSS-Pixel, inkl. 8 px Rand) | – | Größe des Tooltip-Fensters anpassen |
| `dialog_content_size` | `height` (CSS-Pixel), optional `width` | – | Ein Dialog meldet seine Inhaltshöhe, sein Fenster wächst bzw. schrumpft mit (höchstens Arbeitsbereich des Monitors). `width` setzt zusätzlich die Breite: Einstellungen kompakt 480, erweitert 960 |
| `get_ticket` | – | `TicketView` oder `null` | Inhalt des Ticket-Dialogs |
| `submit_ticket` | `ticket`: Text oder `null` | – | zuweisen (leer = abbrechen) bzw. abbrechen; schließt den Dialog |
| `get_glow` | – | `GlowView` | Inhalt des Fensters „Glow einstellen“ |
| `set_glow` | `glow`: `GlowSettings` | `GlowView` | beide Glows setzen (Rust holt sie in die Grenzen), wirkt sofort am Fokus-Rahmen, gespeichert in `deck_settings.json` `glow` |
| `open_glow` | – | – | Fenster „Glow einstellen“ öffnen (wie Strg+Alt+G) |
| `open_keys` | – | – | Fenster „Tastenkürzel“ öffnen (Inhalt aus `SettingsView.shortcuts`) |
| `kill_orphan` | `id` (`OrphanView.id`) | – | „Läuft noch ohne Fenster“: diese Zeile beenden. Kehrt sofort zurück; die Zeile steht bis zum Ende auf `busy`, ein Fehler danach in `error` |
| `kill_all_orphans` | – | – | „Alle beenden“: jede Zeile wie `kill_orphan` |
| `ui_pong` | – | – | Antwort auf `deck://ping`; schweigt die Seite des sichtbaren Fensters oder der Hauptthread 60 s, startet das Deck neu (`deck_core::liveness`) |

### Was die Oberfläche bereits so umsetzt (Stand 06.11.)

- `hover_tile.rect` = `{x, y, width, height}` in physischen Bildschirm-Pixeln (aus
  `innerPosition()` + CSS-Lage × `scaleFactor()`); die Webview braucht dafür die Rechte
  für `inner_position` und `scale_factor`. Die 500-ms-Verzögerung macht die Oberfläche,
  Rust zeigt sofort. Eine nachgereichte Zusammenfassung für eine nicht mehr gehoverte
  Karte verwirft Rust.
- `panel_content_size` enthält 24 px Schattenraum gegenüber der Andockkante; das Panel
  liegt bündig an der Kante (oben-links bei top/left/off, oben-rechts bei right,
  unten-links bei bottom). Für „Zeiger weg → einklappen“ zählt nur das Panel-Rechteck.
- Gleiten: raus 220 ms, rein 180 ms, Deadline +80 ms. Rust zeigt das Fenster vor
  `phase: open` und verbirgt es frühestens ≈ 200 ms nach `phase: closed`.
- Griff-Fenster 152 × 30 (oben/unten) bzw. 30 × 152 (links/rechts), Kapsel 3 px von der
  Kante. Tooltip-Fenster mit `ignore_cursor_events`. Einstellungen ≈ 480 × 820.
- `reorder_blocks` bekommt die volle Liste (Heimat-Block vorn, nicht ziehbar),
  `reorder_tiles` alle Slots des Blocks; die Oberfläche hält die gezogene Reihenfolge
  1,5 s, bis sie im View bestätigt ist.
- `set_setting` gibt den vollen `SettingsView` zurück und sendet `deck://settings`; bei
  `language` zusätzlich ein neues `deck://view`, bei `dockEdge` ein `deck://dock`; bei
  `appearance` stellt Rust Thema und Sonnenstand neu ein (`appearance.rs`).

### Blockkopf, Neustart, Zoom (Stand 07.11., Lückenschluss zur Python-Fassung)

- Kopf eines Blocks (`connect.py`): Rechtsklick → natives Menü mit „<Repo> vergessen“
  (nicht beim Heimat-Block; der hat kein Menü). Das frühere „Mit VS-Code-Fenster
  verbinden …“ ist mit den VS-Code-Agenten entfallen (seit 09.11.2026 nur Windows Terminal).
- `TooltipView.origin`: Herkunftszeilen über Ticket und Zusammenfassung („Repo · Fenster
  A · A2“, bei worktree „↳ wt/<Ordner>“), wie `hover.py: _origin_lines`.
- „Panel neu starten“ (`restart_panel`) startet die Exe mit denselben Argumenten und
  derselben Umgebung neu (Tauri `request_restart`); im Herzschlag steht dann `restart`
  statt eines normalen Endes. Einen Wächter, der bei einem Kind ohne Start einspringt,
  gibt es nicht mehr (`deck-watchdog` ist entfallen).
- Zoom (`layout.py`): Rust rechnet `DockView.zoom` = gezogener Zoom (`panel_zoom`, 0,5 bis
  2), höchstens so groß, dass das Panel auf die Arbeitsfläche passt, und setzt ihn als
  Zoom der Panel-Webview. Die Oberfläche misst weiter ihre natürliche Größe;
  `screenRect` rechnet CSS-Pixel × `scaleFactor()` × `zoom`.
- Frei schwebend (Kante `off`): Druck auf leere Fläche verschiebt das Panel, die Ecke
  unten rechts zoomt; Lage (`panel_pos`) und Zoom (`panel_zoom`) stehen danach in
  `deck_settings.json`. Das freie Panel liegt auf dem Monitor seiner Lage.

### Was Rust dazu tut (Stand 07.11.)

- Panel, Griff und Tooltip zeigt und legt Rust ohne Aktivierung (`SetWindowPos` mit
  `SWP_NOACTIVATE`): das Aufklappen per Hover nimmt dem Editor nie den Fokus. Die drei
  Fenster sind `focusable(false)`.
- Andocken wie Python: Hauptmonitor, Lage entlang der Kante aus `dock_along`, Zeiger-Takt
  70 ms, Einklappen 320 ms nach Verlassen des Panel-Rechtecks (8 px Kulanz), Aufklappen,
  sobald der Zeiger die ersten 16 px des Griffs berührt, danach 220 ms Sperre. Ein
  Zweitstart klappt auf und hält 4 s offen. Solange ein Kontextmenü offen ist oder
  die Oberfläche zieht (`panel_hold`), bleibt das Panel offen.
- Das Kontextmenü (`show_tile_menu`) ist nativ: Model, Ticket (zuweisen öffnet `ticket`,
  entfernen), Effort, Mode, „Agent schließen“.

### Läuft noch ohne Fenster (Stand 09.11.)

- `DeckView.orphans`: Prozesse aus geschlossenen Agentenfenstern, fertig sortiert und
  beschriftet (`OrphanView`: `portText`, `program`, `command`, `commandFull`, `origin`
  „A2 · zu seit 14 min“, `busy`, `error`). Leer = kein Hinweis in der Leiste.
- `id` ist `pid-startzeit` als Text (die FILETIME passt nicht genau in eine JS-Zahl).
  Rust beendet nur, was unter genau dieser PID UND Startzeit noch eine Waise ist, samt
  ihrer Nachkommen von oben nach unten, nie Claude, das Deck, Docker, Konsolenwirte oder
  Prozesse in Sitzung 0 (`deck_core::orphans`), und prüft danach, dass die Prozesse weg
  und ihre Ports frei sind.
- Zugeordnet wird über die Eltern-Kette aus Schnappschüssen und, wo die gerissen ist, über
  die Herkunftsmarke `AGENT_DECK_SHELL` in der Umgebung (`docs/DATEIEN.md`).
- Gemerkt wird in `%LOCALAPPDATA%\claude-agent-deck\orphans.json`, damit ein Neustart
  des Decks die Herkunft nicht vergisst. Gescannt wird in einem eigenen Thread
  (`app/src-tauri/src/orphans.rs`), nicht im Takt.

Ohne Tauri (`pnpm dev` im Browser) bedient ein Mock dieselben Befehle und Ereignisse mit
Beispieldaten, damit die Oberfläche allein entwickelt und angeschaut werden kann.

## Erweiterungen

Diese Ausgabe startet ohne Erweiterung (`main.rs` ruft
`agent_deck_lib::run_with(tauri::generate_context!(), Vec::new())`). Eine andere Ausgabe
baut eine eigene Binärdatei gegen dieselbe Bibliothek und übergibt ihre Erweiterungen;
Schalter oder Einstellungen, die hier etwas freischalten, gibt es bewusst nicht.

- **Rust** (`app/src-tauri/src/extension.rs`, Trait `DeckExtension`, jeder Haken mit
  leerer Vorgabe): `setup` (beim Start, nach den Fenstern des Kerns), `commands` +
  `invoke_handler` (eigene Befehle), `on_tick` (je Takt unter der Sperre des Decks),
  `after_tick` (danach ohne Sperre), `ticket_prefix`, `sun_place`, `settings_section`,
  `set_setting`, `shortcuts` (Zeilen im Fenster „Tastenkürzel“), `on_slot_released`
  (Agent zu oder neues Ticket), `reserve_agent` (einen Agenten versteckt vorwärmen,
  `deck_core::reserve`; der Kern kann es, schaltet es aber nie selbst ein). Unter der Sperre des Decks darf eine Erweiterung ihre
  eigenen Mutexe nehmen, nie umgekehrt.
- **Befehle:** Ein `invoke`, dessen Name eine Erweiterung in `commands` führt, geht zuerst
  an deren Handler, alle anderen an den Kern. Die Oberfläche ruft sie über `extra.invoke`
  / `extra.fire` und hört mit `extra.listen` bzw. `useExtraLive` (`ipc/hooks.ts`) auf
  eigene Ereignisse; dahinter stehen `Backend.invokeAny`/`listenAny`, ohne Typen im
  Vertrag oben.
- **Fenster:** Eine Erweiterung legt ihre Fenster in `setup` mit
  `ui_windows::companion(app, label, title, size)` an (rahmenlos, nie aktiv, nicht immer
  oben, Seite `index.html#/<label>`) und registriert vorn die Ansicht dazu
  (`registerView(label, View)` in `app/src/extensions.ts`). Die freie Kachel neben den
  Agentenfenstern (`wt_gap.rs`, `WtHost::gap`) lässt der Kern leer; eine Erweiterung kann
  sie füllen. `Gap.rect` ist die ganze Kachel samt dem Streifen, in dem bei den Agenten
  der Statusring liegt: wer sie füllt, malt dort selbst einen Rand. Eigene Tasten über `hotkey::listen`.
- **Einstellungen:** `settings_section` liefert einen Abschnitt (`SettingsSection`: `id`,
  `values`), der in `SettingsView.extras` ankommt; die Oberfläche zeichnet ihn nur, wenn
  `registerSettingsSection(id, …)` eine Ansicht dafür hat. Gespeichert wird mit
  `commands.setExtraSetting(key, value)` über `set_setting`.
- **Einstieg vorn:** `app/src/main.tsx` ruft nur `startDeck()` (`start.tsx`). Eine andere
  Ausgabe hat einen eigenen Einstieg, registriert Ansichten, Abschnitte und für
  `pnpm dev` Mock-Befehle (`registerMockCommands`) und ruft dann `startDeck()`.
