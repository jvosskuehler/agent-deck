//! Die nativen Menüs des Panels (`deck/ui/connect.py`):
//! - Kachel (`_card_menu`): Model, Ticket, Effort, Mode als Untermenüs, darunter
//!   „Agent schließen“. Ein bewusst per Rechtsklick gewählter Eintrag ist Absicht genug,
//!   darum ohne Rückfrage.
//! - Kopf eines Blocks per Rechtsklick (`_forget_menu`): Bindung vergessen, mit einem
//!   VS-Code-Fenster verbinden.
//! - ✕ im Kopf (`_close_window_menu`): Rückfrage als Menü. Oben eine nicht wählbare
//!   Kopfzeile direkt unter dem Zeiger: der zweite Klick eines gewohnheitsmäßigen
//!   Doppelklicks landet dort, die eigentliche Aktion braucht einen bewussten Klick.
//!
//! Die Einträge tragen ihre Wirkung in der ID (`model|B3|opus[1m]`); `handle` führt sie
//! aus, wenn Tauri das Menü-Ereignis meldet.

use std::sync::atomic::Ordering;

use deck_core::claude::settings::{EFFORT_CHOICES, MODEL_CHOICES};
use deck_core::i18n::Lang;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Wry};

use crate::state::{lock, shared};
use crate::ui_windows::{PANEL, get};

/// Was ein Eintrag auslöst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Model(String, String),
    AssignTicket(String),
    ClearTicket(String),
    Effort(String, String),
    Mode(String, String),
    Close(String),
    /// Kopf eines Blocks: Bindung des Buchstabens vergessen.
    Forget(String),
    /// Kopf eines Blocks: „Mit Fenster verbinden“ beginnen bzw. abbrechen.
    Bind(String),
    /// Bestätigtes ✕ im Kopf: das ganze Fenster schließen.
    CloseWindow(String),
}

impl MenuAction {
    pub fn id(&self) -> String {
        match self {
            MenuAction::Model(s, v) => format!("model|{s}|{v}"),
            MenuAction::AssignTicket(s) => format!("ticket|{s}"),
            MenuAction::ClearTicket(s) => format!("unticket|{s}"),
            MenuAction::Effort(s, v) => format!("effort|{s}|{v}"),
            MenuAction::Mode(s, v) => format!("mode|{s}|{v}"),
            MenuAction::Close(s) => format!("close|{s}"),
            MenuAction::Forget(w) => format!("forget|{w}"),
            MenuAction::Bind(w) => format!("bind|{w}"),
            MenuAction::CloseWindow(w) => format!("closewin|{w}"),
        }
    }

    pub fn parse(id: &str) -> Option<MenuAction> {
        let mut it = id.splitn(3, '|');
        let (kind, slot) = (it.next()?, it.next()?.to_string());
        let val = it.next().map(String::from);
        Some(match (kind, val) {
            ("model", Some(v)) => MenuAction::Model(slot, v),
            ("ticket", None) => MenuAction::AssignTicket(slot),
            ("unticket", None) => MenuAction::ClearTicket(slot),
            ("effort", Some(v)) => MenuAction::Effort(slot, v),
            ("mode", Some(v)) => MenuAction::Mode(slot, v),
            ("close", None) => MenuAction::Close(slot),
            ("forget", None) => MenuAction::Forget(slot),
            ("bind", None) => MenuAction::Bind(slot),
            ("closewin", None) => MenuAction::CloseWindow(slot),
            _ => return None,
        })
    }
}

/// Erster Buchstabe groß („auto“ → „Auto“), wie Pythons `capitalize`.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect()).unwrap_or_default()
}

fn item(app: &AppHandle, label: &str, action: MenuAction) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(app, action.id(), label, true, None::<&str>)
}

/// Das Menü für `slot` bauen.
fn build(app: &AppHandle, slot: &str) -> tauri::Result<Menu<Wry>> {
    let s = shared(app);
    let (lang, current_ticket, cycle) = {
        let mut d = lock(&s.deck);
        (d.lang, d.store.tickets.get(slot).cloned(), d.mode_cycle())
    };
    let models = Submenu::with_id(app, "m-model", "Model", true)?;
    for (label, value) in MODEL_CHOICES {
        models.append(&item(app, label, MenuAction::Model(slot.into(), value.into()))?)?;
    }
    let ticket = Submenu::with_id(app, "m-ticket", "Ticket", true)?;
    ticket.append(&item(app, lang.l("Ticket zuweisen …", "Assign ticket …"), MenuAction::AssignTicket(slot.into()))?)?;
    if let Some(t) = current_ticket {
        let label = if lang.is_english() { format!("Remove ticket “{t}”") } else { format!("Ticket „{t}“ entfernen") };
        ticket.append(&item(app, &label, MenuAction::ClearTicket(slot.into()))?)?;
    }
    let effort = Submenu::with_id(app, "m-effort", "Effort", true)?;
    for label in EFFORT_CHOICES {
        effort.append(&item(app, label, MenuAction::Effort(slot.into(), label.into()))?)?;
    }
    let mode = Submenu::with_id(app, "m-mode", "Mode", true)?;
    for m in &cycle {
        mode.append(&item(app, &capitalize(m), MenuAction::Mode(slot.into(), m.clone()))?)?;
    }
    let close = if lang.is_english() { format!("Close agent {slot}") } else { format!("Agent {slot} schließen") };
    Menu::with_items(
        app,
        &[&models, &ticket, &effort, &mode, &PredefinedMenuItem::separator(app)?, &item(app, &close, MenuAction::Close(slot.into()))?],
    )
}

/// Name des Blocks für die Menütexte: das Repo, sonst „Fenster A“.
fn block_name(app: &AppHandle, win: &str) -> (String, Lang, bool, bool, bool) {
    let s = shared(app);
    let d = lock(&s.deck);
    let name = d.store.bindings.get(win).cloned().unwrap_or_else(|| format!("{} {win}", d.lang.l("Fenster", "Window")));
    (name, d.lang, d.hosts.is_wt(win), d.home_win.as_deref() == Some(win), d.binding(win))
}

/// Rechtsklick auf den Kopf eines Blocks. `None`, wenn es nichts anzubieten gibt
/// (Heimat-Block: er entsteht beim Start neu, Verbinden gibt es nur für VS Code).
fn build_block(app: &AppHandle, win: &str) -> tauri::Result<Option<Menu<Wry>>> {
    let (name, lang, wt, home, binding) = block_name(app, win);
    let menu = Menu::new(app)?;
    let mut any = false;
    if !wt {
        let label = if binding {
            lang.l("Verbinden abbrechen", "Cancel connecting")
        } else {
            lang.l("Mit VS-Code-Fenster verbinden …", "Connect to a VS Code window …")
        };
        menu.append(&item(app, label, MenuAction::Bind(win.into()))?)?;
        any = true;
    }
    if !home {
        let label = if lang.is_english() { format!("Forget “{name}”") } else { format!("„{name}“ vergessen") };
        menu.append(&item(app, &label, MenuAction::Forget(win.into()))?)?;
        any = true;
    }
    Ok(any.then_some(menu))
}

/// Rückfrage zum ✕ im Kopf: Kopfzeile (nicht wählbar), Trennlinie, die Aktion.
fn build_close_window(app: &AppHandle, win: &str) -> tauri::Result<Menu<Wry>> {
    let (name, lang, wt, _, _) = block_name(app, win);
    let header = if lang.is_english() { format!("Close “{name}”?") } else { format!("„{name}“ schließen?") };
    let action = if wt {
        lang.l("Ja, alle Agenten im Terminal beenden", "Yes, end all agents in the terminal")
    } else {
        lang.l("Ja, VS-Code-Fenster schließen", "Yes, close the VS Code window")
    };
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "m-header", &header, false, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, action, MenuAction::CloseWindow(win.into()))?,
        ],
    )
}

/// Ein Menü an der Zeigerposition zeigen. Blockiert, bis es zu ist; solange bleibt
/// das Panel offen (es klappte sonst unter dem Menü weg).
fn popup(app: &AppHandle, what: &str, menu: tauri::Result<Option<Menu<Wry>>>) {
    let s = shared(app);
    crate::tooltip::hide(app);
    let Some(panel) = get(app, PANEL) else { return };
    match menu {
        Ok(Some(menu)) => {
            s.menu_open.store(true, Ordering::SeqCst);
            let _ = panel.popup_menu(&menu);
            s.menu_open.store(false, Ordering::SeqCst);
        }
        Ok(None) => {}
        Err(e) => crate::applog::note(format!("Kontextmenü {what}: {e}")),
    }
}

/// Kontextmenü einer Kachel.
pub fn show(app: &AppHandle, slot: &str) {
    popup(app, slot, build(app, slot).map(Some));
}

/// Kontextmenü des Kopfes eines Blocks.
pub fn show_block(app: &AppHandle, win: &str) {
    popup(app, win, build_block(app, win));
}

/// Rückfrage zum ✕ im Kopf eines Blocks.
pub fn confirm_close_window(app: &AppHandle, win: &str) {
    popup(app, win, build_close_window(app, win).map(Some));
}

/// Ein Eintrag wurde gewählt.
pub fn handle(app: &AppHandle, id: &str) {
    let Some(action) = MenuAction::parse(id) else { return };
    let s = shared(app);
    if let MenuAction::AssignTicket(slot) = &action {
        crate::ticket_dialog::open(app, slot);
        return;
    }
    let mut d = lock(&s.deck);
    match action {
        MenuAction::Model(slot, v) => d.set_slot_model(&slot, &v),
        MenuAction::ClearTicket(slot) => d.clear_ticket(&slot),
        MenuAction::Effort(slot, v) => d.set_slot_effort(&slot, &v),
        MenuAction::Mode(slot, v) => d.menu_set_mode(&slot, &v),
        MenuAction::Close(slot) => d.close_agent(&slot),
        MenuAction::Forget(win) => d.forget_window(&win),
        MenuAction::Bind(win) => d.start_bind(&win, crate::now()),
        MenuAction::CloseWindow(win) => d.close_window(&win),
        MenuAction::AssignTicket(_) => {}
    }
    drop(d);
    crate::poll::nudge(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_carry_their_action_and_survive_brackets_in_values() {
        for a in [
            MenuAction::Model("B3".into(), "opus[1m]".into()),
            MenuAction::AssignTicket("A1".into()),
            MenuAction::ClearTicket("A1".into()),
            MenuAction::Effort("C2".into(), "Ultracode".into()),
            MenuAction::Mode("C2".into(), "bypass".into()),
            MenuAction::Close("D4".into()),
            MenuAction::Forget("B".into()),
            MenuAction::Bind("C".into()),
            MenuAction::CloseWindow("A".into()),
        ] {
            assert_eq!(MenuAction::parse(&a.id()), Some(a));
        }
        assert_eq!(MenuAction::parse("m-model"), None);
        assert_eq!(MenuAction::parse("m-header"), None, "die Kopfzeile der Rückfrage tut nichts");
        assert_eq!(MenuAction::parse("close|A1|x"), None);
    }

    #[test]
    fn mode_names_are_capitalized_like_python() {
        assert_eq!(capitalize("auto"), "Auto");
        assert_eq!(capitalize("bypass"), "Bypass");
        assert_eq!(capitalize(""), "");
    }
}
