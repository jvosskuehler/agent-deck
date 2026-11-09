//! Ticket und PR je Kachel für die Statuszeile: `state\<slot>.refs.json`
//! (`deck_core::tile_refs`). So zeigt das Terminal dasselbe Ticket wie die Kachel, ohne
//! dass der Hook selbst etwas erkennt.
//!
//! Geschrieben wird nur, was sich geändert hat (der Takt läuft alle 400 ms, die Dateien
//! liest jede Statuszeile alle paar Sekunden), atomar wie alle Zustandsdateien. Die Datei
//! einer Kachel, die verschwindet, kommt weg: ein späterer Agent im selben Slot soll nicht
//! das Ticket seines Vorgängers zeigen.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use deck_core::paths::save_json;
use deck_core::tile_refs::{TileRefs, refs_path};
use deck_core::view::RefView;

use crate::applog::note;

pub struct RefsFiles {
    dir: PathBuf,
    /// Was je Slot zuletzt geschrieben wurde.
    written: HashMap<String, TileRefs>,
}

impl RefsFiles {
    pub fn new(state_dir: PathBuf) -> RefsFiles {
        RefsFiles { dir: state_dir, written: HashMap::new() }
    }

    /// Mit den Kacheln der gerade gebauten Ansicht abgleichen.
    pub fn sync(&mut self, tiles: &HashMap<String, (Option<RefView>, Option<RefView>)>) {
        for (slot, (ticket, pr)) in tiles {
            let refs = TileRefs::from_views(ticket.as_ref(), pr.as_ref());
            if self.written.get(slot) == Some(&refs) {
                continue;
            }
            match save_json(&refs_path(&self.dir, slot), &refs.to_json()) {
                Ok(()) => {
                    self.written.insert(slot.clone(), refs);
                }
                Err(e) => note(format!("refs.json {slot}: {e}")),
            }
        }
        let gone: Vec<String> = self.written.keys().filter(|s| !tiles.contains_key(*s)).cloned().collect();
        for slot in gone {
            let _ = fs::remove_file(refs_path(&self.dir, &slot));
            self.written.remove(&slot);
        }
    }
}
