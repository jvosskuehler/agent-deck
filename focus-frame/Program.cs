using System.Collections.Concurrent;
using System.Diagnostics;
using System.Numerics;
using System.Text;
using System.Text.Json;

namespace FocusFrame;

/// <summary>
/// Der Fokus-Rahmen des Agent Deck. Das Deck (deck/ops/frame_helper.py) startet dieses
/// Programm und schickt je Zeile ein JSON-Kommando ueber stdin:
///
///   {"cmd": "show", "hwnd": 1234, "design": "kometen", "seed": 42}
///   {"cmd": "hide"}
///   {"cmd": "quit"}             (oder einfach stdin schliessen)
///
/// Welches Fenster vorn ist und welches Design dran ist, entscheidet das Deck. Hier
/// geschieht nur das Zeichnen: der Rahmen folgt dem Fenster selbst (jedes Bild seine
/// Lage neu), damit er beim Ziehen nicht hinterherhinkt.
///
/// Das Fenster ist durchklickbar (WS_EX_LAYERED + WS_EX_TRANSPARENT), wird nie aktiv und
/// steht nicht in der Taskleiste. Es deckt das Agentenfenster samt Hof ab, gezeichnet
/// wird aber nur der Rand; innen bleibt es durchsichtig.
/// </summary>
internal static class Program
{
    static int Main()
    {
        Native.SetProcessDpiAwarenessContext(new IntPtr(-4));      // Per-Monitor-V2
        try
        {
            return new App().Run();
        }
        catch (Exception e)
        {
            Log.Write("Abbruch: " + e);
            return 1;
        }
    }
}

internal static class Log
{
    static readonly string PathName = System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "claude-agent-deck", "focus-frame.log");

    public static void Write(string text)
    {
        try
        {
            Directory.CreateDirectory(System.IO.Path.GetDirectoryName(PathName)!);
            File.AppendAllText(PathName, $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss}] {text}{Environment.NewLine}");
        }
        catch
        {
            // Ein Log, das nicht schreiben kann, darf den Rahmen nicht umbringen.
        }
    }
}

internal sealed class App
{
    const string ClassName = "AgentDeckFocusFrameGpu";

    readonly ConcurrentQueue<string> _commands = new();
    readonly AutoResetEvent _wake = new(false);
    readonly Stopwatch _clock = Stopwatch.StartNew();
    readonly Native.WndProc _proc = Native.DefWindowProcW;   // muss leben, solange das Fenster lebt
    IntPtr _hwnd;
    Renderer? _gpu;
    bool _running = true, _shown, _onScreen;
    IntPtr _target;
    int _design, _seed;
    double _t0;
    (int x, int y, int w, int h) _placed;

    public int Run()
    {
        CreateWindow();
        new Thread(ReadStdin) { IsBackground = true, Name = "stdin" }.Start();
        Log.Write("gestartet");
        while (_running)
        {
            Pump();
            Drain();
            if (_shown)
                Frame();
            else
                _wake.WaitOne(100);
        }
        _gpu?.Dispose();
        Log.Write("beendet");
        return 0;
    }

    void CreateWindow()
    {
        var wc = new Native.WNDCLASSEX
        {
            cbSize = System.Runtime.InteropServices.Marshal.SizeOf<Native.WNDCLASSEX>(),
            lpfnWndProc = _proc,
            hInstance = Native.GetModuleHandleW(null),
            lpszClassName = ClassName,
        };
        Native.RegisterClassExW(ref wc);
        int ex = Native.WS_EX_NOREDIRECTIONBITMAP | Native.WS_EX_LAYERED | Native.WS_EX_TRANSPARENT
                 | Native.WS_EX_TOOLWINDOW | Native.WS_EX_NOACTIVATE | Native.WS_EX_TOPMOST;
        _hwnd = Native.CreateWindowExW(ex, ClassName, "Agent Deck Fokus", Native.WS_POPUP,
            0, 0, 1, 1, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        if (_hwnd == IntPtr.Zero)
            throw new InvalidOperationException("Fenster liess sich nicht anlegen");
        // Ohne diesen Aufruf zeigt Windows ein Layered-Fenster gar nicht an.
        Native.SetLayeredWindowAttributes(_hwnd, 0, 255, Native.LWA_ALPHA);
    }

    void ReadStdin()
    {
        try
        {
            using var stdin = new StreamReader(Console.OpenStandardInput(), new UTF8Encoding(false));
            string? line;
            while ((line = stdin.ReadLine()) != null)
            {
                _commands.Enqueue(line);
                _wake.Set();
            }
        }
        catch (Exception e)
        {
            Log.Write("stdin: " + e.Message);
        }
        _commands.Enqueue("{\"cmd\":\"quit\"}");       // Deck weg = Rahmen weg
        _wake.Set();
    }

    static void Pump()
    {
        while (Native.PeekMessageW(out var msg, IntPtr.Zero, 0, 0, Native.PM_REMOVE))
        {
            Native.TranslateMessage(ref msg);
            Native.DispatchMessageW(ref msg);
        }
    }

    void Drain()
    {
        while (_commands.TryDequeue(out var line))
        {
            try
            {
                using var doc = JsonDocument.Parse(line);
                var root = doc.RootElement;
                switch (root.GetProperty("cmd").GetString())
                {
                    case "show":
                        var target = new IntPtr(root.GetProperty("hwnd").GetInt64());
                        int design = Designs.IdOf(root.GetProperty("design").GetString() ?? "");
                        int seed = root.TryGetProperty("seed", out var s) ? s.GetInt32() : 0;
                        if (!_shown || target != _target || design != _design || seed != _seed)
                            _t0 = _clock.Elapsed.TotalSeconds;          // neues Design: Uhr auf null
                        (_target, _design, _seed, _shown) = (target, design, seed, true);
                        break;
                    case "hide":
                        Hide();
                        break;
                    case "quit":
                        _running = false;
                        break;
                }
            }
            catch (Exception e)
            {
                Log.Write($"Kommando nicht verstanden ({e.Message}): {line}");
            }
        }
    }

    void Hide()
    {
        _shown = false;
        if (_onScreen)
            Native.ShowWindow(_hwnd, Native.SW_HIDE);
        _onScreen = false;
        _placed = default;
    }

    void Frame()
    {
        if (!Native.IsWindow(_target) || Native.IsIconic(_target))
        {
            Hide();
            return;
        }
        var vr = Native.VisibleRect(_target);
        var mr = Native.MonitorRect(_target);
        if (vr is not { } r || mr is not { } m)
        {
            Hide();
            return;
        }
        double sc = Math.Max(1, Native.GetDpiForWindow(_target)) / 96.0;
        int g = (int)Math.Ceiling(Designs.MarginPt * sc);
        // Rahmenfenster = Agentenfenster plus Hof, gestutzt auf seinen Monitor: der Hof
        // soll nicht auf den Nachbarbildschirm leuchten.
        int x0 = Math.Max(r.Left - g, m.Left), y0 = Math.Max(r.Top - g, m.Top);
        int x1 = Math.Min(r.Right + g, m.Right), y1 = Math.Min(r.Bottom + g, m.Bottom);
        int w = x1 - x0, h = y1 - y0;
        if (w <= 0 || h <= 0)
        {
            Hide();
            return;
        }
        _gpu ??= new Renderer(_hwnd, w, h);
        _gpu.Resize(w, h);
        var win = new Vector4(r.Left - x0, r.Top - y0, r.Right - x0, r.Bottom - y0);
        double t = _clock.Elapsed.TotalSeconds - _t0;
        var p = Designs.Compute(_design, _seed, t, sc, win, r.Right - r.Left, r.Bottom - r.Top);
        _gpu.Render(p);                                          // wartet auf den Bildtakt
        var place = (x0, y0, w, h);
        if (place != _placed || !_onScreen)
        {
            Native.SetWindowPos(_hwnd, Native.HWND_TOPMOST, x0, y0, w, h,
                Native.SWP_NOACTIVATE | Native.SWP_SHOWWINDOW | Native.SWP_NOOWNERZORDER);
            _placed = place;
            _onScreen = true;
        }
    }
}
