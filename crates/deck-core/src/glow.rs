//! Der eigene Glow des Fokus-Rahmens: je eine Einstellung für helles und dunkles Terminal,
//! gewählt im Fenster „Glow einstellen“ (Strg+Alt+G) und gespeichert in
//! `deck_settings.json` unter `glow`.
//!
//! Eine Einstellung ist eine Glow-Art ([`GlowKind`]: wie das Licht um das Fenster liegt),
//! eine Animation ([`GlowAnim`]) und die Regler dazu. Gezeichnet wird in
//! `frame_eigen.hlsl`: vier weiche Schichten und ein langer Auslauf, der 60 Punkte vor dem
//! Fenster sicher bei null ist; dunkel leuchtet der Glow fast additiv (Licht, das nichts
//! zudeckt), hell liegt er als farbiger Schatten auf dem Papier (Canvas „Fokus-Rahmen
//! Glow und Animation“, 12.11.2026). Was hier steht, wird zu vier Vektoren für den Shader
//! ([`GlowStyle::shader_params`]).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

/// Schlüssel in `deck_settings.json`.
pub const SETTING: &str = "glow";

/// Wie das Licht um das Fenster liegt. Die Reihenfolge ist die Nummer im Shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum GlowKind {
    /// Vier weiche Schichten, überall gleich.
    #[default]
    Weichlicht,
    /// Ungleich entlang der Kante, große ruhige Zungen.
    Glutsaum,
    /// Zwei Farben gegenüber: Farbe oben links, zweite Farbe unten rechts.
    Zwielicht,
    /// Unten kräftig, an den Seiten auslaufend, oben fast nichts.
    Bodenlicht,
    /// Vier Lichter in den Ecken.
    Ecklicht,
}

/// Was sich bewegt. Die Reihenfolge ist die Nummer im Shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum GlowAnim {
    #[default]
    Still,
    /// Langsames Ein- und Ausatmen.
    Atmen,
    /// Ein heller Kamm mit langem Schweif wandert herum.
    Umlauf,
    /// Das Licht zieht ungleich an der Kante entlang.
    Stroemung,
    /// Farbe und zweite Farbe fließen um das Fenster.
    Farbwandern,
    /// Kleine Lichtpunkte lösen sich von der Kante und verlöschen.
    Funken,
}

/// Ein Glow, so wie das Fenster ihn zeigt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GlowStyle {
    pub kind: GlowKind,
    pub animation: GlowAnim,
    /// Hauptfarbe, `#RRGGBB`.
    pub color: String,
    /// Zweite Farbe: in sie läuft der Schein nach außen aus, bei Zwielicht und Farbwandern
    /// ist sie das Gegenstück. Leer = die Hauptfarbe.
    pub outer: String,
    /// Stärke, 0 bis 2.
    pub intensity: f64,
    /// Reichweite des Scheins, 0,3 bis 1,6.
    pub width: f64,
    /// Tempo der Animation, 0,25 bis 2 (1 = wie entworfen).
    pub speed: f64,
}

/// Ein gemerkter Glow: zum Ausprobieren und später Zurückholen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SavedGlow {
    pub name: String,
    pub style: GlowStyle,
}

/// So viele gemerkte Glows je Seite; ältere fallen hinten weg.
pub const SAVED_MAX: usize = 24;

/// Beide Glows und die gemerkten je Seite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GlowSettings {
    pub light: GlowStyle,
    pub dark: GlowStyle,
    /// Gemerkte Glows für hell, neueste zuerst.
    pub saved_light: Vec<SavedGlow>,
    /// Gemerkte Glows für dunkel, neueste zuerst.
    pub saved_dark: Vec<SavedGlow>,
}

/// Inhalt des Fensters „Glow einstellen“.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GlowView {
    pub glow: GlowSettings,
    /// Ist das System gerade dunkel? Die passende Spalte wird hervorgehoben.
    pub night: bool,
    /// Oberflächensprache, „de“ oder „en“.
    pub lang: String,
}

impl Default for GlowStyle {
    fn default() -> Self {
        GlowStyle::dark_default()
    }
}

impl Default for GlowSettings {
    fn default() -> Self {
        GlowSettings {
            light: GlowStyle::light_default(),
            dark: GlowStyle::dark_default(),
            saved_light: Vec::new(),
            saved_dark: Vec::new(),
        }
    }
}

impl GlowStyle {
    /// Weichlicht in Honiggold, nach außen ins Orange.
    pub fn dark_default() -> GlowStyle {
        GlowStyle {
            kind: GlowKind::Weichlicht,
            animation: GlowAnim::Still,
            color: "#F5B847".into(),
            outer: "#F08A3C".into(),
            intensity: 1.0,
            width: 1.0,
            speed: 1.0,
        }
    }

    /// Weichlicht in Terrakotta.
    pub fn light_default() -> GlowStyle {
        GlowStyle { color: "#D9775A".into(), outer: String::new(), intensity: 1.2, ..GlowStyle::dark_default() }
    }

    /// Werte in ihre Grenzen holen; eine unlesbare Farbe wird die der Vorgabe.
    pub fn clamped(&self, fallback: &GlowStyle) -> GlowStyle {
        let color = parse_hex(&self.color).map_or_else(|| fallback.color.clone(), hex);
        let outer = if self.outer.trim().is_empty() { String::new() } else { parse_hex(&self.outer).map_or_else(String::new, hex) };
        let num = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        GlowStyle {
            kind: self.kind,
            animation: self.animation,
            color,
            outer,
            intensity: num(self.intensity, 0.0, 2.0, fallback.intensity),
            width: num(self.width, 0.3, 1.6, fallback.width),
            speed: num(self.speed, 0.25, 2.0, fallback.speed),
        }
    }

    /// Die Vektoren für den Shader (Design `Eigen`): `a` = Farbe und Stärke, `b` =
    /// Reichweite, Glow-Art, Animation (b.w bleibt hell/dunkel), `items[0]` = zweite Farbe
    /// und Tempo; `items[1]` ist frei.
    pub fn shader_params(&self) -> [[f32; 4]; 4] {
        let c = parse_hex(&self.color).unwrap_or([0.85, 0.47, 0.35]);
        let o = parse_hex(&self.outer).unwrap_or(c);
        let f = |v: f64| v as f32;
        [
            [c[0], c[1], c[2], f(self.intensity)],
            [f(self.width), f32::from(self.kind as u8), f32::from(self.animation as u8), 0.0],
            [o[0], o[1], o[2], f(self.speed)],
            [0.0; 4],
        ]
    }
}

impl GlowSettings {
    /// Aus `deck_settings.json`; fehlt der Eintrag oder ein Teil, gilt die Vorgabe.
    pub fn from_settings(settings: &Map<String, Value>) -> GlowSettings {
        let raw: GlowSettings = settings.get(SETTING).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        // Fehlt `light` ganz, füllt serde es mit GlowStyle::default() (= dunkel): darum nachsehen.
        let has_light = settings.get(SETTING).and_then(|v| v.get("light")).is_some();
        let light = if has_light { raw.light } else { GlowStyle::light_default() };
        let keep = |list: Vec<SavedGlow>, fallback: &GlowStyle| -> Vec<SavedGlow> {
            list.into_iter()
                .take(SAVED_MAX)
                .map(|s| SavedGlow { name: s.name.trim().chars().take(60).collect(), style: s.style.clamped(fallback) })
                .collect()
        };
        GlowSettings {
            light: light.clamped(&GlowStyle::light_default()),
            dark: raw.dark.clamped(&GlowStyle::dark_default()),
            saved_light: keep(raw.saved_light, &GlowStyle::light_default()),
            saved_dark: keep(raw.saved_dark, &GlowStyle::dark_default()),
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn for_night(&self, night: bool) -> &GlowStyle {
        if night { &self.dark } else { &self.light }
    }
}

/// `#RRGGBB` (auch ohne `#`) → Farbe 0 bis 1.
pub fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|b| f32::from(b) / 255.0);
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn hex(c: [f32; 3]) -> String {
    let b = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn without_a_saved_glow_both_sides_are_weichlicht_gold_and_terrakotta() {
        let g = GlowSettings::from_settings(&Map::new());
        assert_eq!(g.dark, GlowStyle::dark_default());
        assert_eq!(g.light.color, "#D9775A");
        assert_eq!((g.dark.kind, g.dark.animation), (GlowKind::Weichlicht, GlowAnim::Still));
        let p = g.dark.shader_params();
        assert!((p[0][0] - 0.96).abs() < 0.005 && (p[0][1] - 0.72).abs() < 0.005, "Honiggold");
        assert_eq!(p[1], [1.0, 0.0, 0.0, 0.0], "Reichweite 1, Weichlicht, still");
        assert!((p[2][3] - 1.0).abs() < 1e-6, "Tempo 1");
    }

    #[test]
    fn kind_and_animation_reach_the_shader_as_their_numbers() {
        let s = json!({"glow": {"dark": {"kind": "ecklicht", "animation": "funken", "speed": 1.5}}});
        let g = GlowSettings::from_settings(s.as_object().unwrap());
        assert_eq!((g.dark.kind, g.dark.animation), (GlowKind::Ecklicht, GlowAnim::Funken));
        let p = g.dark.shader_params();
        assert_eq!((p[1][1], p[1][2], p[2][3]), (4.0, 5.0, 1.5), "Nummern wie in frame_eigen.hlsl");
        assert_eq!(serde_json::to_value(GlowAnim::Stroemung).unwrap(), json!("stroemung"));
    }

    #[test]
    fn a_glow_saved_before_kinds_existed_keeps_its_color_and_strength() {
        let s = json!({"glow": {"dark": {"color": "#DB6E40", "outer": "#94331C", "intensity": 0.4, "width": 0.5, "edge": 1.0, "life": 1.0, "breathe": 0.3, "tempo": 8.0}}});
        let g = GlowSettings::from_settings(s.as_object().unwrap());
        assert_eq!((g.dark.color.as_str(), g.dark.intensity, g.dark.width), ("#DB6E40", 0.4, 0.5));
        assert_eq!((g.dark.kind, g.dark.animation, g.dark.speed), (GlowKind::Weichlicht, GlowAnim::Still, 1.0));
    }

    #[test]
    fn a_saved_glow_is_read_and_out_of_range_values_are_clamped() {
        let s = json!({"glow": {"light": {"color": "#00ff00", "intensity": 9.0, "speed": 0.0}}});
        let g = GlowSettings::from_settings(s.as_object().unwrap());
        assert_eq!(g.light.color, "#00FF00");
        assert_eq!(g.light.intensity, 2.0);
        assert_eq!(g.light.speed, 0.25);
        assert_eq!(g.dark, GlowStyle::dark_default(), "fehlendes Dunkel bleibt Vorgabe");
    }

    #[test]
    fn a_broken_color_falls_back_and_an_empty_outer_is_the_main_color() {
        let s = json!({"glow": {"light": {"color": "lila", "outer": ""}}});
        let g = GlowSettings::from_settings(s.as_object().unwrap());
        assert_eq!(g.light.color, "#D9775A");
        let p = g.light.shader_params();
        assert_eq!(&p[2][..3], &p[0][..3]);
    }

    #[test]
    fn saving_and_reading_gives_the_same_glow() {
        let mut g = GlowSettings::default();
        g.light.color = "#286983".into();
        g.dark.kind = GlowKind::Zwielicht;
        g.dark.animation = GlowAnim::Farbwandern;
        let mut m = Map::new();
        m.insert(SETTING.into(), g.to_value());
        assert_eq!(GlowSettings::from_settings(&m), g);
    }

    #[test]
    fn saved_glows_survive_saving_and_are_clamped_and_capped() {
        let mut g = GlowSettings::default();
        let mut wild = GlowStyle::dark_default();
        wild.kind = GlowKind::Ecklicht;
        wild.intensity = 7.0;
        g.saved_dark = (0..SAVED_MAX + 3).map(|i| SavedGlow { name: format!("  Probe {i} "), style: wild.clone() }).collect();
        let mut m = Map::new();
        m.insert(SETTING.into(), g.to_value());
        let back = GlowSettings::from_settings(&m);
        assert_eq!(back.saved_dark.len(), SAVED_MAX);
        assert_eq!(back.saved_dark[0].name, "Probe 0");
        assert_eq!((back.saved_dark[0].style.kind, back.saved_dark[0].style.intensity), (GlowKind::Ecklicht, 2.0));
        assert!(back.saved_light.is_empty());
    }

    #[test]
    fn a_glow_saved_before_the_saved_list_existed_has_an_empty_list() {
        let s = json!({"glow": {"dark": {"color": "#DB6E40"}}});
        let g = GlowSettings::from_settings(s.as_object().unwrap());
        assert!(g.saved_light.is_empty() && g.saved_dark.is_empty());
    }

    #[test]
    fn hex_parsing_accepts_six_digits_with_or_without_hash() {
        assert_eq!(parse_hex("#FF0000"), Some([1.0, 0.0, 0.0]));
        assert_eq!(parse_hex("00ff00"), Some([0.0, 1.0, 0.0]));
        assert_eq!(parse_hex("#F00"), None);
        assert_eq!(parse_hex("#GG0000"), None);
    }
}
