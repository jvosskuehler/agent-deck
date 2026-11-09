//! Ein durchklickbares Fenster über einem fremden Fenster, das ein fertiges Bild zeigt
//! (aus `platform/overlay.py` und `platform/layered.py` der Python-Fassung, Stand vor
//! dem Wechsel auf das GPU-Hilfsprogramm).
//!
//! Ein rohes Win32-Fenster, kein Tauri-Fenster: das würde beim Einblenden nach vorn
//! geholt, und ein Rahmen, der dem Terminal den Fokus stiehlt, wäre schlimmer als
//! keiner. Fensterprozedur ist `DefWindowProcW`; das Bild kommt per
//! `UpdateLayeredWindow`, ein `WM_PAINT` braucht es nicht.
//!
//! Stil-Bits und warum:
//! - `WS_EX_LAYERED`: Bild mit Alpha je Pixel, innen durchsichtig.
//! - `WS_EX_TRANSPARENT`: Maus geht immer durch, auch auf dem Leuchten selbst.
//! - `WS_EX_NOACTIVATE`: wird nie aktiv, nimmt nie den Fokus.
//! - `WS_EX_TOOLWINDOW`: nicht in Taskleiste und Alt+Tab.
//! - `WS_EX_TOPMOST`: liegt über dem Terminal; gezeigt wird es nur, solange das
//!   Terminal ohnehin vorn ist, verdeckt also nichts Fremdes.
//!
//! **Thread-Bindung:** Ein Fenster gehört dem Thread, der es anlegt; dessen Schleife
//! muss [`pump_messages`] rufen. Sonst hängen Rundsendungen anderer Programme
//! (`WM_SETTINGCHANGE` & Co.) bis zu ihrem Timeout an diesem Fenster. Darum ist
//! [`Overlay`] nicht `Send`.
//!
//! **Falle aus der Python-Fassung:** nach Verstecken und Wiederzeigen lehnt
//! `UpdateLayeredWindow` mit Fehler 87 ab, obwohl Stil, Größe und Bild stimmen; die
//! Alpha-Schicht muss dann neu angelegt werden (das Bit wirklich 0 → 1 kippen, dasselbe
//! Bit erneut setzen genügt nicht). [`Overlay::show`] tut das beim Übergang von
//! versteckt nach sichtbar.

use std::marker::PhantomData;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP,
    HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_EXSTYLE, GetWindowLongW,
    HWND_TOPMOST, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongW, SetWindowPos,
    TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WINDOW_EX_STYLE, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::guard;
use crate::types::{Error, Hwnd, Rect, Result};

const CLASS: PCWSTR = w!("AgentDeckOverlay");

/// Ein Overlay-Fenster. Anfangs unsichtbar; [`Overlay::show`] legt Bild und Lage fest.
pub struct Overlay {
    hwnd: HWND,
    shown: bool,
    dib: Option<Dib>,
    _not_send: PhantomData<*const ()>,
}

/// Wiederverwendeter Bildpuffer: nicht je Bild neu anlegen, das liefe im Bildtakt.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: eigene GDI-Objekte, in umgekehrter Reihenfolge freigegeben.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: reine Weitergabe an die Standardbehandlung.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// Ein unsichtbares, durchklickbares, nie aktives Topmost-Popup der Klasse `class`
/// anlegen (Stil-Bits siehe Modulkopf), dazu `extra` Stil-Bits. Auch für das
/// GPU-Rahmenfenster ([`crate::frame_gpu_overlay`]).
pub(crate) fn create_click_through(class: PCWSTR, extra: WINDOW_EX_STYLE) -> Result<HWND> {
    let ex = WS_EX_LAYERED
        | WS_EX_TRANSPARENT
        | WS_EX_NOACTIVATE
        | WS_EX_TOOLWINDOW
        | WS_EX_TOPMOST
        | extra;
    // SAFETY: statische Klassennamen, Prozedur eine echte extern-Funktion. Die zweite
    // Registrierung derselben Klasse scheitert harmlos (ERROR_CLASS_ALREADY_EXISTS).
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| Error::win("GetModuleHandleW", e))?;
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            ex,
            class,
            w!("Agent Deck Fokus"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| Error::win("CreateWindowExW", e))
    }
}

impl Overlay {
    /// Das (noch unsichtbare) Fenster anlegen, auf dem aufrufenden Thread.
    pub fn new() -> Result<Overlay> {
        guard::check("overlay")?;
        let hwnd = create_click_through(CLASS, WINDOW_EX_STYLE(0))?;
        Ok(Overlay {
            hwnd,
            shown: false,
            dib: None,
            _not_send: PhantomData,
        })
    }

    pub fn hwnd(&self) -> Hwnd {
        Hwnd(self.hwnd.0 as isize)
    }

    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// Bild auf `rect` (Bildschirmkoordinaten) zeigen, ohne zu aktivieren. `bgra` sind
    /// `rect.width() * rect.height() * 4` Bytes, Zeile für Zeile von oben,
    /// VORMULTIPLIZIERT (sonst bekommen weiche Kanten einen hellen Saum).
    pub fn show(&mut self, rect: Rect, bgra: &[u8]) -> Result<()> {
        guard::check("overlay")?;
        let (w, h) = (rect.width(), rect.height());
        if w <= 0 || h <= 0 || bgra.len() != (w as usize) * (h as usize) * 4 {
            return Err(Error::Rejected(format!(
                "Bild passt nicht: {w}x{h}, {} Bytes",
                bgra.len()
            )));
        }
        if !self.shown {
            self.relayer();
        }
        let dib = self.dib(w, h)?;
        // SAFETY: Der DIB-Puffer hat genau w*h*4 Bytes (oben geprüft und so angelegt).
        unsafe { std::ptr::copy_nonoverlapping(bgra.as_ptr(), dib.bits, bgra.len()) };
        let dc_src = dib.dc;
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let (pos, size, src) = (
            POINT {
                x: rect.left,
                y: rect.top,
            },
            SIZE { cx: w, cy: h },
            POINT::default(),
        );
        // SAFETY: Zeiger auf lokale Strukturen; Bildschirm-DC wird freigegeben.
        unsafe {
            let screen = GetDC(None);
            let r = UpdateLayeredWindow(
                self.hwnd,
                Some(screen),
                Some(&pos),
                Some(&size),
                Some(dc_src),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
            r.map_err(|e| Error::win("UpdateLayeredWindow", e))?;
            if !self.shown {
                SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
                .map_err(|e| Error::win("SetWindowPos(zeigen)", e))?;
                self.shown = true;
            }
        }
        Ok(())
    }

    /// Nur verschieben, Bild und Größe bleiben. Hält das Fenster obenauf.
    pub fn move_to(&mut self, x: i32, y: i32) -> Result<()> {
        guard::check("overlay")?;
        // SAFETY: eigenes Fenster.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .map_err(|e| Error::win("SetWindowPos(verschieben)", e))
    }

    /// Verstecken (ohne Aktivierung, Z-Reihenfolge bleibt).
    pub fn hide(&mut self) {
        if self.shown {
            // SAFETY: eigenes Fenster.
            let _ = unsafe {
                SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_HIDEWINDOW,
                )
            };
            self.shown = false;
        }
    }

    /// `WS_EX_LAYERED` wirklich 0 → 1 kippen (siehe Modulkopf).
    fn relayer(&self) {
        // SAFETY: eigenes Fenster, nur Stil-Bits.
        unsafe {
            let ex = GetWindowLongW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as i32));
            SetWindowLongW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as i32);
        }
    }

    fn dib(&mut self, w: i32, h: i32) -> Result<&Dib> {
        if self.dib.as_ref().is_none_or(|d| d.w != w || d.h != h) {
            self.dib = None;
            self.dib = Some(new_dib(w, h)?);
        }
        Ok(self.dib.as_ref().expect("eben angelegt"))
    }
}

fn new_dib(w: i32, h: i32) -> Result<Dib> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // negativ = von oben nach unten
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: Bildschirm-DC wird freigegeben; bei Fehlern wird Angelegtes wieder gelöscht.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None, screen);
        let bitmap = match bitmap {
            Ok(b) if !bits.is_null() && !dc.is_invalid() => b,
            other => {
                if let Ok(b) = other {
                    let _ = DeleteObject(b.into());
                }
                let _ = DeleteDC(dc);
                return Err(Error::last("CreateDIBSection"));
            }
        };
        let old = SelectObject(dc, bitmap.into());
        Ok(Dib {
            dc,
            bitmap,
            old,
            bits: bits.cast(),
            w,
            h,
        })
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.dib = None;
        // SAFETY: eigenes Fenster, auf dem Thread zerstört, der es angelegt hat
        // (Overlay ist nicht Send).
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Alle anstehenden Nachrichten des aufrufenden Threads abarbeiten, ohne zu warten.
pub fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: lokale MSG-Struktur.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Bis zum nächsten Bildwechsel des Desktops warten (`DwmFlush`). Der Takt, an dem eine
/// Overlay-Animation hängen sollte: mehr Bilder zu rechnen, als der Monitor zeigt, macht
/// die Bewegung nicht glatter, nur ungleichmäßiger. `false`, wenn DWM nicht mitspielt
/// (dann hält der Aufrufer die Periode selbst ein).
pub fn wait_for_vblank() -> bool {
    // SAFETY: ohne Parameter.
    unsafe { DwmFlush() }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_cannot_be_created_in_tests() {
        assert!(matches!(Overlay::new(), Err(Error::Forbidden(_))));
    }
}
