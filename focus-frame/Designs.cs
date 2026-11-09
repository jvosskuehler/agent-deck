using System.Numerics;
using System.Runtime.InteropServices;

namespace FocusFrame;

/// <summary>Was der Shader je Bild bekommt (Aufbau wie cbuffer Params in Frame.hlsl).</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Params
{
    public Vector4 Win, Misc, A, B;
    public Vector4 I0, I1, I2, I3, I4, I5, I6, I7;

    public void SetItem(int k, Vector4 v)
    {
        switch (k)
        {
            case 0: I0 = v; break; case 1: I1 = v; break; case 2: I2 = v; break;
            case 3: I3 = v; break; case 4: I4 = v; break; case 5: I5 = v; break;
            case 6: I6 = v; break; case 7: I7 = v; break;
        }
    }
}

/// <summary>Die vier Designs (Tafeln 03 bis 06 im Canvas "Fokus-Rahmen Varianten"):
/// alles, was je Bild wechselt, in Doppelgenauigkeit hier, die Pixel im Shader.
/// Masse in Punkten mal Skalierung, Geschwindigkeiten in Punkten je Sekunde: so wirkt
/// ein Design auf einem grossen Fenster genauso schnell wie auf der Tafel.</summary>
public static class Designs
{
    public static readonly string[] Names = { "kometen", "glut", "hochspannung", "lauflicht" };

    /// <summary>Wie weit der Hof ueber die Fensterkante hinausreicht, in Punkten.</summary>
    public const double MarginPt = 36;

    public static int IdOf(string name)
    {
        int i = Array.IndexOf(Names, name);
        return i < 0 ? 0 : i;
    }

    static double Mod(double x, double m) => x - m * Math.Floor(x / m);

    static readonly double[] Levels = { 0.5, 0.65, 0.8, 1.0 };

    /// <summary>Zufallszahl 0..1, fest je (seed, a, b): dieselben Funken und Boegen im
    /// selben Takt, ohne je Bild einen Zufallsgenerator anzulegen.</summary>
    static double Hash(int seed, long a, long b)
    {
        ulong x = unchecked((ulong)seed * 0x9E3779B97F4A7C15UL ^ (ulong)a * 0xBF58476D1CE4E5B9UL
                            ^ (ulong)b * 0x94D049BB133111EBUL);
        x ^= x >> 31; x *= 0x7FB5D329728EA185UL; x ^= x >> 27; x *= 0x81DADEF4BC2DD44DUL; x ^= x >> 33;
        return (x >> 11) * (1.0 / (1UL << 53));
    }

    public static Params Compute(int design, int seed, double t, double sc,
                                 Vector4 win, double width, double height)
    {
        double P = 2 * (width + height);
        var p = new Params { Win = win, Misc = new Vector4((float)sc, (float)P, design, (float)Mod(t, 1000)) };
        switch (design)
        {
            case 0: Kometen(ref p, t, sc, P, width); break;
            case 1: Glut(ref p, seed, t, sc, P, win, width); break;
            case 2: Hochspannung(ref p, seed, t, sc, P); break;
            default: Lauflicht(ref p, t, sc); break;
        }
        return p;
    }

    // 03: ein oranger und ein goldener Komet laufen gegeneinander; wo sie sich
    // treffen (oben und unten in der Mitte), blitzt der Hof auf.
    static void Kometen(ref Params p, double t, double sc, double P, double width)
    {
        double lap = P / (430 * sc);
        double ph = Mod(t / lap, 1);
        double start = width / 2;
        double headA = Mod(start + ph * P, P), headB = Mod(start - ph * P, P);
        double near = Math.Min(Math.Min(ph, Math.Abs(ph - 0.5)), 1 - ph) * lap;
        double amp = Math.Exp(-Math.Pow(near / 0.2, 2));
        double burstAt = (ph < 0.25 || ph > 0.75) ? start : Mod(start + P / 2, P);
        p.A = new Vector4((float)headA, (float)headB, (float)(0.17 * P), (float)amp);
        p.B = new Vector4((float)burstAt, (float)(10 * sc), 0, 0);
    }

    // 04: gluehender Verlauf fliesst langsam herum, der Hof flackert, oben steigen
    // Funken auf.
    static void Glut(ref Params p, int seed, double t, double sc, double P, Vector4 win, double width)
    {
        double n = 0.5 + 0.25 * Math.Sin(2 * Math.PI * 0.83 * t) + 0.15 * Math.Sin(2 * Math.PI * 2.3 * t + 1)
                   + 0.1 * Math.Sin(2 * Math.PI * 5.1 * t + 2);
        p.A = new Vector4((float)Mod(t * 107 * sc, P), (float)(0.55 + 0.45 * n), 0, 0);
        for (int k = 0; k < 7; k++)
        {
            double period = 2.1 + Hash(seed, k, 1) * 1.1, phase = Hash(seed, k, 2) * 3;
            double u = Mod((t + phase) / period, 1);
            long cycle = (long)Math.Floor((t + phase) / period);
            double xf = 0.05 + 0.9 * Hash(seed, k, cycle);
            double alpha = Math.Min(u / 0.12, 1) * (1 - u);
            p.SetItem(k, new Vector4((float)(win.X + xf * width + 9 * sc * u),
                                     (float)(win.Y - u * 26 * sc), (float)alpha, (float)(1.4 * sc)));
        }
    }

    static readonly (double at, double val)[] Flick =
        { (0, 1), (0.12, 0.55), (0.2, 1), (0.43, 0.8), (0.5, 1), (0.71, 0.4), (0.76, 1) };

    // 05: Cyan wie die Deck-Kante: ein flackernder Draht, Lichtboegen springen ueber
    // den Rand, zwei Funken rasen gegeneinander herum.
    static void Hochspannung(ref Params p, int seed, double t, double sc, double P)
    {
        double ph = Mod(t / 0.9, 1), flick = 1;
        foreach (var (at, val) in Flick)
            if (ph >= at) flick = val;
        long tick = (long)Math.Floor(t / 0.06);              // so oft springen die Boegen
        int n = 5 + (int)(Hash(seed, 99, tick) * 4);
        double level = Levels[(int)(Hash(seed, 98, tick) * Levels.Length) % Levels.Length];
        for (int k = 0; k < 8; k++)
            p.SetItem(k, new Vector4((float)(Hash(seed, k, tick * 4) * P),
                                     (float)((10 + Hash(seed, k, tick * 4 + 1) * 50) * sc),
                                     (float)((Hash(seed, k, tick * 4 + 2) - 0.5) * 2 * sc),
                                     (float)(Hash(seed, k, tick * 4 + 3) * 100)));
        double z1 = Mod(t * 1010 * sc, P), z2 = Mod(-t * 660 * sc, P);
        p.A = new Vector4((float)flick, (float)level, (float)z1, (float)z2);
        p.B = new Vector4((float)(0.45 * 12 * sc), n, (float)tick, 0);
    }

    // 06: Gluehbirnen wie am Kinoschild, jede dritte brennt hell, das Licht huepft im
    // Takt im Uhrzeigersinn herum.
    static void Lauflicht(ref Params p, double t, double sc)
    {
        double phase = Math.Floor(t / (0.42 / 3)) % 3;
        p.A = new Vector4((float)phase, (float)Math.Max(6, Math.Round(8 * sc)), (float)Math.Max(1.5, 2.6 * sc), 0);
    }
}
