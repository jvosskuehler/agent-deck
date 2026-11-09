//! Die gemerkte Lage des Panels aus `deck_settings.json`: wo der Griff entlang der Kante
//! steht (`dock_along`), wo das freie Panel lag (`panel_pos`) und wie groß es gezogen war
//! (`panel_zoom`). Kaputte Werte gelten als nicht gesetzt: lieber die Vorgabe als ein
//! Panel an einer erfundenen Stelle.

use serde_json::{Map, Value};

use crate::zoom::{ZOOM_MAX, ZOOM_MIN};

/// Gemerkte Lage entlang der Kante (`dock_along`).
pub fn saved_along(settings: &Map<String, Value>) -> Option<i32> {
    settings.get("dock_along").and_then(Value::as_f64).map(|v| v as i32)
}

/// Gemerkte Lage des freien Panels (`panel_pos`: `[x, y]`).
pub fn saved_free(settings: &Map<String, Value>) -> Option<(i32, i32)> {
    let pos = settings.get("panel_pos")?.as_array()?;
    match pos.as_slice() {
        [x, y] => Some((x.as_f64()? as i32, y.as_f64()? as i32)),
        _ => None,
    }
}

/// Gemerkter Zoom (`panel_zoom`), in seinen Grenzen; fehlt er, 1.
pub fn saved_zoom(settings: &Map<String, Value>) -> f64 {
    settings.get("panel_zoom").and_then(Value::as_f64).filter(|z| z.is_finite()).map_or(1.0, |z| z.clamp(ZOOM_MIN, ZOOM_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn the_handle_keeps_its_place_along_the_edge() {
        assert_eq!(saved_along(&settings(json!({"dock_along": 412.6}))), Some(412));
        assert_eq!(saved_along(&settings(json!({"dock_along": "mitte"}))), None);
        assert_eq!(saved_along(&settings(json!({}))), None);
    }

    #[test]
    fn the_floating_panel_comes_back_where_it_was_left() {
        assert_eq!(saved_free(&settings(json!({"panel_pos": [360, 260]}))), Some((360, 260)));
        assert_eq!(saved_free(&settings(json!({"panel_pos": [360.7, -20]}))), Some((360, -20)));
        assert_eq!(saved_free(&settings(json!({}))), None);
        assert_eq!(saved_free(&settings(json!({"panel_pos": [1]}))), None, "kaputter Wert = Vorgabe");
        assert_eq!(saved_free(&settings(json!({"panel_pos": "oben"}))), None);
    }

    #[test]
    fn a_saved_zoom_stays_within_its_bounds() {
        assert_eq!(saved_zoom(&settings(json!({"panel_zoom": 1.2}))), 1.2);
        assert_eq!(saved_zoom(&settings(json!({"panel_zoom": 9}))), ZOOM_MAX);
        assert_eq!(saved_zoom(&settings(json!({"panel_zoom": 0.1}))), ZOOM_MIN);
        assert_eq!(saved_zoom(&settings(json!({}))), 1.0);
    }
}
