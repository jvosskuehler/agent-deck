//! Das Schutzgitter dieses Crates: dieselbe Idee wie `deck_win::forbid_effects`, für die
//! Eingriffe, die hier selbst stattfinden (die App starten, `claude` aufrufen, den
//! Usage-Endpoint abfragen, fremde Einstellungsdateien ändern).
//!
//! Die erste Verteidigung sind die Traits (Tests bekommen Attrappen). Diese zweite greift,
//! wenn irgendwo doch die echte Fassung landet: in den Unit-Tests dieses Crates ist alles
//! gesperrt, ohne dass jemand daran denken muss, und `deck_win::forbid_effects()` sperrt
//! zugleich die Eingriffe beider Crates (ein Schalter für die App-Tests).

use std::io;

/// Sind echte Eingriffe gesperrt?
pub fn effects_forbidden() -> bool {
    cfg!(test) || deck_win::effects_forbidden()
}

/// Vor jedem Eingriff aufrufen; `what` nennt ihn in der Fehlermeldung.
pub(crate) fn check(what: &str) -> io::Result<()> {
    if effects_forbidden() {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Eingriff gesperrt: {what}")))
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
        assert_eq!(check("claude starten").unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
}
