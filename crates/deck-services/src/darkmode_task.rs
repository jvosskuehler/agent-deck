//! `deck-setup migrate-darkmode`: Jorrits alte geplante Aufgabe „DarkModeZeitplan“
//! weg, sobald das Deck hell und dunkel selbst nach dem Sonnenstand schaltet.
//!
//! Die Aufgabe tat bisher dasselbe wie „Hell/Dunkel: Sonnenstand“ (`sun_switch`); liefen
//! beide, schalteten zwei Stellen um. Gelöscht wird nur, wenn sie da ist UND das Deck auf
//! Sonnenstand steht; ihr Ordner `%LOCALAPPDATA%\DarkModeZeitplan` mit Skript und letztem
//! Lauf bleibt liegen. Ohne beides ist nichts zu tun und nichts zu sagen.
//!
//! Hinter dem Trait [`Scheduler`], Tests bekommen eine Attrappe; die echte Fassung
//! (`schtasks`) löscht nur, solange [`crate::guard`] nicht sperrt.

use std::io;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use deck_core::appearance::Appearance;
use deck_core::deck_settings;
use serde_json::{Map, Value};

use crate::guard;
use crate::report::{Grade, Report};

/// Name der alten Aufgabe (im Stammordner der Aufgabenplanung).
pub const TASK_NAME: &str = "DarkModeZeitplan";
/// Merker für den Installer, der daraus einen Satz macht.
pub const TAG_REMOVED: &str = "darkmode-task-removed";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Die Aufgabenplanung von Windows, so weit gebraucht.
pub trait Scheduler {
    /// Gibt es die Aufgabe?
    fn exists(&self, name: &str) -> bool;
    /// Aufgabe löschen.
    fn delete(&self, name: &str) -> io::Result<()>;
}

/// Die echte Fassung über `schtasks.exe`.
pub struct Schtasks;

fn schtasks(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("schtasks.exe")
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

impl Scheduler for Schtasks {
    fn exists(&self, name: &str) -> bool {
        schtasks(&["/Query", "/TN", name]).is_ok_and(|o| o.status.success())
    }

    fn delete(&self, name: &str) -> io::Result<()> {
        guard::check("geplante Aufgabe löschen")?;
        let out = schtasks(&["/Delete", "/TN", name, "/F"])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()))
        }
    }
}

/// Hell/Dunkel so, wie das Deck es beim nächsten Start liest: mit derselben Migration
/// ([`deck_settings::migrate`]). Der Installer ruft das, bevor das Deck die Datei selbst
/// migriert hat; eine bestehende Installation ohne Version gilt darum schon als `sun`.
/// Geschrieben wird nichts.
pub fn effective_appearance(settings_file: &Path, home: &Path) -> Appearance {
    let raw = std::fs::read(settings_file).ok();
    let mut settings = match raw.as_deref().and_then(|b| serde_json::from_slice::<Value>(b).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    deck_settings::migrate(&mut settings, deck_settings::origin(raw.as_deref()), home);
    Appearance::from_settings(&settings)
}

/// Die Aufgabe löschen, wenn sie da ist und das Deck nach dem Sonnenstand schaltet.
pub fn migrate(appearance: Appearance, sched: &dyn Scheduler) -> Report {
    let mut r = Report::default();
    if !appearance.follows_sun() || !sched.exists(TASK_NAME) {
        return r;
    }
    match sched.delete(TASK_NAME) {
        Ok(()) => {
            r.changed = true;
            r.tag(TAG_REMOVED);
            r.push(Grade::Info, format!("Geplante Aufgabe „{TASK_NAME}“ entfernt: hell und dunkel schaltet jetzt das Deck"));
        }
        Err(e) => r.push(Grade::Warn, format!("Geplante Aufgabe „{TASK_NAME}“ nicht entfernt ({e}); sie schaltet weiter mit")),
    }
    r
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct FakeScheduler {
        present: bool,
        fail: bool,
        deleted: RefCell<Vec<String>>,
    }

    impl FakeScheduler {
        fn new(present: bool) -> FakeScheduler {
            FakeScheduler { present, fail: false, deleted: RefCell::default() }
        }
    }

    impl Scheduler for FakeScheduler {
        fn exists(&self, name: &str) -> bool {
            self.present && name == TASK_NAME
        }
        fn delete(&self, name: &str) -> io::Result<()> {
            if self.fail {
                return Err(io::Error::other("Zugriff verweigert"));
            }
            self.deleted.borrow_mut().push(name.into());
            Ok(())
        }
    }

    #[test]
    fn the_old_task_goes_only_when_the_deck_follows_the_sun() {
        let s = FakeScheduler::new(true);
        let r = migrate(Appearance::Sun, &s);
        assert_eq!(*s.deleted.borrow(), [TASK_NAME]);
        assert_eq!(r.tags, [TAG_REMOVED]);
        assert!(r.lines[0].text.contains("entfernt"));
        for other in [Appearance::System, Appearance::Light, Appearance::Dark] {
            let s = FakeScheduler::new(true);
            let r = migrate(other, &s);
            assert!(s.deleted.borrow().is_empty(), "{other:?}: die Aufgabe bleibt");
            assert!(r.lines.is_empty() && r.tags.is_empty(), "nichts zu sagen");
        }
    }

    #[test]
    fn without_the_task_there_is_nothing_to_say() {
        let r = migrate(Appearance::Sun, &FakeScheduler::new(false));
        assert!(r.lines.is_empty() && r.tags.is_empty() && !r.changed);
    }

    #[test]
    fn a_failed_delete_is_a_warning_not_a_failure() {
        let s = FakeScheduler { present: true, fail: true, deleted: RefCell::default() };
        let r = migrate(Appearance::Sun, &s);
        assert_eq!((r.fails(), r.warns()), (0, 1));
        assert!(r.tags.is_empty());
    }

    #[test]
    fn an_existing_install_without_a_version_already_counts_as_sun() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deck_settings.json");
        assert_eq!(effective_appearance(&file, dir.path()), Appearance::System, "neue Installation");
        std::fs::write(&file, r#"{"dock_edge": "top"}"#).unwrap();
        assert_eq!(effective_appearance(&file, dir.path()), Appearance::Sun, "Autor vor der Migration");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), r#"{"dock_edge": "top"}"#, "nichts geschrieben");
        std::fs::write(&file, r#"{"settings_version": 1, "repos_root": "C:/src"}"#).unwrap();
        assert_eq!(effective_appearance(&file, dir.path()), Appearance::System, "Installer einer Kollegin");
        std::fs::write(&file, r#"{"settings_version": 1, "appearance": "sun"}"#).unwrap();
        assert_eq!(effective_appearance(&file, dir.path()), Appearance::Sun);
    }

    #[test]
    fn the_real_scheduler_never_deletes_in_tests() {
        assert!(Schtasks.delete("AgentDeckTestGibtEsNicht").is_err());
    }
}
