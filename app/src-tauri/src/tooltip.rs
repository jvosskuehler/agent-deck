//! Der Hover-Tooltip einer Karte (`deck/ui/hover.py`): eigenes, nie aktives Fenster,
//! das durchklickt. Die 500-ms-Verzögerung macht die Oberfläche; Rust zeigt sofort.
//!
//! Ablauf: `hover_tile` setzt den Inhalt (`deck://tooltip`), die Ansicht misst sich und
//! meldet `tooltip_content_size`; erst dann wird das Fenster gelegt und gezeigt, unter
//! der Karte (passt es nicht, darüber), immer auf dem Monitor der Karte. Kommt die
//! Zusammenfassung nach, schickt der Takt den neuen Inhalt; gilt der Tooltip dann schon
//! einer anderen Karte, wird sie verworfen.

use deck_core::tooltip::{ScreenRect, place};
use deck_core::view::TooltipView;
use tauri::{AppHandle, Emitter};

use crate::deck::Deck;
use crate::native;
use crate::state::{lock, shared};
use crate::ui_windows::{TOOLTIP, raw_hwnd};

#[derive(Debug, Default)]
pub struct TipState {
    slot: Option<String>,
    rect: Option<ScreenRect>,
    /// Gemeldete Größe (CSS-Pixel, samt Rand).
    size: Option<(f64, f64)>,
    sent: Option<TooltipView>,
}

/// Inhalt des Tooltips: Ticket und PR wie auf der Kachel (samt Herkunft),
/// Zusammenfassung, sonst die letzte Frage.
pub fn content(deck: &Deck, slot: &str) -> TooltipView {
    let st = deck.state_of(slot);
    let field = |k: &str| st.and_then(|s| s.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let sid = field("session_id");
    let (ticket, pr) = deck.tile_refs.get(slot).cloned().unwrap_or_default();
    let prompt = field("prompt");
    TooltipView {
        slot: slot.to_string(),
        ticket,
        pr,
        summary: deck.services.summary(&sid),
        last_prompt: Some(prompt).filter(|p| !p.is_empty()),
        origin: origin_lines(deck, slot),
    }
}

/// Herkunft der Karte (`hover.py: _origin_lines`): die Zusammenfassung sagt, WORUM es
/// geht, nie WO. Bei mehreren Repos ist genau das die Frage am Tooltip, und ein Agent
/// im worktree arbeitet neben dem Repo, das der Blockkopf nennt.
pub fn origin_lines(deck: &Deck, slot: &str) -> Vec<String> {
    if slot.is_empty() {
        return Vec::new();
    }
    let win = Deck::letter(slot);
    let repo = deck.store.bindings.get(&win).cloned().unwrap_or_default();
    let window = format!("{} {win}", deck.lang.l("Fenster", "Window"));
    let parts: Vec<&str> = [repo.as_str(), window.as_str(), slot].into_iter().filter(|p| !p.is_empty()).collect();
    let mut lines = vec![parts.join(" · ")];
    if let Some(wt) = deck.worktrees.get(slot).filter(|w| !w.trim().is_empty()) {
        let name = std::path::Path::new(wt.trim_end_matches(['\\', '/'])).file_name().map(|n| n.to_string_lossy().into_owned());
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            lines.push(format!("↳ wt/{name}"));
        }
    }
    lines
}

fn view_needs_summary(deck: &Deck, slot: &str) -> bool {
    let sid = deck.state_of(slot).and_then(|s| s.get("session_id")).and_then(|v| v.as_str()).unwrap_or("");
    !sid.is_empty() && deck.services.summary(sid).is_none()
}

fn show(app: &AppHandle, tip: &TipState) {
    let (Some(rect), Some((cw, ch))) = (tip.rect, tip.size) else { return };
    let s = shared(app);
    let (cx, cy) = (rect.x + rect.width / 2, rect.y + rect.height / 2);
    let (work, scale) = crate::dock::monitor_at(s.desktop.as_ref(), cx, cy);
    let size = ((cw * scale).round() as i32, (ch * scale).round() as i32);
    native::show_at(raw_hwnd(app, TOOLTIP), place(rect, size, work, scale));
}

/// Karte betreten (`slot`) oder verlassen (`None`).
pub fn hover(app: &AppHandle, slot: Option<String>, rect: Option<ScreenRect>) {
    let s = shared(app);
    let (Some(slot), Some(rect)) = (slot, rect) else {
        hide(app);
        return;
    };
    if !crate::dock::is_open(app) {
        return;
    }
    let view = {
        let d = lock(&s.deck);
        // Zusammenfassung fehlt noch: jetzt anstoßen (sie kommt über den Takt nach).
        if view_needs_summary(&d, &slot) {
            let st = d.state_of(&slot);
            let field = |k: &str| st.and_then(|s| s.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
            d.services.request_info(&field("session_id"), &field("cwd"), d.lang, true);
        }
        content(&d, &slot)
    };
    let mut tip = lock(&s.tip);
    let same = tip.slot.as_deref() == Some(slot.as_str());
    tip.slot = Some(slot);
    tip.rect = Some(rect);
    if !same {
        tip.size = None;
    }
    tip.sent = Some(view.clone());
    let _ = app.emit("deck://tooltip", view);
    if same {
        show(app, &tip);
    }
}

/// Die Ansicht hat sich gemessen.
pub fn content_size(app: &AppHandle, width: f64, height: f64) {
    let s = shared(app);
    let mut tip = lock(&s.tip);
    tip.size = Some((width, height));
    if tip.slot.is_some() {
        show(app, &tip);
    }
}

pub fn hide(app: &AppHandle) {
    let s = shared(app);
    let mut tip = lock(&s.tip);
    let was = tip.slot.take().is_some();
    tip.rect = None;
    tip.sent = None;
    native::hide(raw_hwnd(app, TOOLTIP));
    if was {
        let _ = app.emit("deck://tooltip-hide", ());
    }
}

/// Im Takt: hat sich der Inhalt des gezeigten Tooltips geändert (Zusammenfassung da),
/// den neuen schicken.
pub fn refresh(app: &AppHandle, deck: &Deck) {
    let s = shared(app);
    let mut tip = lock(&s.tip);
    let Some(slot) = tip.slot.clone() else { return };
    let view = content(deck, &slot);
    if tip.sent.as_ref() != Some(&view) {
        tip.sent = Some(view.clone());
        let _ = app.emit("deck://tooltip", view);
    }
}
