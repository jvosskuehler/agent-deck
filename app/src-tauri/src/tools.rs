//! Die Werkzeuge der Einstellungen und ihre Rückmeldung (`settings_dialog.py`): Repo in
//! Windows Terminal öffnen, VS-Code-Fenster einsammeln, VS Code neu starten, Panel neu
//! starten, dazu das Ergebnis des Rings.
//!
//! Python schrieb jedes Ergebnis in eine Statuszeile des Dialogs; hier geht es als
//! [`ToolStatus`] an die Oberfläche: als Antwort auf `run_tool` oder, wenn es später
//! kommt (Fortschritt des Neustarts, Ring), als Ereignis `deck://tool`.

use deck_core::i18n::Lang;
use deck_core::view::ToolStatus;
use deck_services::gather::GatherResult;
use deck_services::vscode_glow::GlowResult;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;

use crate::applog::note;
use crate::state::{lock, shared};

pub const TOOL_EVENT: &str = "deck://tool";

fn status(text: impl Into<String>, tone: &str) -> ToolStatus {
    ToolStatus { text: text.into(), tone: tone.into() }
}

/// Rückmeldung später nachreichen (Neustart, Ring).
pub fn emit(app: &AppHandle, st: &ToolStatus) {
    note(format!("Werkzeug: {}", st.text));
    let _ = app.emit(TOOL_EVENT, st);
}

/// „VS-Code-Fenster einsammeln“, Wortlaut wie `_gather` in Python.
pub fn gather_text(r: &GatherResult, lang: Lang) -> ToolStatus {
    if r.moved == 0 {
        let text = r.error.clone().unwrap_or_else(|| lang.l("Kein Fenster verschoben.", "No window moved.").into());
        return status(text, "error");
    }
    let n = r.moved;
    let msg = if lang.is_english() {
        format!("Moved {n} VS Code window(s) to the primary monitor.")
    } else {
        format!("{n} VS-Code-Fenster auf den Hauptmonitor geholt.")
    };
    match &r.error {
        Some(e) => status(format!("{msg} {e}"), "error"),
        None => status(msg, "ok"),
    }
}

/// Ergebnis des Rings, Wortlaut wie `_toggle_glow` in Python (ohne Gedankenstrich).
pub fn ring_text(on: bool, r: &GlowResult, lang: Lang) -> ToolStatus {
    if let Some(e) = &r.error {
        return status(format!("Ring: {e}"), "error");
    }
    if r.total == 0 {
        return status(lang.l("Ring: keine VS-Code-Installation gefunden.", "Ring: no VS Code installation found."), "error");
    }
    let (done, total) = (r.done, r.total);
    let text = match (lang.is_english(), on) {
        (false, true) => format!("Ring aktiviert ({done}/{total}). In VS Code das Fenster neu laden."),
        (false, false) => format!("Ring entfernt ({done}/{total}). In VS Code das Fenster neu laden."),
        (true, true) => format!("Ring enabled ({done}/{total}). Reload the window in VS Code."),
        (true, false) => format!("Ring removed ({done}/{total}). Reload the window in VS Code."),
    };
    status(text, if on { "ok" } else { "info" })
}

/// Fortschritt des VS-Code-Neustarts: läuft = gedämpft, fertig = grün, Fehler = rot.
fn restart_tone(ok: Option<bool>) -> &'static str {
    match ok {
        Some(true) => "ok",
        Some(false) => "error",
        None => "info",
    }
}

/// Ein Werkzeug ausführen. `None` = nichts zu melden (Dialog abgebrochen) bzw. die
/// Meldung kommt als Ereignis nach.
pub fn run(app: &AppHandle, tool: &str) -> Result<Option<ToolStatus>, String> {
    let s = shared(app);
    let st = match tool {
        "openRepo" => {
            let start = deck_core::config::terminal_home();
            let picked = app.dialog().file().set_directory(start).blocking_pick_folder();
            let Some(folder) = picked.and_then(|f| f.into_path().ok()) else { return Ok(None) };
            let msg = lock(&s.deck).open_repo_in_terminal(&folder.to_string_lossy(), crate::now());
            crate::poll::nudge(app);
            Some(status(msg, "ok"))
        }
        "gatherWindows" => {
            let d = lock(&s.deck);
            d.services.gather_windows(d.lang).map(|r| gather_text(&r, d.lang))
        }
        "restartVscode" => {
            let mut d = lock(&s.deck);
            let states = deck_core::slot_state::read_all(&d.paths.state_dir);
            let model = d.setting_str("model");
            let plan = deck_services::restart_host::build_plan(
                &d.hosts.client_info(),
                &states,
                &d.store.order,
                model.as_deref(),
                &d.store.effort,
                &d.paths.projects_dir,
                crate::now(),
            );
            let tx = app.clone();
            let report: deck_services::restart::Reporter =
                Box::new(move |text: &str, ok: Option<bool>| emit(&tx, &status(text, restart_tone(ok))));
            d.services.restart_vscode(plan, report);
            None
        }
        "restartPanel" => {
            restart_panel(app);
            None
        }
        other => return Err(format!("unbekanntes Werkzeug: {other}")),
    };
    if let Some(st) = &st {
        note(format!("Werkzeug {tool}: {}", st.text));
    }
    Ok(st)
}

/// „Panel neu starten“ (`panel.py: restart`): eine frische Instanz mit denselben
/// Argumenten und derselben Umgebung, dann diese beenden. Tauri gibt vorher Port und
/// Einzelinstanz-Sperre frei. Bewusst ohne „normales Ende“ im Herzschlag: kommt das
/// Kind nicht hoch, soll der Wächter einspringen (wie in Python).
pub fn restart_panel(app: &AppHandle) {
    let s = shared(app);
    s.restarting.store(true, std::sync::atomic::Ordering::SeqCst);
    note("--- Panel-Neustart angefordert (Kind übernimmt) ---");
    app.request_restart();
}

/// Offene VS-Code-Fenster, die das Deck nicht kennt (für die Rückfrage vor dem
/// Neustart; leer heißt auch „konnte nicht nachsehen“).
pub fn foreign_windows(app: &AppHandle) -> Vec<String> {
    let s = shared(app);
    let known: Vec<String> = lock(&s.deck).store.bindings.values().cloned().collect();
    deck_services::restart_host::foreign_vscode_windows(s.desktop.as_ref(), &known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gathering_nothing_says_why_in_red() {
        let r = GatherResult { moved: 0, found: 0, error: Some("Kein VS-Code-Fenster gefunden.".into()) };
        assert_eq!(gather_text(&r, Lang::German), status("Kein VS-Code-Fenster gefunden.", "error"));
        let r = GatherResult { moved: 0, found: 2, error: None };
        assert_eq!(gather_text(&r, Lang::English).text, "No window moved.");
    }

    #[test]
    fn gathering_counts_what_really_moved() {
        let r = GatherResult { moved: 3, found: 4, error: None };
        assert_eq!(gather_text(&r, Lang::German), status("3 VS-Code-Fenster auf den Hauptmonitor geholt.", "ok"));
        let r = GatherResult { moved: 1, found: 2, error: Some("Eins klemmt.".into()) };
        assert_eq!(gather_text(&r, Lang::German).tone, "error");
    }

    #[test]
    fn the_ring_reports_its_count_and_asks_for_a_reload_without_dashes() {
        let ok = GlowResult { done: 2, total: 2, error: None };
        let st = ring_text(true, &ok, Lang::German);
        assert_eq!(st, status("Ring aktiviert (2/2). In VS Code das Fenster neu laden.", "ok"));
        assert_eq!(ring_text(false, &ok, Lang::English).tone, "info");
        let none = GlowResult { done: 0, total: 0, error: None };
        assert_eq!(ring_text(true, &none, Lang::German).tone, "error");
        for st in [ring_text(true, &ok, Lang::English), ring_text(false, &none, Lang::German)] {
            assert!(!st.text.contains('–') && !st.text.contains('—'), "{}", st.text);
        }
    }

    #[test]
    fn restart_progress_is_muted_until_it_ends() {
        assert_eq!(restart_tone(None), "info");
        assert_eq!(restart_tone(Some(true)), "ok");
        assert_eq!(restart_tone(Some(false)), "error");
    }
}
