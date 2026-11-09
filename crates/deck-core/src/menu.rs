//! Die Einträge der nativen Menüs des Panels (`deck/ui/connect.py`) als Daten: jeder
//! Eintrag trägt seine Wirkung in der Menü-ID (`model|B3|opus[1m]`), denn Tauri meldet
//! beim Klick nur diese ID zurück. Gebaut und ausgeführt werden die Menüs in der App.

/// Was ein Eintrag auslöst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Model(String, String),
    AssignTicket(String),
    ClearTicket(String),
    Effort(String, String),
    Mode(String, String),
    Close(String),
    /// Kopf eines Blocks: Bindung des Buchstabens vergessen.
    Forget(String),
}

impl MenuAction {
    pub fn id(&self) -> String {
        match self {
            MenuAction::Model(s, v) => format!("model|{s}|{v}"),
            MenuAction::AssignTicket(s) => format!("ticket|{s}"),
            MenuAction::ClearTicket(s) => format!("unticket|{s}"),
            MenuAction::Effort(s, v) => format!("effort|{s}|{v}"),
            MenuAction::Mode(s, v) => format!("mode|{s}|{v}"),
            MenuAction::Close(s) => format!("close|{s}"),
            MenuAction::Forget(w) => format!("forget|{w}"),
        }
    }

    /// Die Wirkung aus der ID zurück. Der Wert ist das letzte Feld und darf selbst `|`
    /// enthalten; fremde IDs ergeben `None`.
    pub fn parse(id: &str) -> Option<MenuAction> {
        let mut it = id.splitn(3, '|');
        let (kind, slot) = (it.next()?, it.next()?.to_string());
        let val = it.next().map(String::from);
        Some(match (kind, val) {
            ("model", Some(v)) => MenuAction::Model(slot, v),
            ("ticket", None) => MenuAction::AssignTicket(slot),
            ("unticket", None) => MenuAction::ClearTicket(slot),
            ("effort", Some(v)) => MenuAction::Effort(slot, v),
            ("mode", Some(v)) => MenuAction::Mode(slot, v),
            ("close", None) => MenuAction::Close(slot),
            ("forget", None) => MenuAction::Forget(slot),
            _ => return None,
        })
    }
}

/// Erster Buchstabe groß, der Rest klein („auto“ → „Auto“), wie Pythons `capitalize`.
/// So stehen die Modi im Menü.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_carry_their_action_and_survive_brackets_in_values() {
        for a in [
            MenuAction::Model("B3".into(), "opus[1m]".into()),
            MenuAction::AssignTicket("A1".into()),
            MenuAction::ClearTicket("A1".into()),
            MenuAction::Effort("C2".into(), "Ultracode".into()),
            MenuAction::Mode("C2".into(), "bypass".into()),
            MenuAction::Close("D4".into()),
            MenuAction::Forget("B".into()),
        ] {
            assert_eq!(MenuAction::parse(&a.id()), Some(a));
        }
        assert_eq!(MenuAction::parse("m-model"), None);
        assert_eq!(MenuAction::parse("close|A1|x"), None);
    }

    #[test]
    fn mode_names_are_capitalized_like_python() {
        assert_eq!(capitalize("auto"), "Auto");
        assert_eq!(capitalize("bypass"), "Bypass");
        assert_eq!(capitalize("acceptEdits"), "Acceptedits");
        assert_eq!(capitalize(""), "");
    }
}
