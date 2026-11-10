//! Fachkern des Agent Decks: alles, was man auf Papier nachrechnen kann.
//!
//! Kein Fenster, kein Win32, kein Tauri. Darum ist jedes Modul hier ohne laufendes
//! Panel testbar (`cargo test -p deck-core`), und die Hooks (`deck-hooks`) können es
//! benutzen, ohne die Oberfläche nachzuziehen.
//!
//! Die Module entsprechen den Python-Modulen der früheren Fassung (`deck/domain`,
//! `deck/claude`, `deck/i18n.py`). `field` liest fremdes JSON tolerant, `text` zählt in
//! Zeichen statt Bytes.

pub mod agent_flags;
pub mod arrange;
pub mod attention;
pub mod auto_compact;
pub mod auto_mode;
pub mod bind_store;
pub mod binding;
pub mod claude;
pub mod config;
pub mod dock;
pub mod field;
pub mod deck_view;
pub mod front;
pub mod git_head;
pub mod i18n;
pub mod isotime;
pub mod menu;
pub mod ordering;
pub mod orphans;
pub mod paths;
pub mod prompt_cache;
pub mod prompts;
pub mod ref_sources;
pub mod season;
pub mod slot_state;
pub mod status_model;
pub mod text;
pub mod tile_refs;
pub mod tooltip;
pub mod view;
pub mod wt;
pub mod zoom;
