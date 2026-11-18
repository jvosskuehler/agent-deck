//! Dialoge so groß wie ihr Inhalt, ohne dass sie dabei wandern.
//!
//! Die Ansicht misst sich (`useDialogFit`) und meldet Höhe und, bei den Einstellungen,
//! Breite. Zwei Fallen, beide am 18.11.2026 bei Jorrit gesehen:
//!
//! - **Wandern:** ragt ein wachsender Dialog unten hinaus, rückt er hoch. Früher blieb er
//!   beim Schrumpfen oben stehen; jedes „Erweitert“/„Weniger“ schob ihn weiter, bis er am
//!   oberen Rand klebte. Darum gilt ein **Anker**: die Lage beim Öffnen bzw. dort, wohin
//!   der Nutzer ihn gezogen hat. Jede Anpassung rechnet vom Anker aus und rückt nur so weit
//!   hoch wie nötig; schrumpft er, kehrt er an den Anker zurück.
//! - **Riesig beim ersten Öffnen:** der Dialog wird unsichtbar angelegt und landet beim
//!   Zeigen womöglich auf einem Monitor mit anderer Skalierung. Windows (tao) skaliert ihn
//!   dabei mit (WM_DPICHANGED), die gemessene Größe stimmt nicht mehr. Darum merkt sich
//!   [`fit`] die zuletzt gemeldete Inhaltsgröße, und [`refit`] legt sie nach dem Zeigen und
//!   nach jedem Skalierungswechsel neu an.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use deck_core::dock::Area;
use tauri::{PhysicalSize, WebviewWindow};

/// Je Fenster: zuletzt gemeldete Inhaltsgröße (CSS-Pixel), Anker, zuletzt selbst gesetzte Lage.
#[derive(Default, Clone, Copy)]
struct Fit {
    content: Option<(f64, Option<f64>)>,
    anchor: Option<(i32, i32)>,
    placed: Option<(i32, i32)>,
}

fn state() -> &'static Mutex<HashMap<String, Fit>> {
    static S: OnceLock<Mutex<HashMap<String, Fit>>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

fn with<R>(label: &str, f: impl FnOnce(&mut Fit) -> R) -> R {
    let mut map = state().lock().unwrap_or_else(|p| p.into_inner());
    f(map.entry(label.to_string()).or_default())
}

/// Lage, an der ein Fenster der Höhe/Breite `h`/`w` (physisch, samt Rahmen) ab `anchor`
/// steht: so weit hoch bzw. nach links gerückt, dass es in `area` passt, nie über deren
/// oberen bzw. linken Rand hinaus. Reine Rechnung, getestet.
pub fn place(anchor: (i32, i32), size: (u32, u32), area: (i32, i32, u32, u32)) -> (i32, i32) {
    let (ax, ay) = anchor;
    let (left, top, aw, ah) = area;
    let right = left + aw as i32;
    let bottom = top + ah as i32;
    let x = if ax + size.0 as i32 > right { (right - size.0 as i32).max(left) } else { ax };
    let y = if ay + size.1 as i32 > bottom { (bottom - size.1 as i32).max(top) } else { ay };
    (x, y)
}

/// Inhaltsgröße melden: merken und anwenden.
pub fn fit(w: &WebviewWindow, height: f64, width: Option<f64>) -> tauri::Result<()> {
    with(w.label(), |f| f.content = Some((height, width)));
    apply(w)
}

/// Nach dem Zeigen: Anker = jetzige Lage, dann die gemerkte Größe neu anlegen.
pub fn shown(w: &WebviewWindow) {
    if let Ok(p) = w.outer_position() {
        with(w.label(), |f| f.anchor = Some((p.x, p.y)));
    }
    let _ = apply(w);
}

/// Skalierung gewechselt: gemerkte Größe neu anlegen (Windows hat das Fenster mitskaliert).
pub fn refit(w: &WebviewWindow) {
    let _ = apply(w);
}

/// Fenster bewegt. Eine Lage, die wir nicht selbst gesetzt haben, hat der Nutzer gewählt:
/// sie wird der neue Anker.
pub fn moved(label: &str, x: i32, y: i32) {
    with(label, |f| {
        if f.placed != Some((x, y)) {
            f.anchor = Some((x, y));
        }
    });
}

fn apply(w: &WebviewWindow) -> tauri::Result<()> {
    let Fit { content, anchor, .. } = with(w.label(), |f| *f);
    let Some((height, width)) = content else { return Ok(()) };
    let scale = w.scale_factor()?;
    let inner = w.inner_size()?;
    let outer = w.outer_size()?;
    // Titelleiste und Rahmen (physisch): Differenz zwischen Außen- und Innenmaß.
    let chrome = outer.height.saturating_sub(inner.height);
    let chrome_x = outer.width.saturating_sub(inner.width);
    let mut want = (height * scale).ceil() as u32;
    let mut want_w = width.map_or(inner.width, |x| (x * scale).round() as u32);
    if let Some(m) = w.current_monitor()? {
        let area = m.work_area();
        want = want.min(area.size.height.saturating_sub(chrome));
        want_w = want_w.min(area.size.width.saturating_sub(chrome_x));
        let pos = w.outer_position()?;
        let from = anchor.unwrap_or((pos.x, pos.y));
        let to = place(from, (want_w + chrome_x, want + chrome), (area.position.x, area.position.y, area.size.width, area.size.height));
        // Rundung: ein Pixel Unterschied ist kein Grund, das Fenster neu zu setzen.
        let resize = want.abs_diff(inner.height) > 1 || want_w.abs_diff(inner.width) > 1;
        if to == (pos.x, pos.y) && !resize {
            return Ok(());
        }
        // Lage und Größe in EINEM Schritt (ein SetWindowPos): getrennt sähe man das Fenster
        // erst springen, dann wachsen.
        with(w.label(), |f| f.placed = Some(to));
        let (ow, oh) = (want_w + chrome_x, want + chrome);
        let hwnd = w.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        crate::native::move_to(hwnd, Area { left: to.0, top: to.1, right: to.0 + ow as i32, bottom: to.1 + oh as i32 });
        return Ok(());
    }
    if want.abs_diff(inner.height) > 1 || want_w.abs_diff(inner.width) > 1 {
        w.set_size(PhysicalSize::new(want_w, want))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::place;

    const AREA: (i32, i32, u32, u32) = (0, 0, 1920, 1040);

    #[test]
    fn a_window_that_fits_stays_at_its_anchor() {
        assert_eq!(place((500, 300), (500, 600), AREA), (500, 300));
    }

    #[test]
    fn a_growing_window_moves_up_only_as_far_as_needed() {
        assert_eq!(place((500, 600), (1000, 700), AREA), (500, 340));
    }

    #[test]
    fn shrinking_again_returns_to_the_anchor_instead_of_creeping_up() {
        let anchor = (500, 600);
        let wide = place(anchor, (1000, 700), AREA);
        let small = place(anchor, (500, 400), AREA);
        assert_eq!(wide, (500, 340));
        assert_eq!(small, anchor, "zurück an den Anker, nicht oben kleben");
        for _ in 0..5 {
            assert_eq!(place(anchor, (1000, 700), AREA), wide, "Hin und Her wandert nicht");
        }
    }

    #[test]
    fn never_above_or_left_of_the_work_area() {
        assert_eq!(place((1500, 900), (2500, 1500), AREA), (0, 0));
    }
}
