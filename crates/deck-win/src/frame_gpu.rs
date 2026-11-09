//! Der Fokus-Rahmen auf der Grafikkarte: Direct3D 11 rechnet jeden Pixel mit
//! `frame.hlsl` (Port von `focus-frame/Renderer.cs` der Python-Fassung).
//!
//! Warum überhaupt die Grafikkarte: Die Python-Fassung hat den Rahmen zuerst auf der
//! CPU gezeichnet; das sah grob aus und kostete bis zu einem Drittel eines Kerns, darum
//! ist sie am 2026-11-06 auf ein GPU-Hilfsprogramm umgestiegen. Der CPU-Port hier
//! ([`crate::frame_render`]) ist bei voller Auflösung ebenfalls zu teuer für eine
//! Daueranimation (gemessen: 720 000 Pixel je Bild, 10 bis 20 ms auf acht Threads).
//! Er bleibt als Prüfstein für diesen Weg (Test unten) und als Rückfall ohne GPU.
//!
//! Dieses Modul ist nur die Rechenstrecke (Gerät, Shader, Konstantenpuffer); das
//! Fenster dazu steht in [`crate::frame_gpu_overlay`]. [`GpuPipeline::render_to_memory`]
//! zeichnet ohne Fenster in den Speicher, für Tests und Vergleiche.

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    ID3DBlob,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT,
    D3D11CreateDevice, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::core::{PCSTR, s};

use crate::frame_fx::FrameParams;
use crate::types::{Error, Result};

/// Der Shader, dieselbe Datei wie im Hilfsprogramm der Python-Fassung.
pub const SHADER: &str = include_str!("frame.hlsl");

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

    /// Ausdrücklich WARP (Software): gleich auf jedem Rechner, darum für Tests.
    pub fn warp() -> Result<GpuPipeline> {
        GpuPipeline::with_driver(D3D_DRIVER_TYPE_WARP)
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

    /// Ein Bild ohne Fenster rechnen und als vormultipliziertes BGRA (w × h × 4 Bytes,
    /// Zeile für Zeile von oben) zurückholen.
    pub fn render_to_memory(&self, p: &FrameParams, w: u32, h: u32) -> Result<Vec<u8>> {
        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        // SAFETY: Texturen dieses Geräts; gemappter Speicher wird nur innerhalb von
        // Map/Unmap gelesen, Zeilenabstand aus RowPitch.
        unsafe {
            let mut target: Option<ID3D11Texture2D> = None;
            self.device
                .CreateTexture2D(&desc, None, Some(&mut target))
                .map_err(|e| Error::win("CreateTexture2D", e))?;
            let target = target.ok_or_else(|| Error::Helper("keine Textur".into()))?;
            let mut rtv = None;
            self.device
                .CreateRenderTargetView(&target, None, Some(&mut rtv))
                .map_err(|e| Error::win("CreateRenderTargetView", e))?;
            let rtv = rtv.ok_or_else(|| Error::Helper("keine RTV".into()))?;
            self.draw(&rtv, w, h, p);
            self.unbind();
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            let mut staging: Option<ID3D11Texture2D> = None;
            self.device
                .CreateTexture2D(&desc, None, Some(&mut staging))
                .map_err(|e| Error::win("CreateTexture2D", e))?;
            let staging = staging.ok_or_else(|| Error::Helper("keine Textur".into()))?;
            self.ctx.CopyResource(&staging, &target);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| Error::win("Map", e))?;
            let row = (w * 4) as usize;
            let mut out = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                let src = (mapped.pData as *const u8).add(y * mapped.RowPitch as usize);
                out.extend_from_slice(std::slice::from_raw_parts(src, row));
            }
            self.ctx.Unmap(&staging, 0);
            Ok(out)
        }
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
    use super::*;
    use crate::frame_fx::{Design, compute};
    use crate::frame_render::render;
    use crate::types::Rect;

    #[test]
    fn gpu_and_cpu_agree() {
        // Hält den CPU-Port und den Shader beisammen: dasselbe Bild auf WARP und auf
        // der CPU, je Kanal höchstens das Rauschen auseinander (gemessen: 1 Stufe).
        // Hochspannung ohne Lichtbögen: deren Zittern kommt aus sin() großer Zahlen,
        // und das rechnet jede GPU anders (es ist Zufall, kein Bild).
        let gpu = GpuPipeline::warp().expect("WARP gibt es auf jedem Windows");
        let (w, h) = (360u32, 260u32);
        let win = [40.0, 40.0, 320.0, 220.0];
        for d in Design::ALL {
            for t in [0.0, 0.37, 1.9] {
                let mut p = compute(d, 9, t, 1.25, win, 280.0, 180.0);
                if d == Design::Hochspannung {
                    p.b[1] = 0.0;
                }
                let g = gpu.render_to_memory(&p, w, h).unwrap();
                let mut c = vec![0u8; g.len()];
                render(&p, Rect::new(0, 0, w as i32, h as i32), &mut c);
                let worst = g.iter().zip(&c).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                assert!(worst <= 2, "{d:?} t={t}: größte Abweichung {worst} Stufen");
                assert!(
                    g.iter().skip(3).step_by(4).any(|&a| a > 200),
                    "{d:?}: GPU-Bild leer"
                );
            }
        }
    }
}
