//! Der Fokus-Rahmen auf der Grafikkarte: Direct3D 11 rechnet jeden Pixel mit
//! `frame.hlsl` (Port von `focus-frame/Renderer.cs` der Python-Fassung).
//!
//! Warum überhaupt die Grafikkarte: Die Python-Fassung hat den Rahmen zuerst auf der
//! CPU gezeichnet; das sah grob aus und kostete bis zu einem Drittel eines Kerns, darum
//! ist sie am 2026-11-06 auf ein GPU-Hilfsprogramm umgestiegen. Ein CPU-Port war auch
//! hier bei voller Auflösung zu teuer für eine Daueranimation (gemessen: 720 000 Pixel
//! je Bild, 10 bis 20 ms auf acht Threads) und ist darum entfallen.
//!
//! Dieses Modul ist nur die Rechenstrecke (Gerät, Shader, Konstantenpuffer); das
//! Fenster dazu steht in [`crate::frame_gpu_overlay`].

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    ID3DBlob,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, D3D11CreateDevice, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext,
    ID3D11PixelShader, ID3D11RenderTargetView, ID3D11VertexShader,
};
use windows::core::{PCSTR, s};

use crate::frame_fx::FrameParams;
use crate::types::{Error, Result};

/// Der Shader: die Stammdesigns (dieselbe Datei wie im Hilfsprogramm der Python-Fassung),
/// dahinter die Winter-Designs und der eigene Glow.
pub const SHADER: &str = concat!(
    include_str!("frame.hlsl"),
    "\n",
    include_str!("frame_winter.hlsl"),
    "\n",
    include_str!("frame_eigen.hlsl")
);

/// Gerät, Shader und Konstantenpuffer.
pub struct GpuPipeline {
    pub(crate) device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cb: ID3D11Buffer,
}

impl GpuPipeline {
    /// Auf der Grafikkarte; ohne passende Karte (etwa per RDP) auf WARP, dem
    /// Software-Rasterizer von Windows.
    pub fn new() -> Result<GpuPipeline> {
        GpuPipeline::with_driver(D3D_DRIVER_TYPE_HARDWARE)
            .or_else(|_| GpuPipeline::with_driver(D3D_DRIVER_TYPE_WARP))
    }

    fn with_driver(driver: D3D_DRIVER_TYPE) -> Result<GpuPipeline> {
        let levels = [
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
        ];
        let (mut device, mut ctx) = (None, None);
        // SAFETY: Ausgabezeiger auf lokale Options; Shader-Bytecode lebt während der Aufrufe.
        unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .map_err(|e| Error::win("D3D11CreateDevice", e))?;
            let device: ID3D11Device =
                device.ok_or_else(|| Error::Helper("kein D3D11-Gerät".into()))?;
            let ctx = ctx.ok_or_else(|| Error::Helper("kein D3D11-Kontext".into()))?;
            let (mut vs, mut ps, mut cb) = (None, None, None);
            device
                .CreateVertexShader(&compile(s!("VSMain"), s!("vs_5_0"))?, None, Some(&mut vs))
                .map_err(|e| Error::win("CreateVertexShader", e))?;
            device
                .CreatePixelShader(&compile(s!("PSMain"), s!("ps_5_0"))?, None, Some(&mut ps))
                .map_err(|e| Error::win("CreatePixelShader", e))?;
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: size_of::<FrameParams>() as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            };
            device
                .CreateBuffer(&desc, None, Some(&mut cb))
                .map_err(|e| Error::win("CreateBuffer", e))?;
            let missing = || Error::Helper("D3D11 lieferte kein Objekt".into());
            Ok(GpuPipeline {
                device,
                ctx,
                vs: vs.ok_or_else(missing)?,
                ps: ps.ok_or_else(missing)?,
                cb: cb.ok_or_else(missing)?,
            })
        }
    }

    /// Ein Bild in `rtv` (w × h Pixel) zeichnen. Präsentieren macht der Aufrufer.
    pub(crate) fn draw(&self, rtv: &ID3D11RenderTargetView, w: u32, h: u32, p: &FrameParams) {
        let vp = D3D11_VIEWPORT {
            Width: w as f32,
            Height: h as f32,
            MaxDepth: 1.0,
            ..Default::default()
        };
        // SAFETY: Alle Objekte gehören diesem Gerät; `p` hat genau die Puffergröße.
        unsafe {
            self.ctx
                .UpdateSubresource(&self.cb, 0, None, (p as *const FrameParams).cast(), 0, 0);
            self.ctx
                .OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            self.ctx.ClearRenderTargetView(rtv, &[0.0; 4]);
            self.ctx.RSSetViewports(Some(&[vp]));
            self.ctx
                .IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.ctx.VSSetShader(&self.vs, None);
            self.ctx.PSSetShader(&self.ps, None);
            self.ctx
                .PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            self.ctx.Draw(3, 0);
        }
    }

    /// Render-Ziel loslassen (vor dem Größenwechsel einer Swapchain nötig).
    pub(crate) fn unbind(&self) {
        // SAFETY: setzt nur den Zustand des eigenen Kontexts zurück.
        unsafe { self.ctx.OMSetRenderTargets(None, None) };
    }
}

/// HLSL zur Laufzeit übersetzen (`d3dcompiler_47.dll` gehört zu Windows 10 und 11).
fn compile(entry: PCSTR, target: PCSTR) -> Result<Vec<u8>> {
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: Quelltext lebt statisch; Blobs werden nur über ihre eigene Größe gelesen.
    unsafe {
        let r = D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            s!("frame.hlsl"),
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        );
        let text = |b: &ID3DBlob| {
            String::from_utf8_lossy(std::slice::from_raw_parts(
                b.GetBufferPointer() as *const u8,
                b.GetBufferSize(),
            ))
            .into_owned()
        };
        if let Err(e) = r {
            let detail = errors.as_ref().map(text).unwrap_or_default();
            return Err(Error::Helper(format!(
                "Shader {}: {} {detail}",
                entry.display(),
                e.message()
            )));
        }
        let code = code.ok_or_else(|| Error::Helper("Shader ohne Bytecode".into()))?;
        Ok(
            std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize())
                .to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

    use super::*;
    use crate::frame_fx::{Design, MARGIN_PT, compute};

    /// Ein Rahmenbild abseits des Bildschirms: Ziel zum Zeichnen, Kopie zum Zurücklesen.
    struct Canvas {
        target: ID3D11Texture2D,
        copy: ID3D11Texture2D,
        rtv: ID3D11RenderTargetView,
        size: (u32, u32),
    }

    fn canvas(gpu: &GpuPipeline, w: u32, h: u32) -> Canvas {
        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let (mut target, mut copy, mut rtv) = (None, None, None);
        // SAFETY: Beschreibungen und Ausgabezeiger leben während der Aufrufe.
        unsafe {
            gpu.device.CreateTexture2D(&desc, None, Some(&mut target)).unwrap();
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            gpu.device.CreateTexture2D(&desc, None, Some(&mut copy)).unwrap();
            let target: ID3D11Texture2D = target.unwrap();
            gpu.device.CreateRenderTargetView(&target, None, Some(&mut rtv)).unwrap();
            Canvas { target, copy: copy.unwrap(), rtv: rtv.unwrap(), size: (w, h) }
        }
    }

    /// Das zuletzt gezeichnete Bild als BGRA, vormultipliziert, zeilenweise von oben.
    fn pixels(gpu: &GpuPipeline, c: &Canvas) -> Vec<u8> {
        let (w, h) = (c.size.0 as usize, c.size.1 as usize);
        let mut out = vec![0u8; w * h * 4];
        // SAFETY: Die Kopie ist zum Lesen angelegt; gelesen wird nur innerhalb der Zeilen.
        unsafe {
            gpu.ctx.CopyResource(&c.copy, &c.target);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.ctx.Map(&c.copy, 0, D3D11_MAP_READ, 0, Some(&mut map)).unwrap();
            for y in 0..h {
                let row = (map.pData as *const u8).add(y * map.RowPitch as usize);
                out[y * w * 4..(y + 1) * w * 4].copy_from_slice(std::slice::from_raw_parts(row, w * 4));
            }
            gpu.ctx.Unmap(&c.copy, 0);
        }
        out
    }

    /// Parameter für ein Fenster von `w` × `h` Pixeln, das mit vollem Hof im Bild liegt.
    fn params(design: Design, t: f64, sc: f64, w: u32, h: u32, night: bool) -> (FrameParams, u32, u32) {
        let m = (MARGIN_PT * sc).ceil() as u32;
        let win = [m as f32, m as f32, (m + w) as f32, (m + h) as f32];
        let p = compute(design, 7, t, sc, win, f64::from(w), f64::from(h)).with_night(night);
        let p = if design == Design::Eigen { p.with_glow(if night { &GLOW_DARK } else { &GLOW_LIGHT }) } else { p };
        (p, w + 2 * m, h + 2 * m)
    }

    /// Die Vorgaben aus `deck_core::glow` (deck-win kennt deck-core nicht): Weichlicht, still,
    /// dunkel Honiggold, hell Terrakotta.
    const GLOW_DARK: [[f32; 4]; 4] = [[0.961, 0.722, 0.278, 1.0], [1.0, 0.0, 0.0, 0.0], [0.941, 0.541, 0.235, 1.0], [0.0; 4]];
    const GLOW_LIGHT: [[f32; 4]; 4] = [[0.851, 0.467, 0.353, 1.2], [1.0, 0.0, 0.0, 0.0], [0.851, 0.467, 0.353, 1.0], [0.0; 4]];

    #[test]
    fn the_shader_compiles_and_every_design_draws_on_the_edge() {
        let gpu = GpuPipeline::new().expect("Shader übersetzt (frame.hlsl + frame_winter.hlsl + frame_eigen.hlsl)");
        for design in Design::ALL.into_iter().chain(Design::WINTER).chain(Design::EIGEN) {
            let (p, w, h) = params(design, 3.0, 1.0, 300, 200, false);
            let c = canvas(&gpu, w, h);
            gpu.draw(&c.rtv, w, h, &p);
            let px = pixels(&gpu, &c);
            let alpha = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
            let m = (w - 300) / 2;
            let on_edge = (m..m + 300).filter(|&x| (m - 8..=m).any(|y| alpha(x, y) > 40)).count();
            assert!(on_edge > 20, "{design:?}: die Oberkante leuchtet ({on_edge} Pixel)");
            assert_eq!(alpha(0, 0), 0, "{design:?}: die Ecke des Bilds liegt außerhalb des Hofs");
            if design.id() < 10 || design.id() >= 20 {
                assert_eq!(alpha(w / 2, h / 2), 0, "{design:?}: tief im Fenster bleibt es durchsichtig");
            }
        }
    }

    /// Eigener Glow mit Glow-Art `kind` und Animation `anim` (Nummern wie in `frame_eigen.hlsl`).
    fn glow(kind: u8, anim: u8, night: bool) -> [[f32; 4]; 4] {
        let mut g = if night { GLOW_DARK } else { GLOW_LIGHT };
        g[1][1] = f32::from(kind);
        g[1][2] = f32::from(anim);
        g
    }

    #[test]
    fn every_glow_kind_with_every_animation_lights_the_bottom_corner_and_nothing_else() {
        let gpu = GpuPipeline::new().unwrap();
        for night in [false, true] {
            for kind in 0..5u8 {
                for anim in 0..6u8 {
                    let (p, w, h) = params(Design::Eigen, 3.0, 1.0, 300, 200, night);
                    let p = p.with_glow(&glow(kind, anim, night));
                    let c = canvas(&gpu, w, h);
                    gpu.draw(&c.rtv, w, h, &p);
                    let px = pixels(&gpu, &c);
                    let at = |x: u32, y: u32, ch: u32| px[((y * w + x) * 4 + ch) as usize];
                    let m = (w - 300) / 2;
                    // Unten rechts leuchtet jede Art: auch Bodenlicht (oben fast nichts) und
                    // Ecklicht (Kantenmitte fast dunkel). Rot ist in beiden Vorgaben stark.
                    let lit = (m + 250..m + 300).filter(|&x| (m + 201..m + 206).any(|y| at(x, y, 2) > 30)).count();
                    assert!(lit > 20, "Art {kind}, Animation {anim}, dunkel {night}: unten rechts leuchtet es ({lit} Pixel)");
                    assert_eq!((at(0, 0, 3), at(0, 0, 2)), (0, 0), "Art {kind}, Animation {anim}: der Hof endet vor dem Bildrand");
                    assert_eq!((at(w / 2, h / 2, 3), at(w / 2, h / 2, 2)), (0, 0), "Art {kind}, Animation {anim}: im Fenster bleibt es leer");
                }
            }
        }
    }

    /// Standbilder aller Glow-Arten (dunkel, Stärke 1 und 2), je mit einer anderen Animation:
    /// `DECK_FRAME_STILLS=<ordner> cargo test -p deck-win glow_stills -- --ignored`
    #[test]
    #[ignore = "schreibt Bilder; nur zum Ansehen"]
    fn glow_stills() {
        let dir = std::path::PathBuf::from(std::env::var("DECK_FRAME_STILLS").expect("DECK_FRAME_STILLS=<ordner>"));
        let gpu = GpuPipeline::new().unwrap();
        for kind in 0..5u8 {
            for strength in [1.0f32, 2.0] {
                let (p, w, h) = params(Design::Eigen, 3.0, 1.0, 400, 300, true);
                let mut g = glow(kind, kind + 1, true);
                g[0][3] = strength;
                let p = p.with_glow(&g);
                let c = canvas(&gpu, w, h);
                gpu.draw(&c.rtv, w, h, &p);
                let px = pixels(&gpu, &c);
                let m = (w - 400) / 2;
                let mut bmp = bmp_header(w, h);
                for y in (0..h).rev() {
                    for x in 0..w {
                        let i = ((y * w + x) * 4) as usize;
                        let inside = x >= m && x < w - m && y >= m && y < h - m;
                        let under: [u8; 3] = if inside { [0x1B, 0x1C, 0x1D] } else { [0x26, 0x21, 0x1E] };
                        let a = u32::from(px[i + 3]);
                        for ch in 0..3 {
                            bmp.push((u32::from(px[i + ch]) + u32::from(under[ch]) * (255 - a) / 255).min(255) as u8);
                        }
                    }
                    bmp.resize(bmp.len() + (4 - (w as usize * 3) % 4) % 4, 0);
                }
                std::fs::write(dir.join(format!("glow-art{kind}-anim{}-staerke{strength}.bmp", kind + 1)), bmp).unwrap();
            }
        }
    }

    /// Standbilder der Winter-Designs und des eigenen Glows zum Ansehen, über einem gemalten Terminal (hell und
    /// dunkel), dazu die Rechenzeit je Bild bei 1920 × 1080:
    /// `DECK_FRAME_STILLS=<ordner> cargo test -p deck-win stills -- --ignored --nocapture`
    #[test]
    #[ignore = "schreibt Bilder; nur zum Ansehen"]
    fn stills() {
        let dir = std::path::PathBuf::from(std::env::var("DECK_FRAME_STILLS").expect("DECK_FRAME_STILLS=<ordner>"));
        let gpu = GpuPipeline::new().unwrap();
        for design in Design::WINTER.into_iter().chain(Design::EIGEN) {
            for (night, paper, name) in [(false, [0xEDu8, 0xF4, 0xFA], "hell"), (true, [0x1B, 0x1C, 0x1D], "dunkel")] {
                for t in [0.5, 12.0, 40.0] {
                    let (p, w, h) = params(design, t, 1.25, 850, 600, night);
                    let c = canvas(&gpu, w, h);
                    gpu.draw(&c.rtv, w, h, &p);
                    let px = pixels(&gpu, &c);
                    let m = (w - 850) / 2;
                    let mut bmp = bmp_header(w, h);
                    for y in (0..h).rev() {
                        for x in 0..w {
                            let i = ((y * w + x) * 4) as usize;
                            let inside = x >= m && x < w - m && y >= m && y < h - m;
                            // Hell auf hellem Desktop, dunkel auf dunklem, wie sie gedacht sind.
                            let desk = if night { [0x26, 0x21, 0x1E] } else { [0xEB, 0xE6, 0xE3] };
                            let under: [u8; 3] = if inside { paper } else { desk };
                            let a = u32::from(px[i + 3]);
                            for ch in 0..3 {
                                bmp.push((u32::from(px[i + ch]) + u32::from(under[ch]) * (255 - a) / 255).min(255) as u8);
                            }
                        }
                        bmp.resize(bmp.len() + (4 - (w as usize * 3) % 4) % 4, 0);
                    }
                    std::fs::write(dir.join(format!("{design:?}-{name}-{t}.bmp")), bmp).unwrap();
                }
            }
            let (p, w, h) = params(design, 20.0, 1.0, 1920, 1080, false);
            let c = canvas(&gpu, w, h);
            let start = Instant::now();
            for _ in 0..60 {
                gpu.draw(&c.rtv, w, h, &p);
            }
            pixels(&gpu, &c);
            println!("{design:?}: {:.2} ms je Bild bei 1920 × 1080", start.elapsed().as_secs_f64() * 1000.0 / 60.0);
        }
    }

    /// Kopf einer 24-Bit-BMP (Zeilen von unten nach oben, je auf 4 Byte aufgefüllt).
    fn bmp_header(w: u32, h: u32) -> Vec<u8> {
        let row = (w * 3).div_ceil(4) * 4;
        let mut b = Vec::with_capacity((54 + row * h) as usize);
        b.extend_from_slice(b"BM");
        for v in [54 + row * h, 0, 54, 40, w, h, 1 | (24 << 16), 0, row * h, 2835, 2835, 0, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }
}
