// Messwerkzeug für den Messstand des Agent Decks (tools/probe/README.md). Wird von den
// Gast-Skripten per Add-Type geladen, läuft in der Desktop-Sitzung der Test-VM.
//
// - WinLog: jede Lageänderung, jedes Zeigen/Verstecken und jeden Vordergrundwechsel der
//   Agentenfenster (Windows Terminal) und der Hilfsfenster des Decks (Vorhang, Ringe,
//   Schatten, Fokus-Rahmen) mit Zeitstempel (QPC, ms seit Start) als JSON-Zeilen.
// - Sync: ein 12x12-Pixel-Feld oben links. Jede Marke färbt es um;
//   daran richtet die Auswertung Video und Ereignisprotokoll aufeinander aus.
// - Keys, Display, Wallpaper: Tastenkürzel senden, Auflösung, einfarbiger Hintergrund.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class Probe
{
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public IntPtr w, l; public uint t; public int x, y; }
    delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int obj, int child, uint thread, uint time);
    delegate IntPtr WndProc(IntPtr h, uint m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASS { public uint style; public WndProc proc; public int c1, c2; public IntPtr inst, icon, cursor, bg; public string menu, name; }
    [StructLayout(LayoutKind.Sequential)] struct PAINT { public IntPtr hdc; public int erase; public RECT rc; public int a, b; [MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)] public byte[] r; }

    [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint a, uint b, IntPtr mod, WinEventProc p, uint pid, uint tid, uint flags);
    [DllImport("user32.dll")] static extern int GetMessage(out MSG m, IntPtr h, uint a, uint b);
    [DllImport("user32.dll")] static extern int TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG m);
    [DllImport("user32.dll")] static extern bool PostThreadMessage(uint tid, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern ushort RegisterClass(ref WNDCLASS c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateWindowEx(uint ex, string cls, string name, uint style, int x, int y, int w, int h, IntPtr p, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProc(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] static extern bool InvalidateRect(IntPtr h, IntPtr r, bool e);
    [DllImport("user32.dll")] static extern bool UpdateWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr BeginPaint(IntPtr h, out PAINT p);
    [DllImport("user32.dll")] static extern bool EndPaint(IntPtr h, ref PAINT p);
    [DllImport("user32.dll")] static extern int FillRect(IntPtr dc, ref RECT r, IntPtr br);
    [DllImport("gdi32.dll")] static extern IntPtr CreateSolidBrush(uint rgb);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] static extern bool SetLayeredWindowAttributes(IntPtr h, uint key, byte a, uint f);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
    [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandle(string n);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
    [DllImport("user32.dll")] static extern IntPtr SetTimer(IntPtr h, IntPtr id, uint ms, IntPtr proc);

    const uint EV_FOREGROUND = 0x0003, EV_MOVESIZESTART = 0x000A, EV_MOVESIZEEND = 0x000B,
        EV_MINSTART = 0x0016, EV_MINEND = 0x0017, EV_DESTROY = 0x8001, EV_SHOW = 0x8002, EV_HIDE = 0x8003,
        EV_LOCATION = 0x800B;
    const uint WM_PAINT = 0x000F, WM_APP = 0x8000, WM_QUIT = 0x0012;

    static readonly Stopwatch clock = Stopwatch.StartNew();
    static StreamWriter log;
    static readonly object gate = new object();
    static uint loopThread;
    static Thread thread;
    static IntPtr syncWnd;
    static volatile uint syncColor = 0x000000;
    // Das Sync-Feld flackert unmerklich (unterstes Bit), damit QEMU den Bildschirm im
    // schnellen Takt weiterreicht: ohne Änderung wird sein VNC-Takt langsamer.
    static bool flicker;
    static WinEventProc keepProc;
    static WndProc keepWnd;
    static readonly List<IntPtr> hooks = new List<IntPtr>();

    // Farben der Sync-Marken (BGR für GDI). Die Auswertung kennt dieselbe Reihe.
    public static readonly uint[] SyncColors = { 0x0000FF, 0x00FF00, 0xFF0000, 0x00FFFF, 0xFFFF00, 0xFF00FF, 0xFFFFFF, 0x000080 };

    public static double Now() { return clock.Elapsed.TotalMilliseconds; }

    static string Cls(IntPtr h) { var s = new StringBuilder(128); GetClassName(h, s, 128); return s.ToString(); }
    static string Title(IntPtr h) { var s = new StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
    static bool Interesting(string cls) { return cls == "CASCADIA_HOSTING_WINDOW_CLASS" || cls.StartsWith("AgentDeck"); }
    static string Esc(string s) { return s.Replace("\\", "\\\\").Replace("\"", "\\\""); }

    static void Write(string line) { lock (gate) { if (log != null) log.WriteLine(line); } }

    static string RectJson(RECT r) { return "[" + r.L + "," + r.T + "," + r.R + "," + r.B + "]"; }

    static void OnEvent(IntPtr hook, uint ev, IntPtr h, int obj, int child, uint tid, uint time)
    {
        double t = Now();
        if (h == IntPtr.Zero || obj != 0 || child != 0) return;
        if (ev != EV_FOREGROUND && GetAncestor(h, 2) != h) return;
        string cls = Cls(h);
        if (ev != EV_FOREGROUND && !Interesting(cls)) return;
        string name;
        switch (ev)
        {
            case EV_FOREGROUND: name = "fg"; break;
            case EV_MOVESIZESTART: name = "movestart"; break;
            case EV_MOVESIZEEND: name = "moveend"; break;
            case EV_MINSTART: name = "minstart"; break;
            case EV_MINEND: name = "minend"; break;
            case EV_DESTROY: name = "destroy"; break;
            case EV_SHOW: name = "show"; break;
            case EV_HIDE: name = "hide"; break;
            default: name = "loc"; break;
        }
        Write(Describe(t, name, h, cls, ev == EV_DESTROY));
    }


    static string Describe(double t, string name, IntPtr h, string cls, bool gone)
    {
        var sb = new StringBuilder();
        sb.Append("{\"t\":").Append(t.ToString("F3", System.Globalization.CultureInfo.InvariantCulture));
        sb.Append(",\"ev\":\"").Append(name).Append("\",\"h\":").Append(h.ToInt64());
        sb.Append(",\"cls\":\"").Append(Esc(cls)).Append("\",\"title\":\"").Append(Esc(Title(h))).Append("\"");
        if (!gone)
        {
            RECT r;
            if (GetWindowRect(h, out r)) sb.Append(",\"rect\":").Append(RectJson(r));
            if (DwmGetWindowAttribute(h, 9, out r, 16) == 0) sb.Append(",\"frame\":").Append(RectJson(r));
            // Ohne eigenen Schatten (gekachelt, Deck stellt ihn ab) meldet DWM das Außenrechteck;
            // sichtbar ist dann die Client-Fläche (wie deck_win::arrange::visible_rect).
            if (GetClientRect(h, out r)) { var p = new POINT(); ClientToScreen(h, ref p); r.L += p.X; r.R += p.X; r.T += p.Y; r.B += p.Y; sb.Append(",\"client\":").Append(RectJson(r)); }
            sb.Append(",\"vis\":").Append(IsWindowVisible(h) ? 1 : 0).Append(",\"min\":").Append(IsIconic(h) ? 1 : 0);
        }
        sb.Append("}");
        return sb.ToString();
    }

    /// Bestandsaufnahme beim Start: jedes beobachtete Fenster einmal als „snap“.
    static void Snapshot()
    {
        double t = Now();
        EnumWindows((h, l) => { string c = Cls(h); if (Interesting(c)) Write(Describe(t, "snap", h, c, false)); return true; }, IntPtr.Zero);
    }

    static IntPtr SyncProc(IntPtr h, uint m, IntPtr w, IntPtr l)
    {
        if (m == WM_PAINT)
        {
            PAINT p; IntPtr dc = BeginPaint(h, out p);
            RECT r = new RECT { L = 0, T = 0, R = 64, B = 64 };
            IntPtr br = CreateSolidBrush(syncColor ^ (flicker ? 0x010101u : 0u)); FillRect(dc, ref r, br); DeleteObject(br);
            EndPaint(h, ref p);
            return IntPtr.Zero;
        }
        if (m == 0x0113)
        {
            flicker = !flicker;
            // Immer wieder ganz nach oben: der Fokus-Rahmen hebt seine Bühne bei jedem
            // Fokuswechsel über alles.
            SetWindowPos(h, new IntPtr(-1), 0, 0, 0, 0, 0x1 | 0x2 | 0x10);
            InvalidateRect(h, IntPtr.Zero, false);
            return IntPtr.Zero;
        }
        return DefWindowProc(h, m, w, l);
    }

    /// Protokoll nach `path` starten (eigener Thread mit Nachrichtenschleife).
    public static void Start(string path)
    {
        log = new StreamWriter(path, false, new UTF8Encoding(false)) { AutoFlush = false };
        var ready = new ManualResetEvent(false);
        thread = new Thread(() =>
        {
            loopThread = GetCurrentThreadId();
            // Physische Pixel wie im Video, auch bei 150 % (sonst rechnet Windows um).
            SetThreadDpiAwarenessContext(new IntPtr(-4));
            keepProc = OnEvent;
            foreach (var pair in new[] { new[] { EV_FOREGROUND, EV_FOREGROUND }, new[] { EV_MOVESIZESTART, EV_MOVESIZEEND },
                     new[] { EV_MINSTART, EV_MINEND }, new[] { EV_DESTROY, EV_HIDE }, new[] { EV_LOCATION, EV_LOCATION } })
                hooks.Add(SetWinEventHook(pair[0], pair[1], IntPtr.Zero, keepProc, 0, 0, 0x0000 | 0x0002));
            keepWnd = SyncProc;
            var wc = new WNDCLASS { proc = keepWnd, inst = GetModuleHandle(null), name = "DeckProbeSync" };
            RegisterClass(ref wc);
            // Oben links: über der Taskleiste läge es darunter (die hebt sich selbst).
            // WS_EX_TOPMOST|TOOLWINDOW|NOACTIVATE|LAYERED|TRANSPARENT, WS_POPUP
            syncWnd = CreateWindowEx(0x8 | 0x80 | 0x08000000 | 0x80000 | 0x20, "DeckProbeSync", "", 0x80000000, 0, 0, 12, 12, IntPtr.Zero, IntPtr.Zero, wc.inst, IntPtr.Zero);
            Write("{\"t\":0,\"ev\":\"note\",\"data\":{\"sync_hwnd\":" + syncWnd.ToInt64() + ",\"err\":" + Marshal.GetLastWin32Error() + "}}");
            SetLayeredWindowAttributes(syncWnd, 0, 255, 2);
            ShowWindow(syncWnd, 4);
            SetTimer(syncWnd, new IntPtr(1), 25, IntPtr.Zero);
            Snapshot();
            ready.Set();
            MSG msg;
            while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0)
            {
                if (msg.m == WM_APP)
                {
                    syncColor = (uint)msg.w.ToInt64();
                    SetWindowPos(syncWnd, new IntPtr(-1), 0, 0, 0, 0, 0x1 | 0x2 | 0x10);
                    InvalidateRect(syncWnd, IntPtr.Zero, false);
                    UpdateWindow(syncWnd);
                    Write("{\"t\":" + Now().ToString("F3", System.Globalization.CultureInfo.InvariantCulture) + ",\"ev\":\"sync\",\"color\":" + msg.l.ToInt64() + "}");
                    continue;
                }
                TranslateMessage(ref msg); DispatchMessage(ref msg);
            }
        });
        thread.SetApartmentState(ApartmentState.STA);
        thread.IsBackground = true;
        thread.Start();
        ready.WaitOne();
    }

    static int marks = 0;

    /// Marke ins Protokoll und das Sync-Feld umfärben. Liefert die Zeit der Marke.
    public static double Mark(string name)
    {
        double t = Now();
        int k = marks++ % SyncColors.Length;
        Write("{\"t\":" + t.ToString("F3", System.Globalization.CultureInfo.InvariantCulture) + ",\"ev\":\"mark\",\"name\":\"" + Esc(name) + "\",\"color\":" + k + "}");
        PostThreadMessage(loopThread, WM_APP, new IntPtr(SyncColors[k]), new IntPtr(k));
        return t;
    }

    /// Freie Zeile ins Protokoll (Zustand, Zeiten aus dem Skript).
    public static void Note(string json) { Write("{\"t\":" + Now().ToString("F3", System.Globalization.CultureInfo.InvariantCulture) + ",\"ev\":\"note\",\"data\":" + json + "}"); }

    public static void Stop()
    {
        PostThreadMessage(loopThread, WM_QUIT, IntPtr.Zero, IntPtr.Zero);
        thread.Join(2000);
        lock (gate) { log.Flush(); log.Dispose(); log = null; }
    }

    // ── Tastatur ──
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public KEYBDINPUT k; public long pad; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);

    /// Tastenkombination senden, z. B. { 0x11, 0x10, 0xE2 } = Strg+Shift+<.
    public static void Keys(ushort[] vks)
    {
        var list = new List<INPUT>();
        foreach (var v in vks) list.Add(new INPUT { type = 1, k = new KEYBDINPUT { vk = v } });
        for (int i = vks.Length - 1; i >= 0; i--) list.Add(new INPUT { type = 1, k = new KEYBDINPUT { vk = vks[i], flags = 2 } });
        SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
    }

    // ── Maus ──
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);

    // ── Fenster suchen ──
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);

    /// Sichtbare Agentenfenster: Titel → Rahmen (DWM).
    public static Dictionary<string, RECT> Agents()
    {
        SetThreadDpiAwarenessContext(new IntPtr(-4));
        var d = new Dictionary<string, RECT>();
        EnumWindows((h, l) =>
        {
            if (Cls(h) == "CASCADIA_HOSTING_WINDOW_CLASS" && IsWindowVisible(h) && !IsIconic(h))
            {
                RECT r;
                if (DwmGetWindowAttribute(h, 9, out r, 16) == 0) d[Title(h)] = r;
            }
            return true;
        }, IntPtr.Zero);
        return d;
    }

    public static string ForegroundTitle() { return Title(GetForegroundWindow()); }

    // ── Bildschirm ──
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct DEVMODE
    {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string name;
        public short specV, drvV, size, extra; public int fields, x, y, orient, disp;
        public short color, duplex, yres, tt, collate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string form;
        public short logPixels; public int bits, width, height, flags, freq;
        public int icm, icmI, media, dither, r1, r2, pw, ph;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);

    /// Auflösung setzen; 0 = geklappt.
    public static int SetResolution(int w, int h)
    {
        var dm = new DEVMODE(); dm.size = (short)Marshal.SizeOf(typeof(DEVMODE));
        EnumDisplaySettings(null, -1, ref dm);
        dm.width = w; dm.height = h; dm.fields = 0x80000 | 0x100000;
        return ChangeDisplaySettings(ref dm, 1);
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool SystemParametersInfo(uint a, uint b, string c, uint f);
    [DllImport("user32.dll")] static extern bool SetSysColors(int n, int[] e, int[] v);

    [DllImport("user32.dll", EntryPoint = "SystemParametersInfoW")] static extern bool SystemParametersInfoInt(uint a, int b, IntPtr c, uint f);

    /// Skalierung des Hauptmonitors in Stufen über der empfohlenen (0 = empfohlen, 1 = eine
    /// Stufe größer, etwa 125 %). Undokumentiert (SPI_SETLOGICALDPIOVERRIDE), wirkt sofort
    /// wie die Einstellung in „Anzeige“: Der Basic Display Adapter der VM kann keine andere
    /// Auflösung, eine andere Skalierung schon.
    public static bool SetScaleStep(int step) { return SystemParametersInfoInt(0x009F, step, IntPtr.Zero, 1); }

    /// Einfarbiger Desktop (RGB), ohne Bild.
    public static void SolidDesktop(int r, int g, int b)
    {
        SystemParametersInfo(0x14, 0, "", 3);
        SetSysColors(1, new[] { 1 }, new[] { r | (g << 8) | (b << 16) });
    }
}
