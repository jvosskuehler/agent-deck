//! Der Hover-Tooltip einer Karte (`deck/ui/hover.py`): eigenes, nie aktives Fenster,
//! das durchklickt. Die 500-ms-Verzögerung macht die Oberfläche; Rust zeigt sofort.
//!
//! Ablauf: `hover_tile` setzt den Inhalt (`deck://tooltip`), die Ansicht misst sich und
//! meldet `tooltip_content_size`; erst dann wird das Fenster gelegt und gezeigt, unter
//! der Karte (passt es nicht, darüber), immer auf dem Monitor der Karte. Kommt die
//! Zusammenfassung nach, schickt der Takt den neuen Inhalt; gilt der Tooltip dann schon
//! einer anderen Karte, wird sie verworfen.

use deck_core::claude::refs::refs_label;
use deck_core::dock::Area;
use deck_core::view::TooltipView;
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::deck::Deck;
use crate::native;
use crate::state::{lock, shared};
use crate::ui_windows::{TOOLTIP, raw_hwnd};

/// Luft um die Karte im Tooltip-Fenster für den Schatten (CSS-Pixel, TooltipView.tsx).
const TIP_GUTTER: f64 = 8.0;
/// Abstand zwischen Karte und Tooltip (CSS-Pixel).
const TIP_GAP: f64 = 2.0;

/// Rechteck einer Karte in physischen Bildschirmpixeln (von der Oberfläche).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Default)]
pub struct TipState {
    slot: Option<String>,
    rect: Option<ScreenRect>,
    /// Gemeldete Größe (CSS-Pixel, samt Rand).
    size: Option<(f64, f64)>,
    sent: Option<TooltipView>,
}

/// Inhalt des Tooltips: Ticket/PR (zugewiesen oder erkannt), Zusammenfassung, sonst die
/// letzte Frage.
pub fn content(deck: &Deck, slot: &str) -> TooltipView {
    let st = deck.state_of(slot);
    let field = |k: &str| st.and_then(|s| s.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let sid = field("session_id");
    let mut refs = deck.detected_refs.get(&sid).map(refs_label).unwrap_or_default();
    if deck.worktrees.contains_key(slot)
        && let Some(t) = deck.store.tickets.get(slot).or_else(|| deck.found.get(slot))
    {
        refs = if refs.is_empty() { format!("Ticket: {t}") } else { refs };
    }
    let prompt = field("prompt");
    TooltipView {
        slot: slot.to_string(),
        references: Some(refs).filter(|r| !r.is_empty()),
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

/// Lage des Tooltip-Fensters: unter der Karte, sonst darüber, auf dem Monitor geklemmt.
pub fn place(card: ScreenRect, size_px: (i32, i32), work: Area, scale: f64) -> Area {
    let (w, h) = (size_px.0.min(work.width()), size_px.1.min(work.height()));
    let gutter = (TIP_GUTTER * scale).round() as i32;
    let gap = (TIP_GAP * scale).round() as i32;
    let x = (card.x - gutter).clamp(work.left, (work.right - w).max(work.left));
    let below = card.y + card.height + gap - gutter;
    let y = if below + h <= work.bottom { below } else { (card.y - gap + gutter - h).max(work.top) };
    Area::new(x, y, x + w, y + h)
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

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area::new(0, 0, 1920, 1040);

    #[test]
    fn tooltip_sits_under_the_card_with_its_shadow_gutter_outside() {
        let card = ScreenRect { x: 100, y: 200, width: 197, height: 118 };
        let a = place(card, (376, 120), WORK, 1.0);
        assert_eq!((a.left, a.top), (92, 312));
    }

    #[test]
    fn tooltip_flips_above_the_card_when_the_monitor_ends() {
        let card = ScreenRect { x: 100, y: 900, width: 197, height: 118 };
        let a = place(card, (376, 120), WORK, 1.0);
        assert_eq!(a.bottom, 906);
    }

    #[test]
    fn tooltip_stays_on_the_monitor_of_the_card() {
        let work = Area::new(1920, 0, 3840, 1040);
        let card = ScreenRect { x: 3700, y: 10, width: 197, height: 58 };
        let a = place(card, (376, 120), work, 1.0);
        assert_eq!(a.right, 3840);
    }
}
