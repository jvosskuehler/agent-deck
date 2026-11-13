// Der eigene Glow (Design 90), eingestellt im Fenster "Glow einstellen" (deck_core::glow):
// fuenf Glow-Arten und fuenf Animationen (Canvas "Fokus-Rahmen Glow und Animation",
// 2026-11-12). Wird hinter frame_winter.hlsl gehaengt und nutzt dessen whash2().
//
// Das Licht: vier weiche Schichten wie Kamera-Bloom (eng und hell bis weit und zart) und
// ein langer Auslauf, der bei E_M Punkten sicher null ist, also nie abgeschnitten wirkt.
// Eine Tonkurve (1 - exp(-x)) statt einer Klemme: auch bei voller Staerke bleibt ein
// Verlauf, nie eine Flaeche. Dunkel leuchtet der Glow fast additiv (Farbe heller als sein
// Alpha), er hellt auf, was darunter liegt, statt es zuzudecken; hell liegt er als
// farbiger Schatten auf dem Papier. Der Abstand wird zum Fenster mit runden Ecken
// gemessen, eine eigene Linie am Rand gibt es nicht.
//
// pa       Farbe (rgb), Staerke (w)
// pb       Reichweite (x), Glow-Art (y), Animation (z), dunkles Terminal (w)
// items[0] zweite Farbe (rgb), Tempo (w)
//
// Glow-Art:  0 Weichlicht, 1 Glutsaum, 2 Zwielicht, 3 Bodenlicht, 4 Ecklicht
// Animation: 0 still, 1 Atmen, 2 Umlauf, 3 Stroemung, 4 Farbwandern, 5 Funken

static const float E_TAU = 6.2831853;
static const float E_M = 60.0;     // Hof in Punkten, wie MARGIN_PT
static const float E_FAN = 16.0;   // Laenge, die eine Ecke auf dem Umfang einnimmt
static const float E_RAD = 8.0;    // Eckenradius der Fenster unter Windows 11

// Fester Zufall je Gitterpunkt (ganzzahlig, siehe whash2 in frame_winter.hlsl).
float ehash(float2 i) { return whash2(i * 0.125); }
float emod(float x, float n) { return x - n * floor(x / n); }
// Werterauschen, in x nahtlos mit Periode n (Zellen): so hat der Umfang keine Naht.
float epn(float2 p, float n)
{
    float2 i = floor(p), f = frac(p), w = f * f * (3 - 2 * f);
    float x0 = emod(i.x, n), x1 = emod(i.x + 1, n);
    return lerp(lerp(ehash(float2(x0, i.y)), ehash(float2(x1, i.y)), w.x),
                lerp(ehash(float2(x0, i.y + 1)), ehash(float2(x1, i.y + 1)), w.x), w.y);
}
float efbm(float2 p, float n) { return 0.62 * epn(p, n) + 0.38 * epn(p * 2 + float2(3.0, 9.2), n * 2); }
float egs(float d, float s) { float x = d / s; return exp(-x * x); }

// Funken: kleine Lichtpunkte loesen sich von der Kante, treiben hinaus und verloeschen.
// m streckt den Abstand entlang der Kante in den Eckfaechern auf Punkte.
float esparks(float s, float d, float t, float P, float m)
{
    float n = floor(P / 34.0), cw = P / n, ci = floor(s / cw), acc = 0;
    [unroll] for (int j = -1; j <= 1; j++)
    {
        float c = emod(ci + j, n);
        [unroll] for (int k = 0; k < 2; k++)
        {
            float seed = c * 2 + k;
            float life = 2.2 + 1.8 * ehash(float2(seed, 1));
            float ph = (t + 10 * ehash(float2(seed, 2))) / life;
            float cyc = floor(ph), u = frac(ph);
            float on = step(0.58, ehash(float2(seed, 100 + cyc * 4)));
            float sx = (c + ehash(float2(seed, 101 + cyc * 4))) * cw;
            float side = (ehash(float2(seed, 102 + cyc * 4)) - 0.5) * 16 * u;
            float away = u * (16 + 26 * ehash(float2(seed, 103 + cyc * 4)));
            float2 dv = float2(wrapd(s - sx - side, P) * m, d - away);
            float a = smoothstep(0, 0.1, u) * (1 - u) * (1 - u);
            float r2 = dot(dv, dv);
            acc += on * a * (exp(-r2 / 1.4) + 0.22 * exp(-r2 / 20.0));
        }
    }
    return acc;
}

float4 eigen(float2 p, float u, float s, float W, float H)
{
    float sc = misc.x;
    float2 q = (p - win.xy) / sc;
    W /= sc; H /= sc;
    // Abstand zum Fenster mit runden Ecken: aussen positiv.
    float2 hf = float2(W, H) * 0.5;
    float2 v = abs(q - hf) - (hf - E_RAD);
    float d = length(max(v, 0)) + min(max(v.x, v.y), 0) - E_RAD;
    if (d > E_M || d < -1.5) return float4(0, 0, 0, 0);

    // Stelle auf dem Umfang, im Uhrzeigersinn ab oben links; jede Ecke ist ein Faecher.
    float2 o = q - clamp(q, 0, float2(W, H));
    float P = 2 * (W + H) + 4 * E_FAN, qa = E_TAU * 0.25;
    if (o.x == 0 && o.y <= 0) s = q.x;
    else if (o.x > 0 && o.y < 0) s = W + atan2(o.x, -o.y) / qa * E_FAN;
    else if (o.y == 0 && o.x > 0) s = W + E_FAN + q.y;
    else if (o.x > 0 && o.y > 0) s = W + E_FAN + H + atan2(o.y, o.x) / qa * E_FAN;
    else if (o.x == 0 && o.y > 0) s = W + 2 * E_FAN + H + (W - q.x);
    else if (o.x < 0 && o.y > 0) s = 2 * W + 2 * E_FAN + H + atan2(-o.x, o.y) / qa * E_FAN;
    else if (o.y == 0 && o.x < 0) s = 2 * W + 3 * E_FAN + H + (H - q.y);
    else s = 2 * W + 3 * E_FAN + 2 * H + atan2(-o.y, -o.x) / qa * E_FAN;
    float fan = (o.x != 0 && o.y != 0) ? length(o) * qa / E_FAN : 1.0;

    int kind = (int)(pb.y + 0.5), anim = (int)(pb.z + 0.5);
    float I = pa.w, t = misc.w * items[0].w;
    float3 col = pa.rgb, col2 = items[0].rgb;
    float gain = 1, rmul = 1, router = 1;

    // Langsames Rauschen entlang der Kante; bei "Stroemung" ziehen zwei Lagen gegeneinander.
    float flowing = anim == 3 ? 1.0 : 0.0;
    float nA = max(1.0, floor(P / (kind == 1 ? 120.0 : 60.0)));
    float sa = s / P * nA;
    float f1 = efbm(float2(sa - flowing * t * 0.16, 1.3 + flowing * t * 0.07), nA);
    float f2 = efbm(float2(sa + flowing * t * 0.10 + 11.0, 7.9 - flowing * t * 0.05), nA);
    float flow = smoothstep(0.22, 0.78, 0.5 * (f1 + f2));
    if (kind == 1)                                   // Glutsaum: ungleiche Zungen
    {
        float nW = max(1.0, floor(P / 26.0));
        float wn = epn(float2(s / P * nW - flowing * t * 0.2, d / 20.0 - flowing * t * 0.22 + 2.0), nW);
        router *= 0.55 + 1.05 * flow;
        gain *= (0.78 + 0.44 * f2) * (0.9 + 0.2 * wn);
    }
    else if (anim == 3)
    {
        router *= 0.62 + 0.8 * flow;
        gain *= 0.8 + 0.4 * f2;
    }
    if (kind == 2)                                   // Zwielicht: zwei Farben gegenueber
    {
        float cm = 0.5 - 0.5 * cos(E_TAU * (s / P + 0.5 * E_FAN / P));
        col = lerp(pa.rgb, items[0].rgb, smoothstep(0.1, 0.9, cm));
        col2 = col * 0.8;
    }
    if (kind == 3)                                   // Bodenlicht: unten kraeftig
    {
        float ty = saturate(q.y / H);
        gain *= 0.06 + 0.94 * pow(ty, 2.4);
        rmul *= 1.0 + 0.35 * smoothstep(H - 40.0, H + 6.0, q.y);
    }
    if (kind == 4)                                   // Ecklicht: vier Lichter in den Ecken
    {
        float e = min(min(abs(wrapd(s - (W + 0.5 * E_FAN), P)), abs(wrapd(s - (W + H + 1.5 * E_FAN), P))),
                      min(abs(wrapd(s - (2 * W + H + 2.5 * E_FAN), P)), abs(wrapd(s - (P - 0.5 * E_FAN), P))));
        float cw = 0.2 * min(W, H);
        float c = exp(-(e / cw) * (e / cw));
        gain *= 0.1 + 0.9 * c;
        router *= 0.8 + 0.45 * c;
    }
    if (anim == 1)                                   // Atmen
    {
        float b = (exp(sin(E_TAU * t / 6.0)) - 0.3679) / 2.3504;
        gain *= lerp(0.48, 1.0, b);
        rmul *= lerp(0.86, 1.06, b);
    }
    if (anim == 2)                                   // Umlauf: ein Lichtkamm wandert
    {
        float dl = wrapd(s - frac(t / 9.0) * P, P);
        float c = dl > 0 ? exp(-(dl / 46.0) * (dl / 46.0))
                         : exp(dl / (0.16 * P)) * smoothstep(-0.5 * P, -0.25 * P, dl);
        gain *= 0.58 + 1.1 * c;
        rmul *= 0.92 + 0.36 * c;
    }
    if (anim == 4)                                   // Farbwandern
    {
        float cm = 0.5 + 0.5 * sin(E_TAU * (s / P - t / 16.0) + 2.4 * (f1 - 0.5));
        col = lerp(pa.rgb, items[0].rgb, smoothstep(0.15, 0.85, cm));
        col2 = col * 0.8;
    }

    float R = pb.x * rmul, RO = R * router;
    float g1 = egs(d, 3.0 * R), g2 = egs(d, 7.0 * R), g3 = egs(d, 15.0 * RO), g4 = egs(d, 30.0 * RO);
    float lz = 1.0 / (1.0 + (d / (9.0 * RO)) * (d / (9.0 * RO)));
    float k = I * gain * saturate(d * sc + 0.5) * (1.0 - smoothstep(0.45 * E_M, E_M, d));
    float sp = anim == 5 ? esparks(s, d, t, P, fan) * I : 0.0;
    float4 c4;
    if (pb.w > 0.5)
    {
        float3 cN = lerp(col, float3(1, 1, 1), 0.15);
        float3 x = cN * (0.10 * g1) + lerp(cN, col, 0.7) * (0.32 * g2) + lerp(col, col2, 0.55) * (0.26 * g3)
                 + col2 * (0.13 * g4 + 0.05 * lz);
        x = x * k + lerp(col, float3(1, 1, 1), 0.55) * sp * 1.6;
        float3 rgb = 1.0 - exp(-1.35 * x);
        c4 = float4(rgb, 0.4 * max(rgb.r, max(rgb.g, rgb.b)));
    }
    else
    {
        float near = 0.10 * g1 + 0.32 * g2, far = 0.26 * g3 + 0.13 * g4 + 0.05 * lz;
        float a = 1.0 - exp(-1.2 * (near + far) * k - sp * 1.2);
        c4 = float4(lerp(col, col2, far / (near + far + 0.0001)) * a, a);
    }
    // Feines Rauschen gegen Stufen in den langen Auslaeufen.
    float dn = (hash21(p + frac(misc.w) * 61.0) - 0.5) / 255.0;
    c4 += dn * min(1.0, max(c4.a, max(c4.r, c4.g)) * 60.0);
    return saturate(c4);
}
