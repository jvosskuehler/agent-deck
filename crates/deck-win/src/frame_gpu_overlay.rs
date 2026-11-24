//! Der Fokus-Rahmen über DirectComposition: EIN durchklickbares Fenster, die Bühne, liegt
//! über dem ganzen Monitor des Agentenfensters (ohne Taskleiste). Die Grafikkarte
//! zeichnet nur den Rand samt Hof, der Rest bleibt durchsichtig (Port von `App` in
//! `focus-frame/Program.cs` und der Swapchain aus `Renderer.cs` der Python-Fassung).
//!
//! Jeder Pixel entsteht auf der GPU im Takt des Monitors (`Present1(1)` wartet auf den
//! Bildwechsel und ist zugleich der Takt der Animation), die CPU sagt nur, was.
//!
//! Warum eine Bühne über dem ganzen Monitor (15.11.2026): Vorher war das Fenster nur so
//! groß wie Rahmen samt Hof und zog bei jedem Fokuswechsel um. Fensterlage und Bildinhalt
//! erreichen DWM aber auf getrennten Wegen und nicht sicher im selben Bild: für ein, zwei
//! Bilder stand der alte Glow am neuen Platz, gestreckt auf die neue Größe, und der Glow
//! zuckte. Jetzt bewegt sich nur der Inhalt, jeder Wechsel ist ein einziges Bild. Umziehen
//! muss die Bühne nur, wenn der Fokus auf einen anderen Monitor wechselt; das geschieht
//! versteckt.
//!
//! Beim Fokuswechsel wandert der Glow nicht hinüber: er blendet am alten Fenster aus
//! ([`FADE_OUT`]) und danach am neuen ein ([`FADE_IN`]). Ein Hinübergleiten (15.11.2026
//! kurz drin) sah bei jedem Ruckler hakelig aus, weil man jeden fehlenden Zwischenschritt
//! als Sprung im Ort sieht; ein fehlendes Bild beim Blenden fällt nicht auf.
//!
//! Damit DWM trotz der großen Fläche nicht den ganzen Monitor neu mischt, nennt jedes
//! Bild nur seine geänderte Fläche (`Present1` mit Dirty-Rects): den Rahmen von eben und
//! den von jetzt. Der Shader läuft nur im Rahmen (Viewport), der Rest wird nur geleert.
//!
//! Das Fenster trägt `WS_EX_NOREDIRECTIONBITMAP`: es hat keinen eigenen Bildpuffer, sein
//! Inhalt ist allein der DirectComposition-Visual. Ohne `SetLayeredWindowAttributes`
//! zeigt Windows ein Layered-Fenster gar nicht an.
//!
//! Es gehört dem Thread, der es anlegt (nicht `Send`); dessen Schleife ruft
//! [`crate::frame_window::pump_messages`] und dann [`GpuFocusFrame::draw`].

use std::marker::PhantomData;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Direct3D11::ID3D11RenderTargetView;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIDevice1, IDXGIFactory2,
    IDXGISwapChain1,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, HWND_TOPMOST, LWA_ALPHA, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, WS_EX_NOREDIRECTIONBITMAP,
};
use windows::core::{Interface, w};

use crate::arrange::visible_rect;
use crate::dpi;
use crate::frame_fx::Design;
use crate::frame_gpu::GpuPipeline;
use crate::frame_layout::FrameLayout;
use crate::frame_still::{Still, needs_paint};
use crate::guard;
use crate::monitor::monitor_of_window;
use crate::frame_window::{clip_to_work_areas, create_click_through};
use crate::types::{Error, Hwnd, Rect, Result};
use crate::window::{is_minimized, is_window};

/// So lange blendet der Glow am Fenster aus, das den Fokus abgegeben hat.
pub const FADE_OUT: Duration = Duration::from_millis(160);
/// So lange blendet er danach am neuen Fenster ein.
pub const FADE_IN: Duration = Duration::from_millis(240);
/// Längster Zeitschritt je Bild (Sekunden). Hängt das Deck kurz, läuft die Blende danach
/// weiter, statt das Stück zu überspringen.
const MAX_STEP: f64 = 0.05;
/// Ohne Rahmen bleibt die (leere) Bühne so lange stehen: wer kurz in ein anderes
/// Programm klickt und zurückkommt, sieht den Glow sofort wieder, ohne Umzug.
const LINGER: Duration = Duration::from_secs(3);

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
    /// Monitor, über dem die Bühne gerade liegt.
    stage: Option<Rect>,
    /// Die Swapchain ist frisch (angelegt oder in der Größe geändert): das nächste Bild
    /// wird ganz präsentiert, ohne Dirty-Rects.
    fresh: bool,
    /// Das Fenster, das gerade leuchtet (oder noch ausblendet).
    lit: Option<Lit>,
    /// Wie zuletzt gezeichnet wurde; das Ausblenden ohne neues Ziel zeichnet so weiter.
    look: Option<Look>,
    /// Zeitpunkt des letzten Bilds, für den Fortschritt der Blende.
    last_step: Option<Instant>,
    /// Rahmenfläche des letzten Bilds, relativ zur Bühne (für die Dirty-Rects).
    last_area: Option<Rect>,
    /// Rahmenflächen der beiden letzten Bilder: der Puffer, in den das nächste Bild geht,
    /// trägt eines davon, nur diese Flächen müssen geleert werden.
    stale: [Option<Rect>; 2],
    /// Seit wann es keinen Rahmen gibt.
    idle_since: Option<Instant>,
    /// Was das zuletzt präsentierte Bild zeigt ([`crate::frame_still`]).
    painted: Option<Still>,
    /// Das letzte [`GpuFocusFrame::draw`] hat nichts Neues gezeigt (stiller Glow).
    skipped: bool,
    _not_send: PhantomData<*const ()>,
}

/// Ein leuchtendes Fenster: wo, in welcher Skalierung, wie weit eingeblendet (0..1).
#[derive(Clone, Copy, Debug)]
struct Lit {
    hwnd: Hwnd,
    window: Rect,
    scale: f64,
    level: f64,
}

/// Was der Aufrufer zuletzt zeichnen ließ.
#[derive(Clone, Copy)]
struct Look {
    design: Design,
    seed: i32,
    t: f64,
    night: bool,
    glow: [[f32; 4]; 4],
}

/// Deckkraft aus dem Fortschritt der Blende: sanft los, sanft an (smoothstep).
pub fn fade_curve(level: f64) -> f64 {
    let k = level.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// Nächster Stand der Blende. `stays`: das Fenster hat weiter den Fokus und blendet ein,
/// sonst blendet es aus. `dt` in Sekunden.
fn next_level(level: f64, stays: bool, dt: f64) -> f64 {
    if stays {
        (level + dt / FADE_IN.as_secs_f64()).min(1.0)
    } else {
        level - dt / FADE_OUT.as_secs_f64()
    }
}

/// Kleinstes Rechteck um beide.
fn hull(a: Rect, b: Option<Rect>) -> Rect {
    match b {
        Some(b) => Rect::new(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom)),
        None => a,
    }
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
            stage: None,
            fresh: true,
            lit: None,
            look: None,
            last_step: None,
            last_area: None,
            stale: [None; 2],
            idle_since: None,
            painted: None,
            skipped: false,
            _not_send: PhantomData,
        })
    }

    /// Ein Bild um `target` zeichnen und zeigen; wartet auf den Bildwechsel des
    /// Monitors. `night`: das Terminal ist dunkel (Farbe der Schneeflocken im Winter).
    /// `Ok(false)`: nichts zu zeigen (Fenster weg, minimiert, außerhalb), der Rahmen ist
    /// dann weg.
    /// `glow` sind die Werte des eigenen Glows (nur für [`Design::Eigen`] gelesen).
    pub fn draw(&mut self, target: Hwnd, design: Design, seed: i32, t: f64, night: bool, glow: &[[f32; 4]; 4]) -> Result<bool> {
        guard::check("frame_gpu_overlay")?;
        self.skipped = false;
        let dt = self.step();
        self.look = Some(Look { design, seed, t, night, glow: *glow });
        self.advance(Some(target), dt)
    }

    /// Rahmen wegnehmen: ein noch leuchtender Glow blendet aus, danach bleibt die Bühne
    /// noch [`LINGER`] stehen (leer) und wird dann versteckt. Der Aufrufer ruft es in
    /// jedem Takt. `true`, solange noch ausgeblendet wird (dann hat es schon auf den
    /// Bildwechsel gewartet, der Aufrufer muss nicht schlafen).
    pub fn hide(&mut self) -> bool {
        let dt = self.step();
        // Die Animation läuft beim Ausblenden weiter, sie steht nicht still.
        if let Some(look) = self.look.as_mut() {
            look.t += dt;
        }
        self.advance(None, dt).unwrap_or(false)
    }

    /// Hat das letzte [`Self::draw`] das stehende Bild gelassen (nichts gezeichnet, nicht
    /// auf den Bildwechsel gewartet)? Dann wartet der Aufrufer selbst ein Bild lang.
    pub fn skipped(&self) -> bool {
        self.skipped
    }

    /// Sekunden seit dem letzten Bild, gedeckelt auf [`MAX_STEP`].
    fn step(&mut self) -> f64 {
        let now = Instant::now();
        let dt = self.last_step.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        self.last_step = Some(now);
        dt.min(MAX_STEP)
    }

    /// Ein Bild weiter. `target`: wer jetzt den Fokus-Rahmen haben soll. Hat noch ein
    /// anderes Fenster Glow, blendet erst das aus; erst danach blendet `target` ein.
    /// `Ok(true)`, solange etwas zu sehen ist.
    fn advance(&mut self, target: Option<Hwnd>, dt: f64) -> Result<bool> {
        let place = target.and_then(place_of);
        if let Some(l) = self.lit.as_mut() {
            // Bleibt nur, wer weiter vorn ist und auf derselben Bühne liegt; sonst aus.
            let here = place.filter(|&(_, m, _)| Some(l.hwnd) == target && Some(m) == self.stage);
            if let Some((window, _, scale)) = here {
                l.window = window;
                l.scale = scale;
            }
            l.level = next_level(l.level, here.is_some(), dt);
            if l.level <= 0.0 {
                self.lit = None;
            }
        }
        let (mut first, mut started) = (false, false);
        if self.lit.is_none() {
            let (Some(hwnd), Some((window, monitor, scale))) = (target, place) else {
                self.rest();
                return Ok(false);
            };
            if self.stage != Some(monitor) {
                self.move_stage(monitor)?;
                first = true;
            }
            self.lit = Some(Lit { hwnd, window, scale, level: 0.0 });
            started = true;
        }
        self.idle_since = None;
        let lit = self.lit.expect("eben gesetzt");
        if !self.paint(lit)? {
            self.lit = None;
            self.rest();
            return Ok(false);
        }
        if first {
            // Erst zeigen, wenn das erste Bild bei DWM angekommen ist: sonst stünde für
            // einen Augenblick der Inhalt vom alten Monitor da.
            // SAFETY: wartet auf das nächste Bild von DWM.
            let _ = unsafe { DwmFlush() };
            self.show_stage(true)?;
        } else if started {
            // Ein anderes Fenster ist vorn: die Bühne wieder ganz nach oben.
            self.show_stage(false)?;
        }
        Ok(true)
    }

    /// Den Glow um `lit` zeichnen und zeigen, so weit eingeblendet wie `lit.level`.
    /// `Ok(false)`: kein Rahmen möglich (keine Bühne, Fenster außerhalb).
    fn paint(&mut self, lit: Lit) -> Result<bool> {
        let (Some(stage), Some(look)) = (self.stage, self.look) else {
            return Ok(false);
        };
        let Some(layout) = FrameLayout::new(lit.window, stage, lit.scale) else {
            return Ok(false);
        };
        let origin = (stage.left, stage.top);
        let area = layout.frame.offset(-origin.0, -origin.1);
        let rtv = self.target_view(stage.width() as u32, stage.height() as u32)?;
        let key = Still { design: look.design, window: lit.window, stage, scale: lit.scale, night: look.night, glow: look.glow };
        if !self.fresh && !needs_paint(self.painted.as_ref(), &key, lit.level) {
            self.skipped = true;
            return Ok(true);
        }
        let params = layout
            .params_from(origin, look.design, look.seed, look.t)
            .with_night(look.night)
            .with_fade(fade_curve(lit.level));
        let params = if look.design == Design::Eigen { params.with_glow(&look.glow) } else { params };
        let clear = self.clear_rects();
        self.gpu.draw_at(&rtv, (area.left, area.top, area.width() as u32, area.height() as u32), clear.as_deref(), &params);
        self.present(Some(area))?;
        self.painted = Some(key);
        Ok(true)
    }

    /// Nichts mehr zu zeigen: einmal leeren, nach [`LINGER`] die Bühne verstecken.
    fn rest(&mut self) {
        if self.last_area.is_some() {
            let _ = self.present_blank();
        }
        let since = *self.idle_since.get_or_insert_with(Instant::now);
        if self.stage.is_some() && since.elapsed() >= LINGER {
            self.stage = None;
            // SAFETY: eigenes Fenster.
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    /// Ein leeres Bild: was eben noch zu sehen war, verschwindet.
    fn present_blank(&mut self) -> Result<()> {
        self.painted = None;
        let Some(size) = self.surface.as_ref().map(|s| s.size) else {
            self.last_area = None;
            return Ok(());
        };
        let rtv = self.target_view(size.0, size.1)?;
        let clear = self.clear_rects();
        self.gpu.draw_at(&rtv, (0, 0, 0, 0), clear.as_deref(), &Default::default());
        self.present(None)
    }

    /// Das gezeichnete Bild zeigen. Geändert ist nur die Fläche des letzten und dieses
    /// Rahmens (`area`, relativ zur Bühne; `None` = leeres Bild).
    fn present(&mut self, area: Option<Rect>) -> Result<()> {
        let surface = self.surface.as_ref().expect("Render-Ziel angelegt");
        let dirty = match (area, self.last_area) {
            (Some(a), last) => Some(hull(a, last)),
            (None, last) => last,
        };
        let full = Rect::new(0, 0, surface.size.0 as i32, surface.size.1 as i32);
        let mut rects: Vec<RECT> = dirty
            .and_then(|d| d.intersect(&full))
            .filter(|_| !self.fresh)
            .map(|r| RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
            .into_iter()
            .collect();
        let params = DXGI_PRESENT_PARAMETERS {
            DirtyRectsCount: rects.len() as u32,
            pDirtyRects: if rects.is_empty() { std::ptr::null_mut() } else { rects.as_mut_ptr() },
            ..Default::default()
        };
        // SAFETY: eigene Swapchain; `rects` lebt über den Aufruf. Present1(1) wartet auf
        // den nächsten Bildwechsel.
        unsafe { surface.swap.Present1(1, DXGI_PRESENT(0), &params) }
            .ok()
            .map_err(|e| Error::win("Present1", e))?;
        self.fresh = false;
        self.last_area = area;
        self.stale = [area, self.stale[0]];
        Ok(())
    }

    /// Was vor dem nächsten Bild zu leeren ist: `None` = alles (frischer Puffer).
    fn clear_rects(&self) -> Option<Vec<RECT>> {
        if self.fresh {
            return None;
        }
        Some(self.stale.iter().flatten().map(|r| RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom }).collect())
    }

    /// Die Bühne auf den Monitor `monitor` setzen: versteckt umziehen, Taskleiste
    /// ausnehmen. Gezeigt wird sie erst nach dem ersten Bild.
    fn move_stage(&mut self, monitor: Rect) -> Result<()> {
        // SAFETY: eigenes Fenster.
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        self.stage = None;
        self.lit = None;
        self.last_area = None;
        self.stale = [None; 2];
        self.fresh = true;
        self.painted = None;
        // SAFETY: eigenes Fenster; zieht es nur um, zeigt es nicht.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                monitor.left,
                monitor.top,
                monitor.width(),
                monitor.height(),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        }
        .map_err(|e| Error::win("SetWindowPos", e))?;
        clip_to_work_areas(self.hwnd, monitor);
        self.stage = Some(monitor);
        Ok(())
    }

    /// Bühne zeigen (`first`) bzw. nur wieder ganz nach oben legen.
    fn show_stage(&self, first: bool) -> Result<()> {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        let flags = if first { flags | SWP_SHOWWINDOW } else { flags };
        // SAFETY: eigenes Fenster; zeigt es, ohne es zu aktivieren.
        unsafe { SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) }
            .map_err(|e| Error::win("SetWindowPos", e))
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
            self.fresh = true;
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
            // Höchstens ein Bild in der Schlange: was das Deck eben entschieden hat, ist
            // im nächsten Bild zu sehen, nicht erst drei später.
            if let Ok(d1) = dxgi.cast::<IDXGIDevice1>() {
                let _ = d1.SetMaximumFrameLatency(1);
            }
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

/// Sichtbarer Rand von `target`, sein ganzer Monitor und dessen Skalierung, oder `None`,
/// wenn es keinen Rahmen geben soll.
fn place_of(target: Hwnd) -> Option<(Rect, Rect, f64)> {
    if !is_window(target) || is_minimized(target) {
        return None;
    }
    let window = visible_rect(target)?;
    let monitor = monitor_of_window(target)?.rect;
    Some((window, monitor, dpi::factor_for_window(target)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fade_starts_and_lands_softly_and_never_overshoots() {
        assert_eq!(fade_curve(0.0), 0.0);
        assert_eq!(fade_curve(1.0), 1.0);
        assert_eq!(fade_curve(-0.3), 0.0);
        assert_eq!(fade_curve(1.4), 1.0);
        assert!(fade_curve(0.1) < 0.1, "sanft los");
        assert!(fade_curve(0.9) > 0.9, "sanft an");
    }

    #[test]
    fn the_glow_fades_in_and_out_within_its_durations() {
        let frame = 1.0 / 60.0;
        let frames = |d: Duration| (d.as_secs_f64() / frame).ceil() as usize;
        let mut level = 0.0;
        for _ in 0..frames(FADE_IN) {
            level = next_level(level, true, frame);
        }
        assert_eq!(level, 1.0, "nach FADE_IN voll da und nicht drüber");
        for _ in 0..frames(FADE_OUT) {
            level = next_level(level, false, frame);
        }
        assert!(level <= 0.0, "nach FADE_OUT weg");
    }

    #[test]
    fn a_hang_does_not_skip_the_fade() {
        // Ein Bild nach einer halben Sekunde Hänger zählt nur MAX_STEP: der Glow ist
        // danach nicht schlagartig weg, er blendet weiter aus.
        let level = next_level(1.0, false, MAX_STEP);
        assert!(level > 0.0);
    }

    #[test]
    fn the_gpu_frame_window_cannot_be_created_in_tests() {
        assert!(matches!(GpuFocusFrame::new(), Err(Error::Forbidden(_))));
    }
}
