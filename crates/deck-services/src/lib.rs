//! Die Hintergrunddienste des Agent Deck: alles, was neben der Oberfläche läuft und nach
//! außen wirkt (Netz, Prozesse, Dateien fremder Programme, Fenster).
//!
//! Portiert aus `deck/claude/usage*.py`, `deck/claude/summarize.py` (der Prozess-Teil),
//! `deck/ops/*.py` und `deck/ui/worktree_sweep.py` der Python-Fassung. Die reine Rechnung
//! steht weiter in `deck-core` (Usage-Auswertung, Zusammenfassungs-Cache,
//! Hook-Einträge), das Win32-Handwerk in `deck-win`; hier ist die Verdrahtung.
//!
//! **Alles mit Wirkung nach außen steckt hinter einem Trait**, die App reicht die echte
//! Fassung herein, Tests eine Attrappe. Zusätzlich verweigern die echten Fassungen in
//! diesem Crate jeden Eingriff, solange [`guard::effects_forbidden`] gilt (in den
//! Unit-Tests immer, sonst nach `deck_win::forbid_effects`). Am 2026-11-06 hat die
//! Python-Suite zweimal das VS Code beendet, in dem sie lief.
//!
//! | Modul | Python | Die App implementiert |
//! |---|---|---|
//! | [`usage`] | `claude/usage.py`, `usage_token.py`, `claude-usage-shared/usage_poller.py` | nichts |
//! | [`summary`] | `claude/summarize.py` (`generate`, `_run_claude`), `ui/uithread.py` | nichts |
//! | [`worktree`], [`worktree_sweep`] | `ops/worktree.py`, `ui/worktree_sweep.py` | nichts |
//! | [`vscode_cleanup`] | (neu) die frühere VS-Code-Extension entfernen | nichts |
//! | [`install`], [`statusline_backup`] | `claude/hook_setup.py` (`main`) | nichts |
//! | [`shortcut`], [`wt_fragment`] | (neu) Startmenü-Verknüpfung, Windows-Terminal-Profil für deck-setup | nichts |
//! | [`look`], [`autostart`], [`apps_entry`], [`installed`], [`teardown`] | (neu) Papier-Look, Autostart, Eintrag unter „Apps“, `installed.json`, Deinstallation | nichts |
//! | [`user_system`] | (neu) Registratur des Benutzers und Schriften der Sitzung, hinter einem Trait | nichts |
//! | [`git_fs`] | (neu) Branch aus `.git/HEAD` lesen | nichts |
//! | [`orphans`] | (neu) „Läuft noch ohne Fenster“ | den Scan-Thread |
//! | [`store_update`] | (neu) Terminal und PowerShell aktualisieren, wenn kein Agent läuft | den Prüf-Thread |
//! | [`heartbeat`], [`watchdog`] | `ops/instance.py` (Herzschlag), `ops/watchdog.py` | schreibt den Herzschlag |

#![cfg(windows)]

pub mod apps_entry;
pub mod autostart;
pub mod git_fs;
pub mod dash;
pub mod guard;
pub mod heartbeat;
pub mod install;
pub mod installed;
pub mod logfile;
pub mod look;
pub mod orphans;
pub mod report;
pub mod reviewers;
pub mod shortcut;
pub mod statusline_backup;
pub mod store_update;
pub mod summary;
pub mod teardown;
pub mod usage;
pub mod user_system;
pub mod vscode_cleanup;
pub mod watchdog;
pub mod worktree;
pub mod worktree_sweep;
pub mod wt_fragment;

mod fsutil;
