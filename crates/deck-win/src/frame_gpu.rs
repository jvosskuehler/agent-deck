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
