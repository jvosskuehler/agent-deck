//! Die Win32-Schicht des Agent Deck: Fenster finden, fokussieren und anordnen, Monitore
//! und DPI, Prozesse, Tasten in fremde Konsolen, der Fokus-Rahmen, globale
//! Tastenkürzel und DPAPI. Portiert aus `deck/platform/` der Python-Fassung.
//!
//! Für die App gilt: was nach außen wirkt, ruft sie über die Traits in [`effects`] auf,
//! damit Tests Attrappen bekommen. Zusätzlich sperrt [`forbid_effects`] alle echten
//! Eingriffe prozessweit; in den Unit-Tests dieser Crate sind sie immer gesperrt.
//!
//! Koordinaten sind echte Bildschirmpixel (Per-Monitor-V2, siehe [`dpi`]), Rechtecke
//! immer (links, oben, rechts, unten), siehe [`Rect`].
//!
//! Der Fokus-Rahmen folgt dem Stand der Python-Fassung vom 2026-11-06 (vier animierte
//! Designs, gezeichnet auf der Grafikkarte): [`frame_fx`] rechnet je Bild die Parameter,
//! [`frame_gpu_overlay`] zeigt sie mit dem Shader `frame.hlsl` über DirectComposition
//! in einem Fenster aus [`frame_window`]. Vom ersten Advent bis Dreikönig laufen an
//! ihrer Stelle drei Winter-Designs mit Schnee ([`frame_fx_winter`], `frame_winter.hlsl`).
//!
//! Nicht portiert, weil es nur für tkinter nötig war: Per-Pixel-Alpha für das
//! Griff-Fenster (`layered.py`; der Griff ist jetzt WebView), Timer-Auflösung
//! (`timing.py`), Fensterbeschnitt (`clip.py`), der Tk-Oberflächenfaktor in `dpi.py`.
//! Ebenso der UI-Automation-Rückfall `wt_uia.py`: jeder Agent hat heute ein eigenes
//! Windows-Terminal-Fenster.

#![cfg(windows)]

pub mod arrange;
pub mod clock;
pub mod console_input;
pub mod dpapi;
pub mod dpi;
pub mod effects;
pub mod focus;
pub mod frame_fx;
pub mod frame_fx_winter;
pub mod frame_gpu;
pub mod frame_gpu_overlay;
pub mod frame_layout;
pub mod frame_window;
pub mod hotkey;
pub mod idle;
pub mod monitor;
pub mod proc_env;
pub mod proc_scan;
pub mod procs;
pub mod shadow_window;
pub mod taskbar;
pub mod tcp_listen;
pub mod which;
pub mod window;

mod guard;
mod types;

pub use guard::{effects_forbidden, forbid_effects};
pub use types::{Error, Hwnd, Rect, Result};
pub use window::{WT_WINDOW_CLASS, WindowInfo};
