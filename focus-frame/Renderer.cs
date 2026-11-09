using System.Reflection;
using Vortice.D3DCompiler;
using Vortice.Direct3D;
using Vortice.Direct3D11;
using Vortice.DirectComposition;
using Vortice.DXGI;
using Vortice.Mathematics;

namespace FocusFrame;

/// <summary>Grafikkarte: Direct3D 11 zeichnet in eine Swapchain, DirectComposition legt
/// sie mit echtem Alpha je Pixel auf das Fenster. Das ist derselbe Weg, den Browser und
/// Windows-Oberflaechen gehen: die Grafikkarte rechnet, die CPU sagt nur, was.</summary>
internal sealed class Renderer : IDisposable
{
    readonly ID3D11Device _device;
    readonly ID3D11DeviceContext _ctx;
    readonly IDXGISwapChain1 _swap;
    readonly IDCompositionDevice _dcomp;
    readonly IDCompositionTarget _target;
    readonly IDCompositionVisual _visual;
    readonly ID3D11VertexShader _vs;
    readonly ID3D11PixelShader _ps;
    readonly ID3D11Buffer _cb;
    ID3D11RenderTargetView? _rtv;
    public int Width { get; private set; }
    public int Height { get; private set; }

    public Renderer(IntPtr hwnd, int width, int height)
    {
        D3D11.D3D11CreateDevice(null, DriverType.Hardware, DeviceCreationFlags.BgraSupport,
            new[] { FeatureLevel.Level_11_0, FeatureLevel.Level_10_1, FeatureLevel.Level_10_0 },
            out _device, out _ctx).CheckError();
        using var dxgiDevice = _device.QueryInterface<IDXGIDevice>();
        using var adapter = dxgiDevice.GetAdapter();
        using var factory = adapter.GetParent<IDXGIFactory2>();
        Width = Math.Max(1, width);
        Height = Math.Max(1, height);
        var desc = new SwapChainDescription1
        {
            Width = (uint)Width,
            Height = (uint)Height,
            Format = Format.B8G8R8A8_UNorm,
            SampleDescription = new SampleDescription(1, 0),
            BufferUsage = Usage.RenderTargetOutput,
            BufferCount = 2,
            Scaling = Scaling.Stretch,
            SwapEffect = SwapEffect.FlipSequential,
            AlphaMode = AlphaMode.Premultiplied,
        };
        _swap = factory.CreateSwapChainForComposition(_device, desc);
        _dcomp = DComp.DCompositionCreateDevice<IDCompositionDevice>(dxgiDevice);
        _dcomp.CreateTargetForHwnd(hwnd, true, out _target).CheckError();
        _dcomp.CreateVisual(out _visual).CheckError();
        _visual.SetContent(_swap).CheckError();
        _target.SetRoot(_visual).CheckError();
        _dcomp.Commit().CheckError();

        string src = ReadShader();
        _vs = _device.CreateVertexShader(Compiler.Compile(src, "VSMain", "Frame.hlsl", "vs_5_0").Span);
        _ps = _device.CreatePixelShader(Compiler.Compile(src, "PSMain", "Frame.hlsl", "ps_5_0").Span);
        _cb = _device.CreateBuffer(new BufferDescription(
            (uint)System.Runtime.CompilerServices.Unsafe.SizeOf<Params>(),
            BindFlags.ConstantBuffer, ResourceUsage.Default));
        CreateTarget();
    }

    static string ReadShader()
    {
        using var s = Assembly.GetExecutingAssembly().GetManifestResourceStream("Frame.hlsl")
                      ?? throw new InvalidOperationException("Frame.hlsl fehlt im Programm");
        using var r = new StreamReader(s);
        return r.ReadToEnd();
    }

    void CreateTarget()
    {
        using var back = _swap.GetBuffer<ID3D11Texture2D>(0);
        _rtv = _device.CreateRenderTargetView(back);
    }

    /// <summary>Puffer an eine neue Fenstergroesse anpassen.</summary>
    public void Resize(int width, int height)
    {
        width = Math.Max(1, width);
        height = Math.Max(1, height);
        if (width == Width && height == Height)
            return;
        _ctx.OMSetRenderTargets((ID3D11RenderTargetView)null!);
        _rtv?.Dispose();
        _rtv = null;
        _swap.ResizeBuffers(2, (uint)width, (uint)height, Format.B8G8R8A8_UNorm, SwapChainFlags.None).CheckError();
        Width = width;
        Height = height;
        CreateTarget();
    }

    /// <summary>Ein Bild zeichnen und zeigen. Present(1) wartet auf den naechsten
    /// Bildwechsel des Monitors: das ist zugleich der Takt der Animation.</summary>
    public void Render(in Params p)
    {
        _ctx.UpdateSubresource(in p, _cb);
        _ctx.OMSetRenderTargets(_rtv!);
        _ctx.ClearRenderTargetView(_rtv!, new Color4(0, 0, 0, 0));
        _ctx.RSSetViewport(0, 0, Width, Height);
        _ctx.IASetPrimitiveTopology(PrimitiveTopology.TriangleList);
        _ctx.VSSetShader(_vs);
        _ctx.PSSetShader(_ps);
        _ctx.PSSetConstantBuffer(0, _cb);
        _ctx.Draw(3, 0);
        _swap.Present(1, PresentFlags.None);
    }

    public void Dispose()
    {
        _rtv?.Dispose();
        _cb.Dispose(); _ps.Dispose(); _vs.Dispose();
        _visual.Dispose(); _target.Dispose(); _dcomp.Dispose();
        _swap.Dispose(); _ctx.Dispose(); _device.Dispose();
    }
}
