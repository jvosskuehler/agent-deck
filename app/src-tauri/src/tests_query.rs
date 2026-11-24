//! Eine abgelehnte Rückfrage endet ohne Hook (`deck_core::query_end`): Kachel, Sprung-Taste
//! und Datei gegen Attrappen.

use deck_core::view::Status;
use serde_json::json;

use crate::tests::rig;

#[test]
fn a_rejected_question_shows_idle_and_the_jump_key_skips_it_while_an_open_one_stays() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 71);
    r.wt_agent("A2", 72);
    let dir = r.deck.paths.state_dir.clone();
    let rejected = json!({"type": "user", "isSidechain": false, "timestamp": "2026-11-24T12:52:27.217Z", "toolUseResult": "User rejected tool use"});
    std::fs::write(dir.join("a1.jsonl"), format!("{rejected}\n")).unwrap();
    std::fs::write(dir.join("a2.jsonl"), "").unwrap();
    // 12:50:54 UTC
    let asked = 1_795_524_654.9;
    for slot in ["A1", "A2"] {
        let tr = dir.join(format!("{}.jsonl", slot.to_lowercase()));
        r.state(slot, json!({"slot": slot, "status": "waiting", "ts": asked, "transcript": tr.to_string_lossy()}));
    }
    let view = r.deck.tick(asked + 100.0);
    let status = |slot: &str| view.blocks.iter().flat_map(|b| &b.tiles).find(|t| t.slot == slot).map(|t| t.status);
    assert_eq!(status("A1"), Some(Status::Idle));
    assert_eq!(status("A2"), Some(Status::Waiting));
    r.deck.jump_to_waiting();
    assert_eq!(r.deck.active_slot.as_deref(), Some("A2"));
    let file = deck_core::slot_state::read_all(&dir);
    assert_eq!(file["A1"]["status"], "waiting", "die Datei gehört den Hooks");
}
