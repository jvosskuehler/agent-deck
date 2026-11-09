// Der Fokus-Rahmen, Pixel fuer Pixel auf der Grafikkarte gerechnet.
//
// Jeder Pixel des Rahmenfensters fragt: wie tief liege ich im Agentenfenster (u, innen
// positiv, aussen negativ) und an welcher Stelle des Umfangs (s, im Uhrzeigersinn ab der
// Ecke oben links)? Daraus ergeben sich Linie und Leuchthof analytisch, ohne Pixelraster
// dazwischen: Kanten sind geglaettet, Verlaeufe stufenlos, alles in der vollen Aufloesung
// des Monitors. Was je Bild wechselt (Lage der Kometen, Boegen, Funken), rechnet
// Designs.cs und uebergibt es in den Parametern.
//
// Ausgabe ist vormultipliziertes Alpha (DirectComposition, AlphaMode.Premultiplied).

cbuffer Params : register(b0)
{
    float4 win;        // Agentenfenster im Rahmenfenster: x0, y0, x1, y1 (Pixel)
    float4 misc;       // Skalierung, Umfang P, Design (0..3), Zeit
    float4 pa;         // je Design
    float4 pb;         // je Design
    float4 items[8];   // je Design: Funken bzw. Lichtboegen
};

struct VSOut { float4 pos : SV_Position; };

VSOut VSMain(uint id : SV_VertexID)
{
    VSOut o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}

static const float3 ORANGE = float3(217, 119, 87) / 255.0;

float hash11(float x) { return frac(sin(x * 127.1 + 311.7) * 43758.5453); }
float hash21(float2 p) { return frac(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453); }

float noise1(float x)
{
    float i = floor(x), f = frac(x);
    float u = f * f * (3 - 2 * f);
    return lerp(hash11(i), hash11(i + 1), u);
}

// Abstand auf dem Umfang, ueber die Naht bei s = 0 hinweg: Ergebnis in (-P/2, P/2].
float wrapd(float x, float P) { return x - P * round(x / P); }

void edge(float2 p, out float u, out float s, out float W, out float H)
{
    float2 q = p - win.xy;
    W = win.z - win.x;
    H = win.w - win.y;
    float2 c = clamp(q, 0, float2(W, H));
    if (all(q >= 0) && all(q <= float2(W, H)))
    {
        float dl = q.x, dt = q.y, dr = W - q.x, db = H - q.y;
        float m = min(min(dl, dr), min(dt, db));
        u = m;
        if (m == dt) s = q.x;
        else if (m == dr) s = W + q.y;
        else if (m == db) s = W + H + (W - q.x);
        else s = 2 * W + H + (H - q.y);
    }
    else
    {
        u = -length(q - c);
        if (c.y <= 0 && c.x < W) s = c.x;
        else if (c.x >= W && c.y < H) s = W + c.y;
        else if (c.y >= H && c.x > 0) s = W + H + (W - c.x);
        else s = 2 * W + H + (H - c.y);
    }
}

// Deckung der Linie zwischen den Tiefen lo und hi, an den Kanten geglaettet.
float stroke(float u, float lo, float hi) { return saturate(min(u - lo, hi - u) + 0.5); }

// Leuchthof um die Linie: nach aussen eng (s1) plus weit (s2), nach innen nur eng.
float halo(float u, float lo, float hi, float s1, float a1, float s2, float a2)
{
    if (u < lo) { float d = lo - u; return a1 * exp(-(d / s1) * (d / s1)) + a2 * exp(-(d / s2) * (d / s2)); }
    if (u > hi) { float d = u - hi; return a1 * exp(-(d / s1) * (d / s1)); }
    return a1 + a2;
}

// ── 0 Kometen ─────────────────────────────────────────────
// pa: Kopf A, Kopf B, Schweiflaenge, Aufblitzen; pb: Ort des Aufblitzens, Hitzezone
float comet(float k, float L, float soft)
{
    return smoothstep(-soft, soft, k) * pow(saturate(1 - max(k, 0) / L), 1.6);
}

float4 kometen(float u, float s, float P, float sc)
{
    float L = pa.z, hot = pb.y;
    float kA = wrapd(pa.x - s, P);
    float kB = wrapd(s - pa.y, P);
    float3 cA = lerp(float3(1, 0.988, 0.973), ORANGE, saturate(kA / hot));
    float3 cB = lerp(float3(1, 1, 1), float3(1, 0.769, 0.541), saturate(kB / hot));
    float blur = 7 * sc;
    float lin = stroke(u, 0, 3 * sc);
    float amp = pa.w;
    float bump = amp * exp(-pow(wrapd(s - pb.x, P) / max(6.0, 0.03 * P), 2));
    float hal = halo(u, 0, 3 * sc, 5 * sc, 1.0, 15 * sc, 0.55) * (0.75 + 0.25 * amp);
    float iA = comet(kA, L, 0.7), iB = comet(kB, L, 0.7);
    float gA = comet(kA, L, blur), gB = comet(kB, L, blur);
    float3 burst = float3(1, 0.925, 0.863);
    float3 rgb = ORANGE * 0.22 * lin
               + (cA * iA + cB * iB + burst * bump) * lin
               + (cA * gA + cB * gB + burst * bump) * hal;
    float a = 0.22 * lin + (iA + iB + bump) * lin + (gA + gB + bump) * hal;
    return float4(rgb, a);
}

// ── 1 Glut ────────────────────────────────────────────────
// pa: Verschiebung des Verlaufs, Hof-Staerke; items: Funken (x, y, Deckung, Radius)
static const float3 EMBER[8] = {
    float3(124, 45, 18) / 255.0, float3(194, 65, 12) / 255.0, float3(217, 119, 87) / 255.0,
    float3(245, 158, 11) / 255.0, float3(253, 230, 138) / 255.0, float3(245, 158, 11) / 255.0,
    float3(217, 119, 87) / 255.0, float3(194, 65, 12) / 255.0 };

float4 glut(float2 p, float u, float s, float P, float sc)
{
    float x = frac((s - pa.x) / P * 2) * 8;
    int i = (int)floor(x);
    float3 c = lerp(EMBER[i % 8], EMBER[(i + 1) % 8], frac(x));
    float lin = stroke(u, 0, 4 * sc);
    float hal = halo(u, 0, 4 * sc, 5 * sc, 0.9, 16 * sc, 0.5) * pa.y;
    float w = lin + hal;
    float3 rgb = c * w;
    float a = w;
    [unroll] for (int k = 0; k < 7; k++)
    {
        float4 sp = items[k];
        float r = length(p - sp.xy) / sp.w;
        float g = sp.z * exp(-r * r);
        rgb += float3(1, 0.886, 0.659) * g;
        a += g;
    }
    return float4(rgb, a);
}

// ── 2 Hochspannung ────────────────────────────────────────
// pa: Flackern, Hof-Staerke, Funke 1, Funke 2; pb: Funkenbreite, Zahl der Boegen, Takt
// items: Lichtboegen (Anfang, Laenge, Versatz quer zur Kante)
float4 hochspannung(float u, float s, float P, float sc)
{
    float3 cyan = float3(0.494, 0.796, 1.0), arcC = float3(0.749, 0.902, 1.0);
    float flick = pa.x, level = pa.y, zs = pb.x;
    float wire = stroke(u, 0, 2 * sc) * 0.7 * flick;
    float hal = halo(u, 0, 2 * sc, 4 * sc, 1.0, 13 * sc, 0.5) * level;
    float3 rgb = float3(0.902, 0.965, 1.0) * wire + cyan * hal * 0.45 * flick;
    float a = wire + hal * 0.45 * flick;
    float blur = 5 * sc;
    [unroll] for (int k = 0; k < 8; k++)
    {
        if (k >= (int)pb.y) break;
        float4 arc = items[k];
        float ds = s - arc.x;
        ds -= P * floor(ds / P);
        float len = arc.y, edgeLen = max(2.0, len / 4);
        float sharp = saturate(min((ds + 1) / edgeLen, (len - ds) / edgeLen));
        float soft = saturate(min((ds + blur) / (edgeLen + blur), (len - ds + blur) / (edgeLen + blur)));
        // Der Bogen zittert quer zur Kante: Rauschen entlang s, neu je Takt.
        float wob = (noise1(s * 0.35 / sc + arc.w) - 0.5) * 3.0 * sc + arc.z;
        float l = stroke(u - wob, 0, 2 * sc) * sharp;
        float h = halo(u - wob, 0, 2 * sc, 4 * sc, 1.0, 13 * sc, 0.5) * soft * level;
        rgb += arcC * l + cyan * h;
        a += l + h;
    }
    [unroll] for (int z = 0; z < 2; z++)
    {
        float pos = z == 0 ? pa.z : pa.w;
        float g = exp(-pow(wrapd(s - pos, P) / zs, 2));
        float l = stroke(u, -0.5 * sc, 2.5 * sc) * g;
        float h = halo(u, 0, 2 * sc, 4 * sc, 1.0, 13 * sc, 0.5) * g;
        rgb += float3(1, 1, 1) * l + float3(0.85, 0.95, 1) * h;
        a += l + h;
    }
    return float4(rgb, a);
}

// ── 3 Lauflicht ───────────────────────────────────────────
// pa: Takt-Phase (0..2), Birnenabstand, Birnenradius
float4 lauflicht(float u, float s, float P, float sc)
{
    float gap = pa.y, rad = pa.z;
    int phase = (int)pa.x;
    float rail = stroke(u, -0.6 * sc, 0.6 * sc) * 0.22;
    float3 rgb = ORANGE * rail;
    float a = rail;
    float j0 = floor(s / gap);
    [unroll] for (int k = -2; k <= 2; k++)
    {
        float j = j0 + k;
        float ds = wrapd(s - (j + 0.5) * gap, P);
        float r = length(float2(ds, u));
        bool bright = ((int)(j + 3000) - phase) % 3 == 0;
        float core = exp(-1.2 * (r / rad) * (r / rad));
        if (bright)
        {
            float3 c = lerp(float3(1, 0.965, 0.933), ORANGE, saturate(r / rad));
            float g = 0.8 * exp(-pow(r / (4 * sc), 2)) + 0.45 * exp(-pow(r / (11 * sc), 2));
            rgb += c * core + ORANGE * g;
            a += core + g;
        }
        else
        {
            float g = 0.25 * exp(-pow(r / (4 * sc), 2));
            rgb += ORANGE * (0.42 * core + g);
            a += 0.42 * core + g;
        }
    }
    return float4(rgb, a);
}

float4 PSMain(VSOut i) : SV_Target
{
    float2 p = i.pos.xy;
    float u, s, W, H;
    edge(p, u, s, W, H);
    float sc = misc.x, P = misc.y;
    // Tief im Fenster leuchtet nichts mehr: dort sofort durchsichtig, das spart den
    // weitaus groessten Teil der Pixel.
    if (u > 24 * sc)
        return float4(0, 0, 0, 0);
    int design = (int)misc.z;
    float4 c;
    if (design == 0) c = kometen(u, s, P, sc);
    else if (design == 1) c = glut(p, u, s, P, sc);
    else if (design == 2) c = hochspannung(u, s, P, sc);
    else c = lauflicht(u, s, P, sc);
    // Leichtes Rauschen gegen Stufen in den dunklen Auslaeufen des Hofs.
    float n = (hash21(p + frac(misc.w)) - 0.5) / 255.0;
    float a = saturate(c.a + n);
    float3 rgb = min(saturate(c.rgb + n), a);
    return float4(rgb, a);
}
