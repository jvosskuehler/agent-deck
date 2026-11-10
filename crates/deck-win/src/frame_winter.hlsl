// Die Winter-Designs des Fokus-Rahmens (Canvas "Weihnachts-Terminals", Runde 2,
// 2026-11-10): 10 Schneewehe, 11 Nordlicht, 12 Weihnachtsexpress. Wird hinter frame.hlsl
// gehaengt und nutzt dessen cbuffer, halo() und wrapd().
//
// Anders als die Stammdesigns zeichnen diese ueber das ganze Fenster: es schneit auch
// ueber dem Text (dort duenner). Gerechnet wird in Punkten (Pixel / Skalierung), so sind
// Flocken, Eiszapfen und Zug auf jedem Monitor gleich gross.
//
// Parameter: pa je Design (frame_fx_winter.rs), pb.w = 1 bei dunklem Terminal (weisse
// Flocken), 0 bei hellem (eisblaue: weisse waeren auf dem Papier unsichtbar).

static const float W_MARGIN = 36.0;   // Hof in Punkten, wie MARGIN_PT
static const float W_TAU = 6.28318;
static float gSc;                     // Skalierung, fuer geglaettete Kanten

// Deckung an einer Kante: d = Abstand nach innen in Punkten.
float cov(float d) { return saturate(d * gSc + 0.5); }
float wstroke(float u, float lo, float hi) { return cov(min(u - lo, hi - u)); }
float4 over(float4 top, float4 bot) { return top + bot * (1 - top.a); }

// Ganzzahliger Hash: derselbe Gitterpunkt ergibt denselben Wert, auf welchem Weg er auch
// erreicht wird. Der sin-Hash bricht an den Zellgrenzen, weil der Uebersetzer die
// Rechnung je Aufruf anders umstellt (gesehen 2026-11-10 als Kloetzchen im Leuchten).
uint ihash(uint x) { x ^= x >> 16; x *= 0x7feb352du; x ^= x >> 15; x *= 0x846ca68bu; x ^= x >> 16; return x; }
float whash2(float2 p)
{
    int2 i = (int2)floor(p * 8.0 + 0.5);
    return (float)(ihash((uint)i.x * 0x9E3779B9u + ihash((uint)i.y + 0x632BE5ABu)) >> 8) / 16777216.0;
}
float whash1(float x) { return whash2(float2(x, 17.0)); }
float wnoise1(float x)
{
    float i = floor(x), f = frac(x), w = f * f * (3 - 2 * f);
    return lerp(whash1(i), whash1(i + 1), w);
}
float wnoise2(float2 p)
{
    float2 i = floor(p), f = frac(p), w = f * f * (3 - 2 * f);
    return lerp(lerp(whash2(i), whash2(i + float2(1, 0)), w.x),
                lerp(whash2(i + float2(0, 1)), whash2(i + float2(1, 1)), w.x), w.y);
}
float wfbm(float2 p)
{
    float v = 0, a = 0.5;
    [unroll] for (int i = 0; i < 4; i++)
    {
        v += a * wnoise2(p);
        p = float2(0.8 * p.x - 0.6 * p.y, 0.6 * p.x + 0.8 * p.y) * 2.02;
        a *= 0.5;
    }
    return v;
}
// Der Umfang als Kreis: Rauschen entlang der Kante hat so keine Naht.
float2 ring(float s, float P, float f)
{
    float a = s / P * W_TAU;
    return float2(cos(a), sin(a)) * (P * f / W_TAU);
}

// ── Schnee ────────────────────────────────────────────────
// Eine Schicht fallender Flocken: x = Flocke, y = weicher Saum; star > 0 = sechs Arme.
float2 flakes(float2 p, float size, float rpt, float fall, float drift, float seed, float dens, float t, float star)
{
    float2 q = p / size;
    q.y -= t * fall / size;
    q.x -= (t * drift + 30.0 * sin(t * 0.23 + seed)) / size;
    float2 id = floor(q), f = frac(q) - 0.5;
    float on = 1 - smoothstep(dens * 0.85, dens, whash2(id + seed));
    if (on <= 0) return float2(0, 0);
    float k = whash2(id + seed + 2.3);
    float2 o = (float2(whash2(id + seed + 1.7), whash2(id + seed + 4.1)) - 0.5) * 0.4;
    o.x += 0.1 * sin(t * (0.6 + k) + k * 40.0);
    float r = rpt * (0.55 + 0.45 * k) / size;
    float2 v = f - o;
    float d = length(v);
    float arms = pow(abs(cos(3.0 * (atan2(v.y, v.x) + k * 6.0 + t * (k - 0.5)))), 10.0);
    float rr = r * lerp(1.0, 0.5 + 1.15 * arms, star);
    return float2(1 - smoothstep(rr * 0.4, rr, d), 1 - smoothstep(r * 0.9, r * 2.2, d)) * on;
}

// Vier Schichten; ueber dem Text duenner (inside), am Rand des Hofs laeuft es aus.
float4 snow(float2 p, float u, float t, float amount, float wind, float inside, float night)
{
    float dens = 0.36 * amount * lerp(1.0, inside, smoothstep(4.0, 60.0, u));
    if (dens < 0.004) return float4(0, 0, 0, 0);
    float2 a = flakes(p, 64.0, 4.4, 74.0, 9.0 * wind, 11.0, dens, t, 1.0);
    a = max(a, flakes(p, 48.0, 2.6, 55.0, 7.0 * wind, 23.0, dens, t, 0.0) * 0.95);
    a = max(a, flakes(p, 36.0, 1.9, 40.0, 5.0 * wind, 37.0, dens, t, 0.0) * 0.85);
    a = max(a, flakes(p, 26.0, 1.3, 28.0, 3.0 * wind, 51.0, dens, t, 0.0) * 0.7);
    float fade = smoothstep(-W_MARGIN, -W_MARGIN + 10.0, u);
    float body = a.x * fade, rim = a.y * fade * (1 - a.x) * 0.22;
    float3 fc = lerp(float3(0.56, 0.72, 0.96), float3(1, 1, 1), night);
    return float4(fc * body + lerp(fc, float3(0.5, 0.62, 0.85), night) * rim, body + rim);
}

// ── 10 Schneewehe ─────────────────────────────────────────
// g = pa.x: wie weit Schnee, Reif und Eiszapfen seit dem Fokus gewachsen sind.
float4 schneewehe(float2 p, float2 q, float u, float s, float W, float H, float P, float t, float g)
{
    float3 ICE = float3(0.42, 0.74, 1.0);
    // Licht wandert durchs Eis
    float wave = pow(0.5 + 0.5 * sin(s * 0.011 - t * 1.2), 5.0);
    float breath = 0.72 + 0.28 * sin(t * 0.8);
    float lin = wstroke(u, -0.5, 2.0);
    float hal = halo(u, -0.5, 2.0, 4.5, 0.85, 15.0, 0.42) * (breath + 0.6 * wave);
    float4 c = float4(lerp(ICE, float3(1, 1, 1), 0.5 + 0.5 * wave) * lin + ICE * hal, lin + hal);
    // Glitzern auf der Kante
    float cell = 14.0, id = floor(s / cell), fs = (frac(s / cell) - 0.5) * cell;
    float hs = whash1(id + 3.0);
    float tw = pow(max(sin(t * (1.2 + 2.5 * hs) + hs * 60.0), 0.0), 30.0) * step(0.4, whash1(id + 9.0));
    float du = u - (whash1(id + 5.0) * 5.0 - 2.0);
    float sp = exp(-(fs * fs + du * du) / 2.5) + 0.6 * exp(-abs(fs) / 3.0) * exp(-du * du / 0.5)
             + 0.6 * exp(-abs(du) / 3.0) * exp(-fs * fs / 0.5);
    c += sp * tw;
    // Reif kriecht von der Kante herein, in den Ecken am weitesten
    if (u > 0)
    {
        float cd = min(min(length(q), length(q - float2(W, 0))), min(length(q - float2(0, H)), length(q - float2(W, H))));
        float reach = (9.0 + 20.0 * exp(-cd / 70.0)) * g;
        float2 rp = ring(s, P, 0.05) + float2(u * 0.11, -u * 0.07);
        float n = wfbm(rp);
        float frost = smoothstep(0.0, 0.22, n * 1.3 - u / max(reach, 0.01)) * (0.7 + 0.3 * wnoise2(rp * 4.0));
        c = over(float4(lerp(float3(0.56, 0.78, 1.0), float3(0.92, 0.97, 1.0), n), 1) * (0.62 * frost), c);
    }
    // Schnee sammelt sich auf der Oberkante
    float tap = smoothstep(-7.0, 18.0, q.x) * smoothstep(-7.0, 18.0, W - q.x);
    float hh = g * tap * (6.0 + 11.0 * wnoise1(q.x * 0.017 + 3.1) + 4.0 * wnoise1(q.x * 0.063 + 9.2));
    float depth = q.y + hh;
    float cap = cov(depth) * saturate(2.0 - q.y) * smoothstep(0.3, 1.5, hh);
    if (cap > 0)
    {
        float3 col = lerp(float3(1, 1, 1), float3(0.74, 0.85, 0.97), smoothstep(0.0, 15.0, depth));
        col = lerp(col, float3(0.66, 0.78, 0.93), (1 - smoothstep(0.0, 1.8, depth)) * 0.6);
        float2 gi = floor(p / 3.0);
        col += pow(max(sin(t * 2.0 + whash2(gi) * 80.0), 0.0), 40.0) * step(0.8, whash2(gi + 7.0));
        c = over(float4(min(col, 1), 1) * cap, c);
    }
    // Eiszapfen wachsen von der Unterkante, an der Spitze loest sich ein Tropfen
    float yb = q.y - H;
    if (yb > -1.0)
    {
        float cw = 15.0, ci = floor(q.x / cw);
        float h1 = whash1(ci + 1.3), h2 = whash1(ci + 7.7), h3 = whash1(ci + 13.1);
        float cx = (ci + 0.5) * cw + (h2 - 0.5) * 6.0;
        float there = step(h1, 0.72) * step(5.0, cx) * step(cx, W - 5.0);
        float L = g * (7.0 + 23.0 * h3 * h3);
        float there2 = there * step(1.0, L);
        float yn = saturate(yb / max(L, 0.01));
        float hw = (2.0 + 1.6 * h3) * pow(1 - yn, 0.75);
        float ic = cov(hw - abs(q.x - cx)) * step(yb, L) * there2;
        float3 icol = lerp(float3(0.56, 0.8, 1.0), float3(1, 1, 1), 0.3 + 0.6 * (1 - smoothstep(-1.5, 0.5, q.x - cx)));
        c = over(float4(icol, 1) * (ic * 0.9), c);
        float ph = frac(t / (2.6 + 3.5 * h2) + h1 * 7.0);
        float fall = max(ph - 0.55, 0.0) / 0.45;
        float yd = L + 1.5 + fall * fall * (W_MARGIN - L);
        float rd = 1.7 * smoothstep(0.0, 0.5, ph);
        float dd = length(float2(q.x - cx, (yb - yd) * (1 - 0.35 * fall)));
        float dr = (1 - smoothstep(rd - 1.0, rd, dd)) * there2 * (1 - smoothstep(0.8, 1.0, fall));
        c = over(float4(0.78, 0.9, 1.0, 1) * (dr * 0.95), c);
    }
    return c;
}

// ── 11 Nordlicht ──────────────────────────────────────────
// Drei Lichtvorhaenge, jeder treibt auf seine Weise an der Kante entlang.
float4 nordlicht(float2 p, float u, float s, float P, float t)
{
    float3 GREEN = float3(0.2, 1.0, 0.55), TEAL = float3(0.15, 0.8, 0.95), VIOLET = float3(0.62, 0.32, 1.0);
    float d = max(-u, 0.0);
    float3 rgb = float3(0, 0, 0);
    float a = 0, glow = 0;
    [unroll] for (int i = 0; i < 3; i++)
    {
        float fi = i, dir = i == 1 ? -1.0 : 1.0;
        float sm = s + dir * t * (16.0 + 10.0 * fi);
        float band = smoothstep(0.3, 0.72, wnoise2(ring(sm, P, 0.0045 + 0.002 * fi) + 7.0 * fi + 20.0));
        float lean = sm + d * 0.7 * dir;
        float rays = 0.4 + 0.6 * (0.65 * wnoise2(ring(lean, P, 0.055) + float2(t * 0.3, 3.0 * fi) + 90.0)
                                + 0.35 * wnoise2(ring(lean, P, 0.14) + float2(-t * 0.5, 5.0 * fi) + 60.0));
        float h = 7.0 + 26.0 * wnoise2(ring(sm, P, 0.012) + float2(11.0 * fi, t * 0.16) + 40.0);
        float fall = u < 0 ? exp(-d / h) : exp(-u / 5.0);
        float I = band * rays * fall * (0.8 - 0.12 * fi);
        float hue = saturate(d / h * 0.7 + 0.22 * fi + 0.2 * sin(t * 0.3 + fi * 2.0));
        float3 col = lerp(lerp(GREEN, TEAL, smoothstep(0.0, 0.5, hue)), VIOLET, smoothstep(0.45, 1.0, hue));
        rgb += col * I;
        a += I;
        glow += band;
    }
    float lin = wstroke(u, -0.5, 1.8) * (0.45 + 0.3 * glow);
    rgb += lerp(GREEN, float3(1, 1, 1), 0.5) * lin;
    a += lin;
    // ein paar Sterne funkeln durch das Licht
    if (u < -2.0)
    {
        float2 ci = floor(p / 16.0), cf = frac(p / 16.0) - 0.5;
        float hs = whash2(ci + 3.0);
        float2 so = (float2(whash2(ci + 5.0), whash2(ci + 9.0)) - 0.5) * 0.6;
        float st = step(0.7, hs) * pow(max(sin(t * (0.8 + 2.0 * hs) + hs * 40.0), 0.0), 6.0) * exp(-dot(cf - so, cf - so) * 160.0);
        rgb += float3(0.85, 1.0, 0.95) * st * 0.9;
        a += st * 0.9;
    }
    return float4(rgb, a);
}

// ── 12 Weihnachtsexpress ──────────────────────────────────
static const float TRACK_OUT = 4.5;   // so weit ausserhalb der Kante liegt das Gleis
static const float TRACK_R = 14.0;    // mit diesem Radius nimmt es die Ecken
static const float TRAIN_Z = 1.5;     // Groesse des Zugs

// a: Weg auf dem Gleis im Uhrzeigersinn ab dem Anfang der Oberkante; b: Hoehe ueber dem
// Gleis (nach aussen); PT: Laenge einer Runde (wie track_length in frame_fx_winter.rs).
void rail(float2 q, float W, float H, out float a, out float b, out float PT)
{
    float2 lo = float2(TRACK_R - TRACK_OUT, TRACK_R - TRACK_OUT), hi = float2(W, H) - lo;
    float Lw = hi.x - lo.x, Lh = hi.y - lo.y, arc = 1.5707963 * TRACK_R;
    PT = 2 * (Lw + Lh) + 4 * arc;
    float2 c = clamp(q, lo, hi), v = q - c;
    b = length(v) - TRACK_R;
    a = 0;
    if (v.x == 0 && v.y == 0) { b = -100; return; }
    if (v.x == 0 && v.y < 0) a = c.x - lo.x;
    else if (v.x > 0 && v.y < 0) a = Lw + atan2(v.x, -v.y) * TRACK_R;
    else if (v.x > 0 && v.y == 0) a = Lw + arc + (c.y - lo.y);
    else if (v.x > 0 && v.y > 0) a = Lw + arc + Lh + atan2(v.y, v.x) * TRACK_R;
    else if (v.x == 0 && v.y > 0) a = Lw + 2 * arc + Lh + (hi.x - c.x);
    else if (v.x < 0 && v.y > 0) a = 2 * Lw + 2 * arc + Lh + atan2(-v.x, v.y) * TRACK_R;
    else if (v.x < 0 && v.y == 0) a = 2 * Lw + 3 * arc + Lh + (hi.y - c.y);
    else a = 2 * Lw + 3 * arc + 2 * Lh + atan2(-v.y, -v.x) * TRACK_R;
}
float box(float x, float y, float x0, float x1, float y0, float y1) { return cov(min(min(x - x0, x1 - x), min(y - y0, y1 - y)) * TRAIN_Z); }
float disc(float x, float y, float cx, float cy, float r) { return cov((r - length(float2(x - cx, y - cy))) * TRAIN_Z); }
float4 paint(float3 col, float cover, float4 under) { return over(float4(col, 1) * cover, under); }

// Lok und vier Wagen; trail = wie frisch der Zug hier vorbeikam. head = pa.x.
float4 train(float a, float b, float PT, float t, float head, float night, out float trail)
{
    float x = -wrapd(a - head, PT);              // Weg hinter der Loknase; negativ = davor
    trail = exp(-(x < 0 ? x + PT : x) / 520.0);
    float4 c = float4(0, 0, 0, 0);
    if (x < 0 && x > -110.0)
    {
        float bb = (b - 8.0) / 10.0, beam = exp(x / 34.0) * exp(-bb * bb) * 0.6;
        c += float4(float3(1.0, 0.86, 0.5) * beam, beam);
    }
    // Dampfwoelkchen bleiben, wo der Schornstein war, und treiben davon
    float sm = 0;
    [unroll] for (int i = 0; i < 6; i++)
    {
        float age = frac(t / 0.3) * 0.3 + 0.3 * i;
        float da = wrapd(a - (head - age * 85.0 - 6.6), PT);
        float dd = length(float2(da + age * 6.0, b - (22.0 + age * 9.0)));
        float pr = 3.5 + age * 5.0;
        sm = max(sm, 0.6 * max(1 - age / 1.8, 0.0) * (1 - smoothstep(pr * 0.45, pr, dd)));
    }
    c = paint(lerp(float3(0.6, 0.65, 0.74), float3(0.9, 0.92, 0.96), night), sm, c);
    x /= TRAIN_Z;
    b /= TRAIN_Z;
    if (x < -1.0 || x > 119.5 || b < -1.0 || b > 16.0) return c;
    float k = floor(x / 24.0), lx = x - k * 24.0;
    float3 dark = float3(0.23, 0.24, 0.3), lamp = float3(1.0, 0.86, 0.45), brass = float3(0.97, 0.72, 0.24);
    float3 white = float3(0.95, 0.97, 1.0), red = float3(0.86, 0.14, 0.15);
    float4 car = float4(0, 0, 0, 0);
    if (k < 0.5)
    {
        car = paint(float3(0.08, 0.44, 0.26), box(lx, b, 1.0, 13.5, 3.0, 9.0), car);   // Kessel
        car = paint(brass, box(lx, b, 6.5, 8.0, 3.0, 9.0), car);
        car = paint(dark, box(lx, b, 2.8, 6.0, 9.0, 13.0), car);                       // Schornstein
        car = paint(brass, box(lx, b, 2.2, 6.6, 12.6, 14.0), car);
        car = paint(red, box(lx, b, 13.0, 21.0, 3.0, 12.2), car);                      // Fuehrerhaus
        car = paint(lamp, box(lx, b, 15.0, 19.0, 7.6, 10.6), car);
        car = paint(dark, box(lx, b, 12.4, 21.6, 12.2, 13.4), car);
        car = paint(white, box(lx, b, 13.0, 21.0, 13.4, 14.6), car);                   // Schnee auf dem Dach
        car = paint(float3(1.0, 0.96, 0.8), disc(lx, b, 1.2, 6.5, 1.8), car);          // Lampe
    }
    else
    {
        float3 col = k < 1.5 ? float3(0.1, 0.52, 0.28) : k < 2.5 ? float3(0.97, 0.68, 0.18) : k < 3.5 ? float3(0.2, 0.44, 0.9) : red;
        car = paint(col, box(lx, b, 1.0, 21.0, 3.0, 10.6), car);
        car = paint(lamp, max(box(lx, b, 4.0, 9.0, 5.8, 8.8), box(lx, b, 13.0, 18.0, 5.8, 8.8)), car);
        car = paint(dark, box(lx, b, 0.4, 21.6, 10.6, 11.8), car);
        car = paint(white, box(lx, b, 1.2, 20.8, 11.8, 13.2), car);                    // Schnee auf dem Dach
    }
    car = paint(dark, max(disc(lx, b, 5.0, 2.0, 2.4), disc(lx, b, 17.0, 2.0, 2.4)), car);
    car = paint(float3(0.85, 0.66, 0.3), max(disc(lx, b, 5.0, 2.0, 1.0), disc(lx, b, 17.0, 2.0, 1.0)), car);
    if (k < 3.5) car = paint(dark, box(lx, b, 21.0, 24.6, 4.0, 5.2), car);              // Kupplung
    return over(car, c);
}

float4 express(float2 q, float u, float W, float H, float t, float head, float night)
{
    float a, b, PT, trail;
    rail(q, W, H, a, b, PT);
    float4 tr = train(a, b, PT, t, head, night, trail);
    // Die Kante glueht, wo der Zug eben war, und verblasst, bis er wieder vorbeikommt.
    float3 lc = lerp(float3(0.9, 0.2, 0.16), float3(1.0, 0.74, 0.28), trail);
    float lin = wstroke(u, -0.5, 1.8), hal = halo(u, -0.5, 1.8, 5.0, 0.9, 16.0, 0.45) * (1 - lin);
    float4 c = float4(lc, 1) * ((lin + hal) * (0.25 + 0.75 * trail));
    // Schwellen und Schiene
    float am = a - 6.0 * floor(a / 6.0);
    c = paint(float3(0.52, 0.36, 0.22), cov(min(min(am, 2.2 - am), min(b + 2.6, 2.0 - b))) * 0.9, c);
    c = paint(float3(0.8, 0.82, 0.88), wstroke(b, -0.7, 0.7), c);
    return over(tr, c);
}

// Einstieg aus PSMain: p Pixel im Rahmenbild, u/s/W/H wie edge() sie liefert (Pixel).
float4 winter(int design, float2 p, float u, float s, float W, float H)
{
    gSc = misc.x;
    float2 q = (p - win.xy) / gSc;
    p /= gSc; u /= gSc; s /= gSc; W /= gSc; H /= gSc;
    if (u < -W_MARGIN) return float4(0, 0, 0, 0);
    float P = 2 * (W + H), t = misc.w, night = pb.w;
    float4 c = float4(0, 0, 0, 0);
    float amount = 1.0, wind = 1.0;
    // Tief im Fenster zeichnet nur noch der Schnee.
    if (design == 10)
    {
        if (u < 40.0) c = schneewehe(p, q, u, s, W, H, P, t, pa.x);
        amount = 1.6 * pa.y;
        wind = 3.0;
    }
    else if (design == 11)
    {
        if (u < 30.0) c = nordlicht(p, u, s, P, t) * smoothstep(-W_MARGIN, -W_MARGIN + 12.0, u);
        wind = 1.4;
    }
    else
    {
        if (u < 24.0) c = express(q, u, W, H, t, pa.x, night) * smoothstep(-W_MARGIN, -W_MARGIN + 10.0, u);
        amount = 1.3;
    }
    c.a = saturate(c.a);
    c.rgb = min(c.rgb, c.a);
    return over(snow(p, u, t, amount, wind, 0.45, night), c);
}
