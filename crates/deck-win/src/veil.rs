//! Der Schleier: ein Standbild der Monitore, das über allem liegt, solange darunter
//! Fenster umziehen (Taste „alle nach vorn“, [`crate::together`]).
//!
//! Fremde Fenster lassen sich nicht unsichtbar verschieben: `DWMWA_CLOAK` gilt nur für
//! eigene Fenster, und Windows Terminal halbdurchsichtig zu schalten stört seine
//! Darstellung. Also umgekehrt: erst ein Foto des Bildschirms davor legen, dann darunter
//! wiederherstellen, verschieben und warten, bis jedes Fenster seine Größe hat und neu
//! gezeichnet ist, zuletzt das Foto wegnehmen. Wer hinschaut, sieht den alten Stand
//! stehen bleiben und dann den neuen auf einmal, ohne Zwischenschritte.
//!
//! Je Monitor ein Fenster aus [`crate::frame_window`] (durchklickbar, nie aktiv,
//! topmost) mit dem Foto als Layered-Bitmap (`UpdateLayeredWindow`). Ausblenden über die
//! Deckkraft, Bild für Bild im Takt von DWM (`DwmFlush`). Das Foto kommt per `BitBlt`
//! vom Bildschirm-DC: seit DWM ist das das fertig zusammengesetzte Bild, Terminals
//! eingeschlossen.
//!
//! Die Fenster gehören dem Thread, der den Schleier anlegt; er pumpt beim Warten
//! ([`crate::frame_window::pump_messages`]). Der Schleier lebt nur Sekundenbruchteile.

use std::marker::PhantomData;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmFlush, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_OVER, BLENDFUNCTION, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WINDOW_EX_STYLE,
};
use windows::core::w;

use crate::frame_window::{create_click_through, pump_messages};
use crate::guard;
use crate::types::{Hwnd, Rect};

/// Ein Monitor unter dem Schleier: sein Fenster und das Foto.
struct Pane {
    hwnd: HWND,
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
}

/// Das Standbild über den Monitoren. Weg ist es mit [`Veil::lift`] oder beim Drop
/// (dann ohne Ausblenden).
pub struct Veil {
    panes: Vec<Pane>,
    _not_send: PhantomData<*const ()>,
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: 0 }
}

impl Veil {
    /// Die Flächen `areas` (ganze Monitore) fotografieren und das Foto darüberlegen.
    /// Kehrt erst zurück, wenn DWM es zeigt. `None`: ging nicht, dann eben ohne.
    pub fn cover(areas: &[Rect]) -> Option<Veil> {
        guard::check("veil").ok()?;
        let mut veil = Veil { panes: Vec::new(), _not_send: PhantomData };
        // SAFETY: Bildschirm-DC wird nach dem Kopieren freigegeben; Speicher-DC, Bitmap
        // und Fenster gehören dem Schleier und werden im Drop freigegeben.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return None;
            }
            for a in areas.iter().filter(|a| !a.is_empty()) {
                let dc = CreateCompatibleDC(Some(screen));
                let bmp = CreateCompatibleBitmap(screen, a.width(), a.height());
                let old = SelectObject(dc, bmp.into());
                let shot = BitBlt(dc, 0, 0, a.width(), a.height(), Some(screen), a.left, a.top, SRCCOPY).is_ok();
                let Ok(hwnd) = create_click_through(w!("AgentDeckVeil"), WINDOW_EX_STYLE(0)) else {
                    SelectObject(dc, old);
                    let _ = DeleteObject(bmp.into());
                    let _ = DeleteDC(dc);
                    break;
                };
                veil.panes.push(Pane { hwnd, dc, bmp, old });
                if !shot {
                    break;
                }
                let on: u32 = 1;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_TRANSITIONS_FORCEDISABLED,
                    (&raw const on).cast(),
                    size_of::<u32>() as u32,
                );
                let at = POINT { x: a.left, y: a.top };
                let size = SIZE { cx: a.width(), cy: a.height() };
                let origin = POINT::default();
                if UpdateLayeredWindow(
                    hwnd,
                    None,
                    Some(&at),
                    Some(&size),
                    Some(dc),
                    Some(&origin),
                    COLORREF(0),
                    Some(&blend(255)),
                    ULW_ALPHA,
                )
                .is_err()
                {
                    break;
                }
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            ReleaseDC(None, screen);
            if veil.panes.len() != areas.iter().filter(|a| !a.is_empty()).count() {
                return None;
            }
            veil.on_top();
            let _ = DwmFlush();
        }
        Some(veil)
    }

    /// Das Fenster, unter das Umziehende gelegt werden (über alle anderen, unter dem
    /// Foto).
    pub fn bottom(&self) -> Option<Hwnd> {
        self.panes.last().map(|p| Hwnd(p.hwnd.0 as isize))
    }

    /// Wieder ganz nach oben (falls ein anderes Topmost-Fenster sich davorgeschoben hat).
    pub fn on_top(&self) {
        for p in &self.panes {
            // SAFETY: eigenes Fenster.
            let _ = unsafe {
                SetWindowPos(p.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
            };
        }
    }

    /// Warten und dabei die Nachrichten der Schleier-Fenster abarbeiten.
    pub fn wait(&self, d: Duration) {
        let until = Instant::now() + d;
        while Instant::now() < until {
            pump_messages();
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    /// Das Foto wegnehmen, über `fade` ausgeblendet (0 = auf einen Schlag).
    pub fn lift(self, fade: Duration) {
        let start = Instant::now();
        while !fade.is_zero() {
            let t = start.elapsed().as_secs_f64() / fade.as_secs_f64();
            if t >= 1.0 {
                break;
            }
            let alpha = (255.0 * (1.0 - t)).round() as u8;
            for p in &self.panes {
                // SAFETY: eigenes Fenster; ohne Quelle ändert sich nur die Deckkraft.
                let _ = unsafe {
                    UpdateLayeredWindow(p.hwnd, None, None, None, None, None, COLORREF(0), Some(&blend(alpha)), ULW_ALPHA)
                };
            }
            pump_messages();
            // SAFETY: wartet auf das nächste Bild von DWM.
            if unsafe { DwmFlush() }.is_err() {
                std::thread::sleep(Duration::from_millis(16));
            }
        }
    }
}

impl Drop for Veil {
    fn drop(&mut self) {
        for p in self.panes.drain(..) {
            // SAFETY: alles eigene Objekte, genau einmal freigegeben.
            unsafe {
                let _ = DestroyWindow(p.hwnd);
                SelectObject(p.dc, p.old);
                let _ = DeleteObject(p.bmp.into());
                let _ = DeleteDC(p.dc);
            }
        }
        pump_messages();
    }
}
