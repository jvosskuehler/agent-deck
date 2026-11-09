# Schnittstelle Oberfläche ↔ Rust

Die Typen stammen aus `crates/deck-core/src/view.rs` und liegen erzeugt in
`app/src/bindings/`. Diese Datei legt fest, welche Fenster es gibt, welche Ereignisse
Rust schickt und welche Befehle die Oberfläche aufruft. Beide Seiten halten sich daran;
wer etwas ändert, ändert es hier zuerst.

## Fenster

Alle Fenster laden dieselbe Seite (`index.html`) und wählen ihre Ansicht über den Hash.

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

Beim Start ruft jede Ansicht zusätzlich einmal den passenden `get_*`-Befehl, damit sie
nicht auf das erste Ereignis warten muss.

## Befehle (Oberfläche → Rust, `invoke`)

| Befehl | Argumente | Rückgabe | Wirkung |
|---|---|---|---|
| `get_view` | – | `DeckView` | aktueller Stand |
| `get_dock` | – | `DockView` | aktueller Andock-Zustand |
| `get_settings` | – | `SettingsView` | aktuelle Einstellungen |
| `panel_content_size` | `width`, `height` (CSS-Pixel) | – | Fenstergröße anpassen |
| `focus_slot` | `slot` | – | Agent nach vorn holen (Fenster + Pane), Auswahl setzen |
| `close_slot` | `slot` | – | Agent schließen (inkl. Worktree aufräumen wie heute) |
| `create_agent` | `letter` | – | neuen Agenten im Block anlegen |
| `show_tile_menu` | `slot` | – | natives Kontextmenü an der Zeigerposition |
| `reorder_tiles` | `letter`, `slots: string[]` | – | neue Kartenreihenfolge im Block |
| `reorder_blocks` | `letters: string[]` | – | neue Blockreihenfolge |
| `hover_tile` | `slot` oder `null`, `rect` (Bildschirm-Pixel der Karte) | – | Tooltip anfordern bzw. verbergen |
| `open_usage_page` | – | – | claude.ai/settings/usage im Browser |
| `open_settings` | – | – | Einstellungsfenster zeigen |
| `set_setting` | `key` (Feldname aus `SettingsView`), `value` | `SettingsView` | sofort speichern |
| `restart_panel` | – | – | „Panel neu starten“: frische Instanz mit denselben Argumenten, diese endet |
| `show_block_menu` | `letter` | – | Rechtsklick auf den Kopf eines Blocks: natives Menü (vergessen, mit VS-Code-Fenster verbinden) |
| `panel_hold` | `hold: boolean` | – | Karte oder Block wird gezogen: Panel bleibt offen (höchstens 60 s) |
| `panel_drag` | `mode`: `move` \| `zoom`, `x`, `y` (Stelle des Drucks, CSS-Pixel) | – | Nur frei schwebend: Rust folgt dem Zeiger, bis die Taste los ist, verschiebt bzw. zoomt und merkt sich das Ergebnis |
| `focus_block` | `letter` | – | Klick auf den Repo-Namen: Fenster des Blocks nach vorn (wie `show_window` in Python) |
| `tooltip_content_size` | `width`, `height` (CSS-Pixel, inkl. 8 px Rand) | – | Größe des Tooltip-Fensters anpassen |
| `get_ticket` | – | `TicketView` oder `null` | Inhalt des Ticket-Dialogs |
| `submit_ticket` | `ticket`: Text oder `null` | – | zuweisen (leer = abbrechen) bzw. abbrechen; schließt den Dialog |

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
  `language` zusätzlich ein neues `deck://view`, bei `dockEdge` ein `deck://dock`.

### Blockkopf, Neustart, Zoom (Stand 07.11., Lückenschluss zur Python-Fassung)

- Kopf eines Blocks (`connect.py`): Rechtsklick → natives Menü mit „Mit VS-Code-Fenster
  verbinden …“ (nur VS Code; der nächste Klick in ein VS-Code-Fenster binnen 20 s bindet
  es, ein zweites Mal bricht ab) und „<Repo> vergessen“ (nicht beim Heimat-Block). Solange
  verbunden wird, steht `BlockView.binding` und der Kopf zeigt die Bitte statt des Hosts.
- `TooltipView.origin`: Herkunftszeilen über Ticket und Zusammenfassung („Repo · Fenster
  A · A2“, bei worktree „↳ wt/<Ordner>“), wie `hover.py: _origin_lines`.
- „Panel neu starten“ (`restart_panel`) startet die Exe mit denselben Argumenten und
  derselben Umgebung neu (Tauri `request_restart`); im Herzschlag steht dann kein normales
  Ende, damit der Wächter einspringt, falls das Kind nicht hochkommt.
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

Ohne Tauri (`pnpm dev` im Browser) bedient ein Mock dieselben Befehle und Ereignisse mit
Beispieldaten, damit die Oberfläche allein entwickelt und angeschaut werden kann.
