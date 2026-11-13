//! Das Fenster für den Gruppenschatten (`deck_core::group_shadow`): ein durchklickbares,
//! nie aktives Fenster, das direkt HINTER dem untersten Agentenfenster einer Gruppe liegt
//! und dort ein fertiges Bild mit Deckung je Pixel zeigt (`UpdateLayeredWindow`).
//!
//! Wie der Fokus-Rahmen ein rohes Win32-Fenster ([`crate::frame_window`]) und an den
//! Thread gebunden, der es anlegt; dessen Schleife ruft `pump_messages`.

use std::marker::PhantomData;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WINDOW_EX_STYLE,
};
use windows::core::w;

use crate::frame_window::{clip_to_work_areas, create_layer};
use crate::guard;
use crate::types::{Error, Hwnd, Rect, Result};

/// Ein Schattenfenster. Unsichtbar, bis [`ShadowWindow::show`] ein Bild bekommt.
pub struct ShadowWindow {
    hwnd: HWND,
    shown: bool,
    _not_send: PhantomData<*const ()>,
}

impl ShadowWindow {
    pub fn new() -> Result<ShadowWindow> {
        guard::check("shadow_window")?;
        let hwnd = create_layer(w!("AgentDeckGroupShadow"), WINDOW_EX_STYLE(0))?;
        Ok(ShadowWindow { hwnd, shown: false, _not_send: PhantomData })
    }

    /// Das Bild `bgra` (vormultipliziert, Zeilen von oben, genau `area` groß) auf die
    /// Fläche `area` legen und das Fenster direkt hinter `below` zeigen.
    pub fn show(&mut self, area: Rect, bgra: &[u8], below: Hwnd) -> Result<()> {
        guard::check("shadow_window")?;
        let (w, h) = (area.width(), area.height());
        if w <= 0 || h <= 0 || bgra.len() != (w as usize) * (h as usize) * 4 {
            return Err(Error::Helper(format!("Schattenbild passt nicht zur Fläche {w}×{h}")));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negativ = Zeilen von oben nach unten.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: Die GDI-Objekte leben nur in diesem Block und werden in jedem Fall
        // freigegeben; `bits` zeigt auf genau w*h*4 Bytes, so viel wird kopiert.
        let done = unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let done = match CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(dib) if !bits.is_null() => {
                    std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
                    let old = SelectObject(mem, dib.into());
                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    let done = UpdateLayeredWindow(
                        self.hwnd,
                        Some(screen),
                        Some(&POINT { x: area.left, y: area.top }),
                        Some(&SIZE { cx: w, cy: h }),
                        Some(mem),
                        Some(&POINT { x: 0, y: 0 }),
                        COLORREF(0),
                        Some(&blend),
                        ULW_ALPHA,
                    )
                    .map_err(|e| Error::win("UpdateLayeredWindow", e));
                    SelectObject(mem, old);
                    let _ = DeleteObject(dib.into());
                    done
                }
                Ok(dib) => {
                    let _ = DeleteObject(dib.into());
                    Err(Error::Helper("CreateDIBSection ohne Speicher".into()))
                }
                Err(e) => Err(Error::win("CreateDIBSection", e)),
            };
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            done
        };
        done?;
        // Der Schatten ragt unter dem Terminal hervor; die Taskleiste bleibt frei.
        clip_to_work_areas(self.hwnd, area);
        self.shown = true;
        self.restack(below)
    }

    /// Das Fenster (wieder) direkt hinter `below` legen, ohne es zu aktivieren.
    pub fn restack(&mut self, below: Hwnd) -> Result<()> {
        guard::check("shadow_window")?;
        if !self.shown {
            return Ok(());
        }
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW;
        // SAFETY: eigenes Fenster; ein ungültiges `below` lässt den Aufruf scheitern.
        unsafe { SetWindowPos(self.hwnd, Some(below.h()), 0, 0, 0, 0, flags) }.map_err(|e| Error::win("SetWindowPos", e))
    }

    pub fn hide(&mut self) {
        if std::mem::take(&mut self.shown) {
            // SAFETY: eigenes Fenster.
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    /// Das eigene Fenster, um es in der Fensterfolge wiederzufinden.
    pub fn hwnd(&self) -> Hwnd {
        Hwnd(self.hwnd.0 as isize)
    }
}

impl Drop for ShadowWindow {
    fn drop(&mut self) {
        // SAFETY: eigenes Fenster, im Thread, der es angelegt hat.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}
