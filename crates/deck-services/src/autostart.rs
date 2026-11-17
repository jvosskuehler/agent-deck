//! Autostart beim Anmelden: Wert `AgentDeck` unter `HKCU\…\CurrentVersion\Run`.

use std::path::Path;

use crate::installed::Installed;
use crate::report::{Grade, Report};
use crate::user_system::{RUN_KEY, RegValue, UserSystem};

pub const VALUE: &str = "AgentDeck";

/// Der Wert: die Exe in Anführungszeichen.
pub fn command(bin: &Path) -> String {
    format!("\"{}\"", bin.join("agent-deck.exe").display())
}

pub fn set(bin: &Path, sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = Report::default();
    let cmd = command(bin);
    if sys.user_value(RUN_KEY, VALUE).as_deref() == Some(cmd.as_str()) {
        r.push(Grade::Ok, "Autostart ist eingetragen");
    } else if let Err(e) = sys.set(RUN_KEY, VALUE, &RegValue::Str(cmd.clone())) {
        r.push(Grade::Warn, format!("Autostart nicht eingetragen: {e}"));
        return r;
    } else {
        r.changed = true;
        r.push(Grade::Info, "Autostart eingetragen");
    }
    m.autostart = Some(cmd);
    r
}

/// Der Wert gehört dem Deck (Name), er geht immer.
pub fn remove(sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = Report::default();
    m.autostart = None;
    if sys.user_value(RUN_KEY, VALUE).is_none() {
        r.push(Grade::Ok, "Kein Autostart eingetragen");
        return r;
    }
    match sys.delete_value(RUN_KEY, VALUE) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Info, "Autostart entfernt");
        }
        Err(e) => r.push(Grade::Warn, format!("Autostart nicht entfernt: {e}")),
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_system::mem::MemSystem;

    #[test]
    fn autostart_points_at_the_exe_and_goes_away_again() {
        let sys = MemSystem::default();
        let mut m = Installed::default();
        let bin = Path::new(r"C:\Users\x\AppData\Local\claude-agent-deck\bin");
        assert!(set(bin, &sys, &mut m).changed);
        assert_eq!(sys.user_value(RUN_KEY, VALUE).as_deref(), Some(r#""C:\Users\x\AppData\Local\claude-agent-deck\bin\agent-deck.exe""#));
        assert_eq!(m.autostart, sys.user_value(RUN_KEY, VALUE));
        assert!(!set(bin, &sys, &mut m).changed);
        assert!(remove(&sys, &mut m).changed);
        assert!(sys.user_value(RUN_KEY, VALUE).is_none() && m.autostart.is_none());
        assert!(!remove(&sys, &mut m).changed);
    }
}
