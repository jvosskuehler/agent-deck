//! Der Fokus-Rahmen über DirectComposition: EIN durchklickbares Fenster über dem
//! Agentenfenster samt Hof, die Grafikkarte zeichnet nur den Rand, innen bleibt es
//! durchsichtig (Port von `App` in `focus-frame/Program.cs` und der Swapchain aus
//! `Renderer.cs` der Python-Fassung).
//!
//! Das ist der Weg, den die Python-Referenz seit 2026-11-06 geht, und der empfohlene für
//! die App: jeder Pixel auf der GPU im Takt des Monitors (`Present(1)` wartet auf den
//! Bildwechsel und ist zugleich der Takt der Animation), die CPU sagt nur, was.
//!
//! Das Fenster trägt `WS_EX_NOREDIRECTIONBITMAP`: es hat keinen eigenen Bildpuffer, sein
//! Inhalt ist allein der DirectComposition-Visual. Ohne `SetLayeredWindowAttributes`
//! zeigt Windows ein Layered-Fenster gar nicht an.
//!
//! Es gehört dem Thread, der es anlegt (nicht `Send`); dessen Schleife ruft
//! [`crate::frame_window::pump_messages`] und dann [`GpuFocusFrame::draw`].

use std::marker::PhantomData;

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Direct3D11::ID3D11RenderTargetView;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2,
    IDXGISwapChain1,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, HWND_TOPMOST, LWA_ALPHA, SW_HIDE, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    WS_EX_NOREDIRECTIONBITMAP,
};
use windows::core::{Interface, w};

use crate::arrange::visible_rect;
use crate::dpi;
use crate::frame_fx::Design;
use crate::frame_gpu::GpuPipeline;
use crate::frame_layout::FrameLayout;
use crate::guard;
use crate::monitor::monitor_of_window;
use crate::frame_window::create_click_through;
use crate::types::{Error, Hwnd, Rect, Result};
use crate::window::{is_minimized, is_window};

/// Swapchain samt Composition-Baum und Render-Ziel in passender Größe.
struct Surface {
    swap: IDXGISwapChain1,
    rtv: Option<ID3D11RenderTargetView>,
    size: (u32, u32),
    // Halten den Composition-Baum am Leben (in Freigabe-Reihenfolge nach `swap`).
    _visual: IDCompositionVisual,
    _target: IDCompositionTarget,
    _dcomp: IDCompositionDevice,
}

/// Das GPU-Rahmenfenster.
pub struct GpuFocusFrame {
    hwnd: HWND,
    gpu: GpuPipeline,
    surface: Option<Surface>,
    placed: Option<Rect>,
    _not_send: PhantomData<*const ()>,
}

impl GpuFocusFrame {
    /// Fenster und Grafikgerät anlegen (unsichtbar).
    pub fn new() -> Result<GpuFocusFrame> {
        guard::check("frame_gpu_overlay")?;
        let gpu = GpuPipeline::new()?;
        let hwnd = create_click_through(w!("AgentDeckFocusFrameGpu"), WS_EX_NOREDIRECTIONBITMAP)?;
        // SAFETY: eigenes Fenster.
        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) }
            .map_err(|e| Error::win("SetLayeredWindowAttributes", e))?;
        Ok(GpuFocusFrame {
            hwnd,
            gpu,
            surface: None,
            placed: None,
            _not_send: PhantomData,
        })
    }

    /// Ein Bild um `target` zeichnen und zeigen; wartet auf den Bildwechsel des
    /// Monitors. `night`: das Terminal ist dunkel (Farbe der Schneeflocken im Winter).
    /// `Ok(false)`: nichts zu zeigen (Fenster weg, minimiert, außerhalb), der Rahmen ist
    /// dann versteckt.
    pub fn draw(&mut self, target: Hwnd, design: Design, seed: i32, t: f64, night: bool) -> Result<bool> {
        guard::check("frame_gpu_overlay")?;
        let Some(layout) = layout_of(target) else {
            self.hide();
            return Ok(false);
        };
        let frame = layout.frame;
        let (w, h) = (frame.width() as u32, frame.height() as u32);
        let rtv = self.target_view(w, h)?;
        self.gpu.draw(&rtv, w, h, &layout.params(design, seed, t).with_night(night));
        let swap = &self.surface.as_ref().expect("eben angelegt").swap;
        // SAFETY: eigene Swapchain; Present(1) wartet auf den nächsten Bildwechsel.
        unsafe { swap.Present(1, DXGI_PRESENT(0)) }
            .ok()
            .map_err(|e| Error::win("Present", e))?;
        if self.placed != Some(frame) {
            // SAFETY: eigenes Fenster; zeigt es, ohne es zu aktivieren.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    frame.left,
                    frame.top,
                    frame.width(),
                    frame.height(),
                    SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOOWNERZORDER,
                )
            }
            .map_err(|e| Error::win("SetWindowPos", e))?;
            self.placed = Some(frame);
        }
        Ok(true)
    }

    /// Rahmen verstecken (Fenster und Gerät bleiben für das nächste Bild).
    pub fn hide(&mut self) {
        if self.placed.take().is_some() {
            // SAFETY: eigenes Fenster.
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    pub fn hwnd(&self) -> Hwnd {
        Hwnd(self.hwnd.0 as isize)
    }

    /// Render-Ziel in Größe w × h; legt die Swapchain beim ersten Mal an und passt sie
    /// sonst an.
    fn target_view(&mut self, w: u32, h: u32) -> Result<ID3D11RenderTargetView> {
        if self.surface.is_none() {
            self.surface = Some(self.create_surface(w, h)?);
        }
        let surface = self.surface.as_mut().expect("eben angelegt");
        if surface.size != (w, h) {
            surface.rtv = None;
            self.gpu.unbind();
            // SAFETY: Alle Verweise auf die Puffer sind oben freigegeben.
            unsafe {
                surface.swap.ResizeBuffers(
                    2,
                    w,
                    h,
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
            }
            .map_err(|e| Error::win("ResizeBuffers", e))?;
            surface.size = (w, h);
        }
        if surface.rtv.is_none() {
            // SAFETY: Puffer 0 der eigenen Swapchain.
            unsafe {
                let back: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D = surface
                    .swap
                    .GetBuffer(0)
                    .map_err(|e| Error::win("GetBuffer", e))?;
                let mut rtv = None;
                self.gpu
                    .device
                    .CreateRenderTargetView(&back, None, Some(&mut rtv))
                    .map_err(|e| Error::win("CreateRenderTargetView", e))?;
                surface.rtv = rtv;
            }
        }
        surface
            .rtv
            .clone()
            .ok_or_else(|| Error::Helper("keine RTV".into()))
    }

    fn create_surface(&self, w: u32, h: u32) -> Result<Surface> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w.max(1),
            Height: h.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            ..Default::default()
        };
        let err = |call| move |e| Error::win(call, e);
        // SAFETY: COM-Aufrufe auf Objekten dieses Geräts und dieses Fensters.
        unsafe {
            let dxgi: IDXGIDevice = self.gpu.device.cast().map_err(err("IDXGIDevice"))?;
            let factory: IDXGIFactory2 = dxgi
                .GetAdapter()
                .and_then(|a| a.GetParent())
                .map_err(err("IDXGIFactory2"))?;
            let swap = factory
                .CreateSwapChainForComposition(&self.gpu.device, &desc, None)
                .map_err(err("CreateSwapChainForComposition"))?;
            let dcomp: IDCompositionDevice =
                DCompositionCreateDevice(&dxgi).map_err(err("DCompositionCreateDevice"))?;
            let target = dcomp
                .CreateTargetForHwnd(self.hwnd, true)
                .map_err(err("CreateTargetForHwnd"))?;
            let visual = dcomp.CreateVisual().map_err(err("CreateVisual"))?;
            visual.SetContent(&swap).map_err(err("SetContent"))?;
            target.SetRoot(&visual).map_err(err("SetRoot"))?;
            dcomp.Commit().map_err(err("Commit"))?;
            Ok(Surface {
                swap,
                rtv: None,
                size: (desc.Width, desc.Height),
                _visual: visual,
                _target: target,
                _dcomp: dcomp,
            })
        }
    }
}

impl Drop for GpuFocusFrame {
    fn drop(&mut self) {
        self.surface = None;
        // SAFETY: eigenes Fenster, auf seinem Thread (nicht Send).
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Lage des Rahmens um `target`, oder `None`, wenn es keinen geben soll.
fn layout_of(target: Hwnd) -> Option<FrameLayout> {
    if !is_window(target) || is_minimized(target) {
        return None;
    }
    let window = visible_rect(target)?;
    let monitor = monitor_of_window(target)?.rect;
    FrameLayout::new(window, monitor, dpi::factor_for_window(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gpu_frame_window_cannot_be_created_in_tests() {
        assert!(matches!(GpuFocusFrame::new(), Err(Error::Forbidden(_))));
    }
}
