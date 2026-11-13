//! Mehrere Agentenfenster auf einmal zeigen (Taste „alle nach vorn“): wiederherstellen,
//! an ihre Kacheln legen, über die anderen Fenster heben, keines aktivieren.
//!
//! Ziel (Wunsch vom 12.11.2026): man sieht die Fenster weder einzeln auftauchen noch
//! sich auf ihre Größe zurechtrücken, sondern den fertigen Block auf einmal. Darum
//! passiert alles unter dem [`Veil`], einem Standbild des Bildschirms:
//!
//! 1. Foto über alle Monitore, die die Fenster jetzt oder danach berühren.
//! 2. Minimierte kommen gleich an ihrem Zielplatz zurück (`SetWindowPlacement`,
//!    asynchron, alle zugleich), ohne Animation.
//! 3. Legen mit `DeferWindowPos`, alle in einem Zug, so oft, bis jedes Fenster seine
//!    Kachel trifft. Zieht ein Fenster auf einen Monitor mit anderer Skalierung, passt
//!    es beim ersten Zug seine Größe an die neue DPI an; erst ein weiterer trifft.
//! 4. Kurz warten, bis Windows Terminal in der neuen Größe gezeichnet hat.
//! 5. Heben: direkt unter das Foto, dann aus „immer im Vordergrund“ zurück. Nur so legt
//!    Windows ein fremdes Fenster über das aktive, ohne es zu aktivieren.
//! 6. Foto ausblenden.
//!
//! Gibt es den Schleier nicht (geht nicht), läuft dasselbe ohne ihn. Das Verstecken
//! ([`hide_together`]) geht denselben Weg rückwärts.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, GetForegroundWindow, GetWindowPlacement, HWND_NOTOPMOST,
    HWND_TOPMOST, SET_WINDOW_POS_FLAGS, SW_MINIMIZE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_ASYNCWINDOWPOS,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPlacement, SetWindowPos, ShowWindow,
    ShowWindowAsync, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT,
};

use crate::focus::focus_window;
use crate::arrange::{frame_insets, no_transitions, outer_for_visible, remembered_insets, visible_rect, window_rect};
use crate::guard;
use crate::monitor::monitors;
use crate::types::{Error, Hwnd, Rect, Result};
use crate::veil::Veil;
use crate::window::{is_maximized, is_minimized};

/// So lange blendet das Foto am Ende aus: kurz genug für „zack“, lang genug, dass der
/// Wechsel nicht blitzt.
const FADE: Duration = Duration::from_millis(90);
/// Höchstens so viele Züge (je 20 ms), bis alle Fenster ihre Kachel treffen; danach
/// geht das Foto trotzdem weg.
const MAX_ROUNDS: usize = 20;
/// Zeit für Windows Terminal, nach dem letzten Zug in der neuen Größe zu zeichnen.
const PAINT: Duration = Duration::from_millis(70);
/// Abweichung in Pixeln, die noch als „trifft die Kachel“ gilt.
const SLACK: i32 = 2;

/// Siehe Modulkopf. `None` als Ziel = das Fenster bleibt, wo es ist, und wird nur
/// gehoben.
pub fn show_together(moves: &[(Hwnd, Option<Rect>)]) -> Result<()> {
    guard::check("show_together")?;
    let veil = Veil::cover(&touched_monitors(moves));
    let wait = |d: Duration| match &veil {
        Some(v) => v.wait(d),
        None => std::thread::sleep(d),
    };

    let mut waking: Vec<Hwnd> = Vec::new();
    for (h, target) in moves {
        no_transitions(*h);
        if is_minimized(*h) {
            wake_at(*h, target.map(|t| outer_for_visible(t, remembered_insets(*h))));
            waking.push(*h);
        } else if target.is_some() && is_maximized(*h) {
            // SAFETY: Handle als Wert; Windows prüft es selbst.
            let _ = unsafe { ShowWindow(h.h(), SW_SHOWNOACTIVATE) };
        }
    }
    let until = Instant::now() + Duration::from_millis(500);
    while waking.iter().any(|h| is_minimized(*h)) && Instant::now() < until {
        wait(Duration::from_millis(8));
    }

    for round in 0..MAX_ROUNDS {
        let off: Vec<(Hwnd, Option<Rect>)> =
            moves.iter().filter(|(h, t)| t.is_some_and(|t| !hits(*h, t))).copied().collect();
        if off.is_empty() {
            break;
        }
        // Unter dem Schleier asynchron: jedes Terminal zieht in seinem eigenen Takt um,
        // keines kann das Deck (und damit das Foto) aufhalten. Ohne Schleier lieber
        // alle in einem Zug, das sieht man.
        if veil.is_some() || round > 0 {
            place_async(&off);
        } else {
            defer_all(&off, None)?;
        }
        wait(Duration::from_millis(20));
    }
    if veil.is_some() {
        wait(PAINT);
    }

    // Heben: unter das Foto (ohne Schleier: „immer im Vordergrund“), dann zurück.
    let above = veil.as_ref().and_then(Veil::bottom).map(|h| h.h()).unwrap_or(HWND_TOPMOST);
    defer_all(moves, Some(above))?;
    defer_all(moves, Some(HWND_NOTOPMOST))?;
    if let Some(v) = veil {
        v.on_top();
        v.wait(Duration::from_millis(16));
        v.lift(FADE);
    }
    Ok(())
}

/// Taste „alle nach vorn“, Verstecken: dasselbe Bild wie beim Zeigen, nur umgekehrt.
/// Unter dem [`Veil`] zurück nach `back` (das Ausgangsfenster kommt dabei nach vorn,
/// was man sonst kurz vor den Terminals aufblitzen sähe), dann alle `hwnds` minimieren,
/// ohne Animation und asynchron (ein hängendes Terminal hält das Foto nicht auf). Sind
/// alle weg, blendet das Foto aus: der Block verschwindet auf einmal.
pub fn hide_together(hwnds: &[Hwnd], back: Option<Hwnd>) -> Result<()> {
    guard::check("hide_together")?;
    let moves: Vec<(Hwnd, Option<Rect>)> = hwnds.iter().filter(|h| !is_minimized(**h)).map(|h| (*h, None)).collect();
    let veil = Veil::cover(&touched_monitors(&moves));
    if let Some(b) = back {
        let _ = focus_window(b);
    }
    for (h, _) in &moves {
        no_transitions(*h);
        // SAFETY: Handle als Wert; Windows prüft es selbst.
        unsafe {
            // Hat ein Terminal doch die Tastatur (kein Ausgangsfenster), reicht Windows
            // sie mit SW_MINIMIZE weiter; sonst tippte man ins Versteckte.
            let cmd = if GetForegroundWindow() == h.h() { SW_MINIMIZE } else { SW_SHOWMINNOACTIVE };
            let _ = ShowWindowAsync(h.h(), cmd);
        }
    }
    let Some(v) = veil else { return Ok(()) };
    let until = Instant::now() + Duration::from_millis(500);
    while moves.iter().any(|(h, _)| !is_minimized(*h)) && Instant::now() < until {
        v.wait(Duration::from_millis(8));
    }
    // Was darunter frei wird, zeigt DWM aus seinem Puffer; ein Bild Luft reicht.
    v.wait(Duration::from_millis(30));
    v.on_top();
    v.lift(FADE);
    Ok(())
}

/// Ganze Monitore, die eines der Fenster jetzt berührt (offen) oder nach dem Umzug
/// berühren wird.
fn touched_monitors(moves: &[(Hwnd, Option<Rect>)]) -> Vec<Rect> {
    let rects: Vec<Rect> = moves
        .iter()
        .flat_map(|(h, t)| {
            let now = if is_minimized(*h) { None } else { window_rect(*h) };
            [now, *t]
        })
        .flatten()
        .collect();
    monitors()
        .into_iter()
        .map(|m| m.rect)
        .filter(|m| rects.iter().any(|r| r.intersect(m).is_some()))
        .collect()
}

/// Jedes Fenster an sein Ziel, ohne auf das Terminal zu warten (`SWP_ASYNCWINDOWPOS`).
fn place_async(moves: &[(Hwnd, Option<Rect>)]) {
    for (h, t) in moves {
        if let Some(t) = t {
            let o = outer_for_visible(*t, frame_insets(*h));
            // SAFETY: Handle als Wert; Windows prüft es selbst.
            let _ = unsafe {
                SetWindowPos(
                    h.h(),
                    None,
                    o.left,
                    o.top,
                    o.width(),
                    o.height(),
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS,
                )
            };
        }
    }
}

/// Liegt der sichtbare Rahmen des Fensters auf `target` (bis auf [`SLACK`])?
fn hits(hwnd: Hwnd, target: Rect) -> bool {
    !is_minimized(hwnd)
        && visible_rect(hwnd).is_some_and(|v| {
            (v.left - target.left).abs() <= SLACK
                && (v.top - target.top).abs() <= SLACK
                && (v.right - target.right).abs() <= SLACK
                && (v.bottom - target.bottom).abs() <= SLACK
        })
}

/// Ein minimiertes Fenster asynchron wiederherstellen, ohne es zu aktivieren, mit
/// `outer` als neuem Platz (`None` = sein alter). Windows meint mit `rcNormalPosition`
/// Arbeitsflächen-Koordinaten; bei der Taskleiste unten ist das dasselbe wie der
/// Bildschirm, sonst richtet es der nächste Zug.
fn wake_at(hwnd: Hwnd, outer: Option<Rect>) {
    let mut p = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    // SAFETY: Zeiger auf lokale Variable mit gesetzter Länge; Handle als Wert.
    unsafe {
        if GetWindowPlacement(hwnd.h(), &mut p).is_err() {
            let _ = ShowWindow(hwnd.h(), SW_SHOWNOACTIVATE);
            return;
        }
        if let Some(o) = outer {
            p.rcNormalPosition = RECT { left: o.left, top: o.top, right: o.right, bottom: o.bottom };
        }
        p.showCmd = SW_SHOWNOACTIVATE.0 as u32;
        p.flags = WPF_ASYNCWINDOWPLACEMENT;
        if SetWindowPlacement(hwnd.h(), &p).is_err() {
            let _ = ShowWindow(hwnd.h(), SW_SHOWNOACTIVATE);
        }
    }
}

/// Ein Zug `DeferWindowPos` über alle Fenster: an ihr Ziel (Rand neu gemessen) und, mit
/// `after`, in der Z-Ordnung dahinter. Schlägt der Sammelaufruf fehl, einzeln.
fn defer_all(moves: &[(Hwnd, Option<Rect>)], after: Option<HWND>) -> Result<()> {
    let z = if after.is_some() { SWP_NOACTIVATE } else { SWP_NOACTIVATE | SWP_NOZORDER };
    let plan: Vec<(Hwnd, Rect, SET_WINDOW_POS_FLAGS)> = moves
        .iter()
        .filter(|(_, t)| t.is_some() || after.is_some())
        .map(|(h, t)| match t {
            Some(t) => (*h, outer_for_visible(*t, frame_insets(*h)), z),
            None => (*h, Rect::default(), z | SWP_NOMOVE | SWP_NOSIZE),
        })
        .collect();
    if plan.is_empty() {
        return Ok(());
    }
    // SAFETY: Handles als Werte; jedes HDWP stammt aus dem vorigen Aufruf und wird genau
    // einmal mit EndDeferWindowPos abgeschlossen (bei Fehler hat Windows es schon freigegeben).
    unsafe {
        if let Ok(mut hdwp) = BeginDeferWindowPos(plan.len() as i32) {
            let mut ok = true;
            for (h, o, flags) in &plan {
                match DeferWindowPos(hdwp, h.h(), after, o.left, o.top, o.width(), o.height(), *flags) {
                    Ok(next) => hdwp = next,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && EndDeferWindowPos(hdwp).is_ok() {
                return Ok(());
            }
        }
        let mut last = Ok(());
        for (h, o, flags) in &plan {
            last = SetWindowPos(h.h(), after, o.left, o.top, o.width(), o.height(), *flags)
                .map_err(|e| Error::win("SetWindowPos", e));
        }
        last
    }
}
