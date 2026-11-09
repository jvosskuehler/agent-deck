//! Grundtypen der Crate: Fensterhandle, Rechteck, Fehler.
//!
//! `HWND` aus der `windows`-Crate ist ein roher Zeiger und damit weder `Send` noch `Sync`.
//! Die App reicht Fenster aber über Threads und Kanäle (Poll-Schleife, Arbeiter-Thread,
//! Tauri-Befehle). Darum führt die öffentliche Schnittstelle Handles als Zahl
//! ([`Hwnd`]); erst unmittelbar vor dem Win32-Aufruf wird daraus wieder ein `HWND`.

use windows::Win32::Foundation::{HWND, RECT};

/// Ein Fensterhandle als Zahl. Null heißt „kein Fenster“.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Hwnd(pub isize);

impl Hwnd {
    /// Der Zahlenwert, etwa fürs Protokoll oder eine Datei.
    pub const fn raw(self) -> isize {
        self.0
    }

    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn h(self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }

    pub(crate) fn from_h(h: HWND) -> Option<Self> {
        let raw = h.0 as isize;
        (raw != 0).then_some(Hwnd(raw))
    }
}

/// Rechteck in Bildschirmpixeln als (links, oben, rechts, unten), rechts und unten
/// ausschließlich, wie `RECT` in Win32.
///
/// Die Python-Fassung führte zwei Schreibweisen nebeneinander, (l, t, r, b) und
/// (x, y, w, h), und hat sie an einer Stelle auch verwechselt. Hier gibt es nur diese
/// eine; wer (x, y, w, h) hat, baut mit [`Rect::from_xywh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn from_xywh(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }

    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Leer heißt: keine Fläche (auch bei vertauschten Kanten).
    pub const fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    /// Mitte, ganzzahlig abgerundet (so rechnete die Python-Fassung `(l + r) // 2`).
    pub const fn center(&self) -> (i32, i32) {
        (
            (self.left + self.right).div_euclid(2),
            (self.top + self.bottom).div_euclid(2),
        )
    }

    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Schnittfläche, `None` wenn nichts übrig bleibt.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let r = Rect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        );
        (!r.is_empty()).then_some(r)
    }

    pub const fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.left + dx,
            self.top + dy,
            self.right + dx,
            self.bottom + dy,
        )
    }
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Rect::new(r.left, r.top, r.right, r.bottom)
    }
}

/// Fehler der Win32-Schicht. Bewusst ohne `windows::core::Error` darin: der trägt je
/// nach Fall COM-Fehlerinfo mit sich und ist dann nicht `Send`, die App will Fehler
/// aber über Threads reichen und ins Log schreiben.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Ein Win32-Aufruf ist gescheitert; `code` ist das HRESULT.
    #[error("{call} fehlgeschlagen: {message} (0x{code:08X})")]
    Win32 {
        call: &'static str,
        code: u32,
        message: String,
    },
    /// Die Eingabe wurde abgelehnt, bevor irgendetwas passiert ist (unplausibler
    /// Prozessname, Systemprozess, leeres Bild …).
    #[error("abgelehnt: {0}")]
    Rejected(String),
    /// Ein Hilfsprozess (taskkill, deck-console) hat nicht wie erwartet geantwortet.
    #[error("Hilfsprozess: {0}")]
    Helper(String),
    /// Echte Eingriffe sind in diesem Prozess gesperrt (Tests, siehe [`crate::forbid_effects`]).
    #[error("echter Eingriff gesperrt: {0}")]
    Forbidden(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn win(call: &'static str, e: windows::core::Error) -> Self {
        Error::Win32 {
            call,
            code: e.code().0 as u32,
            message: e.message(),
        }
    }
}

/// UTF-16-Puffer bis zur ersten Null als String (Ersatzzeichen statt Fehler).
pub(crate) fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xywh_and_ltrb_describe_the_same_rect() {
        let r = Rect::from_xywh(10, 20, 300, 200);
        assert_eq!(r, Rect::new(10, 20, 310, 220));
        assert_eq!((r.width(), r.height()), (300, 200));
    }

    #[test]
    fn center_rounds_down_also_for_negative_coordinates() {
        assert_eq!(Rect::new(-3, -3, 0, 0).center(), (-2, -2));
        assert_eq!(Rect::new(0, 0, 5, 5).center(), (2, 2));
    }

    #[test]
    fn disjoint_rects_have_no_intersection() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(a.intersect(&Rect::new(10, 0, 20, 10)), None);
        assert_eq!(
            a.intersect(&Rect::new(5, 5, 20, 20)),
            Some(Rect::new(5, 5, 10, 10))
        );
    }

    #[test]
    fn right_and_bottom_edges_are_exclusive() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(r.contains(9, 9));
        assert!(!r.contains(10, 5));
    }

    #[test]
    fn wide_strings_stop_at_the_first_nul() {
        let w: Vec<u16> = "Fenster ä\0Rest".encode_utf16().collect();
        assert_eq!(from_wide(&w), "Fenster ä");
        assert_eq!(from_wide(&[65, 0, 66]), "A");
    }

    #[test]
    fn null_handle_is_not_a_window() {
        assert!(Hwnd(0).is_null());
        assert_eq!(Hwnd::from_h(HWND(std::ptr::null_mut())), None);
    }
}
