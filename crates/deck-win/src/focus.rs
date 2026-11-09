//! Fenster finden, nach vorn holen und per DWM umstylen (aus `platform/focus.py`).
//!
//! Beim Klick auf eine Kachel holt die App das richtige Fenster nach vorn: ein
//! VS-Code-Fenster (gefunden über den Titel) oder das Windows-Terminal-Fenster eines
//! Agenten (Klasse [`WT_WINDOW_CLASS`](crate::window::WT_WINDOW_CLASS), Titel = Slot).
//! Das einzelne Terminal in VS Code fokussiert die Extension über den Broker, nicht
//! dieses Modul.
//!
//! Nicht übernommen: `toplevel_hwnd` (Tk-Fenster-Id → echtes Fenster, gibt es in Tauri
//! nicht) und `restrict_resize_to_corner` (WndProc-Subclassing für den nativen Rahmen
//! des Tk-Panels; das Tauri-Fenster regelt seine Größenänderung selbst).

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
    DWMWCP_DONOTROUND, DWMWCP_ROUND, DWMWCP_ROUNDSMALL, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

use crate::guard;
use crate::types::{Hwnd, Result};
use crate::window::{
    WindowInfo, class_of, contains_all, describe, for_each_top_level, is_minimized, is_visible,
    pid_of, thread_of, title_of,
};

/// Das aktive Fenster, oder `None`, wenn gerade keins aktiv ist (etwa beim Wechsel).
pub fn foreground_hwnd() -> Option<Hwnd> {
    // SAFETY: reine Abfrage.
    Hwnd::from_h(unsafe { GetForegroundWindow() })
}

/// Das aktive Fenster mit Titel, Klasse und Prozess.
pub fn foreground() -> Option<WindowInfo> {
    foreground_hwnd().and_then(describe)
}

/// Erstes sichtbares Fenster (Z-Reihenfolge), dessen Titel ALLE `needles` enthält,
/// ohne Groß-/Kleinschreibung. So findet die App das VS-Code-Fenster eines Repos.
pub fn find_window(needles: &[&str]) -> Option<Hwnd> {
    let mut hit = None;
    for_each_top_level(|h| {
        let Some(h) = Hwnd::from_h(h) else {
            return true;
        };
        if is_visible(h) {
            let t = title_of(h);
            if !t.is_empty() && contains_all(&t, needles) {
                hit = Some(h);
                return false;
            }
        }
        true
    });
    hit
}

/// Titel aller sichtbaren Fenster, die alle `needles` enthalten. Gedacht für „welche
/// VS-Code-Fenster sind überhaupt noch offen?“: ein geschlossenes fehlt, ein minimiertes
/// oder gerade neu ladendes ist dabei.
pub fn list_titles(needles: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for_each_top_level(|h| {
        if let Some(h) = Hwnd::from_h(h)
            && is_visible(h)
        {
            let t = title_of(h);
            if !t.is_empty() && contains_all(&t, needles) {
                out.push(t);
            }
        }
        true
    });
    out
}

/// Sichtbares Fenster der Klasse `class` mit genau dem Titel `title` (auch minimiert).
/// Für die WT-Agentenfenster: `find_by_class_and_title(WT_WINDOW_CLASS, "A3")`.
pub fn find_by_class_and_title(class: &str, title: &str) -> Option<Hwnd> {
    let mut hit = None;
    for_each_top_level(|h| {
        let Some(h) = Hwnd::from_h(h) else {
            return true;
        };
        if is_visible(h) && class_of(h) == class && title_of(h) == title {
            hit = Some(h);
            return false;
        }
        true
    });
    hit
}

/// Alle sichtbaren Top-Level-Fenster eines Prozesses. Ohne Nebenwirkung: der Wächter
/// fragt so „lebt das Panel noch?“, ohne dem Nutzer den Fokus zu klauen.
pub fn windows_of_pid(pid: u32) -> Vec<Hwnd> {
    let mut out = Vec::new();
    for_each_top_level(|h| {
        if let Some(h) = Hwnd::from_h(h)
            && is_visible(h)
            && pid_of(h) == Some(pid)
        {
            out.push(h);
        }
        true
    });
    out
}

/// Hat der Prozess mindestens ein sichtbares Fenster?
pub fn has_window(pid: u32) -> bool {
    !windows_of_pid(pid).is_empty()
}

/// Holt `hwnd` zuverlässig nach vorn. `Ok(true)`, wenn es danach das aktive Fenster ist.
///
/// Windows verhindert normalerweise, dass ein Hintergrundprozess den Vordergrund an
/// sich reißt (dann blinkt nur die Taskleiste). Der Trick der Python-Fassung: den
/// eigenen Eingabe-Thread kurz an den des aktiven und den des Zielfensters hängen
/// (`AttachThreadInput`); für Windows kommt der Wunsch dann „von innen“.
///
/// Wiederhergestellt wird NUR ein minimiertes Fenster. Ein maximiertes oder Vollbild-
/// Fenster behält seine Größe, `SW_RESTORE` würde es sonst verkleinern.
pub fn focus_window(hwnd: Hwnd) -> Result<bool> {
    guard::check("focus_window")?;
    if hwnd.is_null() {
        return Ok(false);
    }
    let target = hwnd.h();
    // SAFETY: Alle Aufrufe nehmen Handles/Thread-Ids als Werte; ein ungültiges Handle
    // lässt sie scheitern, nicht abstürzen. Das Anhängen wird in jedem Fall gelöst.
    unsafe {
        if is_minimized(hwnd) {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        let cur = GetCurrentThreadId();
        let fg_thr = thread_of(fg);
        let tgt_thr = thread_of(target);
        // Ein Anhängen an sich selbst scheitert harmlos; das Ergebnis interessiert nicht.
        let _ = AttachThreadInput(cur, fg_thr, true);
        let _ = AttachThreadInput(cur, tgt_thr, true);
        let _ = BringWindowToTop(target);
        let _ = SetForegroundWindow(target);
        let _ = AttachThreadInput(cur, fg_thr, false);
        let _ = AttachThreadInput(cur, tgt_thr, false);
        Ok(GetForegroundWindow() == target)
    }
}

/// Erstes sichtbares Fenster des Prozesses nach vorn holen. `Ok(false)`, wenn es keins
/// gibt. Über die PID statt den Titel, damit kein fremdes Fenster mit ähnlichem Titel
/// getroffen wird (Einzelinstanz-Schutz: die zweite Instanz holt die erste nach vorn).
pub fn focus_pid(pid: u32) -> Result<bool> {
    guard::check("focus_pid")?;
    match windows_of_pid(pid).first() {
        Some(&h) => focus_window(h),
        None => Ok(false),
    }
}

/// Wie die Ecken eines Fensters aussehen (Windows 11, `DWMWA_WINDOW_CORNER_PREFERENCE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    Default,
    /// Eckig. Für die Agentenfenster: der Fokus-Rahmen ist eckig, um eine runde Ecke
    /// herum schiene dort der Hintergrund durch.
    Square,
    /// Standardradius (~8 px).
    Round,
    /// Kleiner Radius (~4 px), „leicht rund“, für das rahmenlose Panel.
    RoundSmall,
}

impl Corners {
    fn dwm(self) -> DWM_WINDOW_CORNER_PREFERENCE {
        match self {
            Corners::Default => DWMWCP_DEFAULT,
            Corners::Square => DWMWCP_DONOTROUND,
            Corners::Round => DWMWCP_ROUND,
            Corners::RoundSmall => DWMWCP_ROUNDSMALL,
        }
    }
}

/// Ecken setzen, auch bei fremden und rahmenlosen Fenstern. DWM glättet die Rundung
/// (anders als `SetWindowRgn`, das treppt). `Ok(false)` auf Windows ohne das Attribut
/// (vor Windows 11), das Fenster bleibt dann, wie es ist.
pub fn set_corners(hwnd: Hwnd, corners: Corners) -> Result<bool> {
    guard::check("set_corners")?;
    Ok(dwm_set(
        hwnd.h(),
        DWMWA_WINDOW_CORNER_PREFERENCE,
        corners.dwm().0 as u32,
    ))
}

/// Kurzform für [`Corners::Square`].
pub fn square_corners(hwnd: Hwnd) -> Result<bool> {
    set_corners(hwnd, Corners::Square)
}

/// Den Fensterrand deckend in `rgb` (`0xRRGGBB`) malen. Der Win11-Rand ist sonst
/// halbdurchsichtig und rund 1 px (bei 150 % 2 px) breit: zwischen zwei gekachelten
/// Fenstern schien dort das Hintergrundbild durch. „Kein Rand“ (`DWMWA_COLOR_NONE`)
/// hilft nicht, die Fläche bleibt dann durchsichtig (gemessen 2026-11-09).
/// `Ok(false)` vor Windows 11.
pub fn set_border_color(hwnd: Hwnd, rgb: u32) -> Result<bool> {
    guard::check("set_border_color")?;
    Ok(dwm_set(hwnd.h(), DWMWA_BORDER_COLOR, rgb_to_colorref(rgb)))
}

/// Läuft Windows für Apps im hellen Modus? `None`, wenn der Wert fehlt.
pub fn apps_use_light_theme() -> Option<bool> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: Zeiger auf lokale Variablen, Größe passend.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            windows::core::w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    err.is_ok().then_some(value != 0)
}

/// Wie die native Titelleiste aussehen soll. Farben als `0xRRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TitlebarStyle {
    pub dark: bool,
    pub border: Option<u32>,
    pub caption: Option<u32>,
    pub text: Option<u32>,
    pub corners: Option<Corners>,
}

impl Default for TitlebarStyle {
    fn default() -> Self {
        TitlebarStyle {
            dark: true,
            border: None,
            caption: None,
            text: None,
            corners: Some(Corners::Round),
        }
    }
}

/// Native Titelleiste umstylen: dunkel, farbiger Rand, eigene Caption- und Textfarbe.
/// Jede Eigenschaft wirkt einzeln und fällt auf älterem Windows still weg (Rand,
/// Caption und Text brauchen Windows 11 22000+).
pub fn style_titlebar(hwnd: Hwnd, style: &TitlebarStyle) -> Result<()> {
    guard::check("style_titlebar")?;
    if hwnd.is_null() {
        return Ok(());
    }
    let h = hwnd.h();
    if style.dark {
        dwm_set(h, DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
        dwm_set(h, DWMWINDOWATTRIBUTE(19), 1); // ältere Builds nutzten 19 für Dark Mode
    }
    if let Some(c) = style.corners {
        dwm_set(h, DWMWA_WINDOW_CORNER_PREFERENCE, c.dwm().0 as u32);
    }
    for (attr, color) in [
        (DWMWA_BORDER_COLOR, style.border),
        (DWMWA_CAPTION_COLOR, style.caption),
        (DWMWA_TEXT_COLOR, style.text),
    ] {
        if let Some(rgb) = color {
            dwm_set(h, attr, rgb_to_colorref(rgb));
        }
    }
    Ok(())
}

/// `0xRRGGBB` → Windows-`COLORREF` (`0x00BBGGRR`).
pub const fn rgb_to_colorref(rgb: u32) -> u32 {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    r | (g << 8) | (b << 16)
}

/// `"#RRGGBB"` (Raute optional) → `0xRRGGBB`; alles andere → `None`.
pub fn parse_hex_color(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

/// Ein DWORD-Attribut setzen; `false`, wenn DWM es nicht kennt oder ablehnt.
fn dwm_set(hwnd: HWND, attr: DWMWINDOWATTRIBUTE, value: u32) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    // SAFETY: Zeiger auf einen lokalen u32, Größe passend.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        )
        .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Error;

    #[test]
    fn colorref_swaps_red_and_blue() {
        assert_eq!(rgb_to_colorref(0x7ECBFF), 0xFFCB7E);
        assert_eq!(rgb_to_colorref(0x000000), 0);
    }

    #[test]
    fn hex_colors_need_six_hex_digits() {
        assert_eq!(parse_hex_color("#7ecbff"), Some(0x7ECBFF));
        assert_eq!(parse_hex_color("D97757"), Some(0xD97757));
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("#gg0000"), None);
    }

    #[test]
    fn focusing_and_restyling_are_blocked_in_tests() {
        assert!(matches!(focus_window(Hwnd(1)), Err(Error::Forbidden(_))));
        assert!(matches!(focus_pid(1), Err(Error::Forbidden(_))));
        assert!(matches!(square_corners(Hwnd(1)), Err(Error::Forbidden(_))));
        assert!(matches!(
            style_titlebar(Hwnd(1), &TitlebarStyle::default()),
            Err(Error::Forbidden(_))
        ));
    }

    #[test]
    fn a_process_without_windows_has_none() {
        // PID 0 ist der Leerlaufprozess, er besitzt kein Fenster.
        assert!(!has_window(0));
    }
}
