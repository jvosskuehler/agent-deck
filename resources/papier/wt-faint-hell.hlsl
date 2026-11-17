// Agent Deck: heller Vorschlagstext in Claude Code (Windows Terminal, Profil "Agent Deck").
//
// Claude zeichnet Eingabevorschläge mit SGR 2 ("faint") ohne eigene Farbe. Windows
// Terminal setzt faint um, indem es die Schriftfarbe halbiert: im hellen Schema
// papier-ebenen wird #575279 zu #2B293C, also fast schwarz statt blasser.
// Dieser Shader ersetzt genau diese Farbe durch ein helles Grau. Im dunklen Schema
// ergibt faint #73706B und bleibt unberührt.
//
// Die Textur enthält nur die Schrift, vormultipliziert auf transparentem Grund
// (Papier hat Alpha 0, Background im cbuffer ist leer); WT legt den Grund danach drunter.
Texture2D shaderTexture;
SamplerState samplerState;
cbuffer PixelShaderSettings { float Time; float Scale; float2 Resolution; float4 Background; };

static const float3 FAINT  = float3(0x2B, 0x29, 0x3C) / 255.0;  // #575279 halbiert
static const float3 TARGET = float3(0xA6, 0xA1, 0xB4) / 255.0;  // heller als brightBlack #797593

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET
{
    float4 c = shaderTexture.Sample(samplerState, tex);
    if (c.a < 0.02) return c;
    float3 col = c.rgb / c.a;
    if (distance(col, FAINT) > 0.04) return c;
    return float4(TARGET * c.a, c.a);
}
