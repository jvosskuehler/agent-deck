//! Das Schutzgitter: echte Eingriffe lassen sich prozessweit sperren.
//!
//! Hintergrund: Am 2026-11-06 hat die Python-Testsuite zweimal das VS Code beendet, in
//! dem sie lief. Die erste Verteidigung sind die Traits in [`crate::effects`] (Tests
//! bekommen Attrappen). Diese zweite greift, wenn eine Attrappe vergessen wurde: jede
//! Funktion mit Wirkung nach außen fragt hier nach, bevor sie etwas tut.
//!
//! - In den Unit-Tests dieser Crate ist alles gesperrt, ohne dass jemand daran denken muss.
//! - Die App ruft in ihren Tests [`forbid_effects`] auf (einmal je Test reicht, der
//!   Schalter gilt für den ganzen Prozess und lässt sich nicht zurücknehmen).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::{Error, Result};

static FORBIDDEN: AtomicBool = AtomicBool::new(false);

/// Ab jetzt schlägt in diesem Prozess jeder echte Eingriff mit [`Error::Forbidden`] fehl:
/// Fenster bewegen, schließen, fokussieren, umstylen, Prozesse beenden, Tasten senden,
/// Overlay zeigen, globale Tasten belegen. Lesen bleibt erlaubt.
pub fn forbid_effects() {
    FORBIDDEN.store(true, Ordering::SeqCst);
}

/// Sind echte Eingriffe gesperrt?
pub fn effects_forbidden() -> bool {
    cfg!(test) || FORBIDDEN.load(Ordering::SeqCst)
}

/// Vor jedem Eingriff aufrufen; `what` nennt ihn in der Fehlermeldung.
pub(crate) fn check(what: &'static str) -> Result<()> {
    if effects_forbidden() {
        Err(Error::Forbidden(what))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_tests_of_this_crate_can_never_cause_effects() {
        assert!(effects_forbidden());
        assert_eq!(check("kill_tree"), Err(Error::Forbidden("kill_tree")));
    }
}
