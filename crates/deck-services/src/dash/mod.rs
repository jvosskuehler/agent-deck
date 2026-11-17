//! Datenquellen des Info-Fensters (`deck_core::dash`): jede ein Hintergrund-Thread, der
//! im eigenen Takt nachsieht und den letzten Stand hält. Texte baut erst deck-core.
//!
//! Wirkung nach außen (gh, ssh, git) steckt hinter Traits; Tests bekommen
//! Attrappen und starten nie ein echtes Programm.

pub mod agents;
pub mod cleanup;
pub mod costs;
pub mod effects;
pub mod feed_actions;
mod feed_agents;
mod feed_round;
pub mod feed_service;
pub mod github;
pub mod hidden_cmd;
pub mod ticker;
pub mod titan;
