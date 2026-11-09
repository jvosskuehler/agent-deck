//! `state\<slot>.refs.json` für die Statuszeile: das Deck schreibt Ticket und PR jeder
//! Kachel, nur bei Änderung, und räumt die Datei einer verschwundenen Kachel weg. Alles im
//! Wegwerf-Ordner des Prüfstands (`Paths::under`).

use std::fs;

use deck_core::paths::load_json;
use deck_core::tile_refs::{TileRefs, refs_path};
use serde_json::json;

use crate::tests::rig;

#[test]
fn every_tile_gets_its_ticket_and_pr_file_and_only_a_change_rewrites_it() {
    let mut r = rig();
    r.wt_block("B", "repo");
    r.wt_agent("B1", 11);
    r.deck.store.tickets.insert("B1".into(), "ORPH-4116".into());
    r.deck.tick(100.0);
    let path = refs_path(&r.deck.paths.state_dir, "B1");
    let written = load_json(&path).expect("refs.json geschrieben");
    assert_eq!(
        written,
        json!({"ticket": "ORPH-4116", "ticket_source": "assigned", "ticket_detected": false,
            "pr": null, "pr_source": null, "pr_detected": false})
    );

    // Unverändert: kein neues Schreiben (die gelöschte Datei kommt nicht wieder).
    fs::remove_file(&path).unwrap();
    r.deck.tick(100.4);
    assert!(!path.exists(), "ohne Änderung wird nicht geschrieben");

    // Geändert: sofort neu.
    r.deck.store.tickets.insert("B1".into(), "ORPH-4062".into());
    r.deck.tick(100.8);
    let refs = TileRefs::parse(&load_json(&path).unwrap());
    assert_eq!(refs.ticket.map(|t| t.text).as_deref(), Some("ORPH-4062"));
}

#[test]
fn the_file_of_a_tile_that_is_gone_is_removed() {
    let mut r = rig();
    r.wt_block("B", "repo");
    r.wt_agent("B1", 11);
    r.deck.tick(100.0);
    let path = refs_path(&r.deck.paths.state_dir, "B1");
    assert!(path.exists(), "auch ohne Ticket steht die Datei (leer), damit nichts Altes bleibt");
    r.deck.forget_window("B");
    r.deck.tick(100.4);
    assert!(!path.exists());
}
