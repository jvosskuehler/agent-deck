//! Wire-Vokabular des Broker-Protokolls zur VS-Code-Extension.
//!
//! Zwischen App und Extension laufen newline-getrennte JSON-Zeilen über TCP
//! 127.0.0.1:8765. Die Strings hier sind der Vertrag: wer einen ändert, ändert ihn in
//! `extension/extension.js` mit. Die Extension kann diese Datei nicht importieren, sie
//! spiegelt die Strings von Hand; hier gebündelt wird aus einem Tippfehler wenigstens
//! auf der Rust-Seite ein Übersetzungsfehler statt eines still verpuffenden Kommandos.

// ── Kommandos: App → Extension (JSON-Feld "cmd") ────────────────────────────

/// Der Extension ihren Fenster-Buchstaben zuweisen.
pub const CMD_ASSIGN: &str = "assign";
/// Buchstaben wieder vergessen (löst eine Phantomkachel).
pub const CMD_UNASSIGN: &str = "unassign";
/// Ein bestimmtes Terminal (Slot) fokussieren.
pub const CMD_FOCUS_PANE: &str = "focusPane";
/// Text oder Slash-Kommando an den Agenten. Feld `submit: true`: die Extension schreibt
/// den Text und schickt ihn per SEPARATEM Enter ab (ein langer Prompt kommt als Paste
/// an, ein mitgeschicktes `\r` würde verschluckt).
pub const CMD_SEND: &str = "send";
/// Einzelne Taste (enter/esc/shift-tab, ggf. `repeat`).
pub const CMD_KEY: &str = "key";
/// Ein weiteres Claude-Terminal öffnen. Felder: `model` (CLI `--model`), `args` (fertige
/// Flag-Zeile aus `restart_plan::flags`, hat Vorrang vor `model`), `cwd` (beim Resume
/// PFLICHT: Claude findet eine Session über ihr Projektverzeichnis) und `slot`
/// (Wunschname; belegt oder fremd → die Extension vergibt selbst).
pub const CMD_CREATE_AGENT: &str = "createAgent";
/// „Developer: Reload Window“ auslösen.
pub const CMD_RELOAD: &str = "reload";
/// Ein einzelnes Terminal/einen Agenten schließen.
pub const CMD_CLOSE_AGENT: &str = "closeAgent";
/// Das ganze VS-Code-Fenster schließen.
pub const CMD_CLOSE_WINDOW: &str = "closeWindow";
/// Alle Editoren speichern (vor dem Schließen: ein ungespeicherter Editor macht aus
/// closeWindow einen modalen Dialog, der einen automatischen Ablauf still anhält).
pub const CMD_SAVE_ALL: &str = "saveAll";

// ── Nachrichten: Extension → App (JSON-Feld "type") ─────────────────────────

/// Erstmeldung: Workspace-Name (evtl. mit window/slots). Wie `terminals` mit `folder`
/// (Ordnerpfad, bei Remote die Adresse `vscode-remote://…`, `null` ohne Ordner) und
/// `remote` (`null` lokal, sonst `ssh-remote`, `wsl`, `dev-container` …). Ohne den
/// Remote-Marker sähe eine Remote-Kachel wie eine ruhige lokale aus.
pub const TYPE_HELLO: &str = "hello";
/// Aktualisierte Terminal-/Slot-Liste des Fensters.
pub const TYPE_TERMINALS: &str = "terminals";
/// Pane in VS Code direkt fokussiert → Slot als gelesen markieren (done → idle).
pub const TYPE_SEEN: &str = "seen";

// ── Kommandos als Typ ────────────────────────────────────────────────────────

use serde_json::{Map, Value};

/// Ein Kommando an einen Agenten-Host. Die VS-Code-Seite bekommt es als JSON-Zeile
/// ([`Command::to_value`], Felder wie `net/commands.py`), der Windows-Terminal-Host
/// führt es selbst aus. So baut keine Aufrufstelle ihr Dict von Hand.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Assign { window: String },
    Unassign,
    FocusPane { slot: String },
    /// `submit`: Text schreiben und per eigenem Enter abschicken; `execute` gilt dann nicht.
    Send { slot: String, text: String, execute: bool, submit: bool },
    Key { slot: String, key: String, repeat: u32 },
    CreateAgent { model: Option<String>, args: Option<String>, cwd: Option<String>, slot: Option<String> },
    Reload,
    CloseAgent { slot: String },
    CloseWindow,
    SaveAll,
}

impl Command {
    /// Die Drahtform für die Extension.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: Value| {
            m.insert(k.to_string(), v);
        };
        match self {
            Command::Assign { window } => {
                put("cmd", CMD_ASSIGN.into());
                put("window", window.as_str().into());
            }
            Command::Unassign => put("cmd", CMD_UNASSIGN.into()),
            Command::FocusPane { slot } => {
                put("cmd", CMD_FOCUS_PANE.into());
                put("slot", slot.as_str().into());
            }
            Command::Send { slot, text, execute, submit } => {
                put("cmd", CMD_SEND.into());
                put("slot", slot.as_str().into());
                put("text", text.as_str().into());
                put("execute", (*execute && !*submit).into());
                if *submit {
                    put("submit", true.into());
                }
            }
            Command::Key { slot, key, repeat } => {
                put("cmd", CMD_KEY.into());
                put("slot", slot.as_str().into());
                put("key", key.as_str().into());
                if *repeat > 1 {
                    put("repeat", (*repeat).into());
                }
            }
            Command::CreateAgent { model, args, cwd, slot } => {
                put("cmd", CMD_CREATE_AGENT.into());
                for (k, v) in [("model", model), ("args", args), ("cwd", cwd), ("slot", slot)] {
                    if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                        put(k, v.into());
                    }
                }
            }
            Command::Reload => put("cmd", CMD_RELOAD.into()),
            Command::CloseAgent { slot } => {
                put("cmd", CMD_CLOSE_AGENT.into());
                put("slot", slot.as_str().into());
            }
            Command::CloseWindow => put("cmd", CMD_CLOSE_WINDOW.into()),
            Command::SaveAll => put("cmd", CMD_SAVE_ALL.into()),
        }
        Value::Object(m)
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn submitted_text_never_carries_its_own_enter() {
        let c = Command::Send { slot: "B2".into(), text: "hi".into(), execute: true, submit: true };
        assert_eq!(c.to_value(), json!({"cmd": "send", "slot": "B2", "text": "hi", "execute": false, "submit": true}));
        let c = Command::Send { slot: "B2".into(), text: "/model x".into(), execute: true, submit: false };
        assert_eq!(c.to_value(), json!({"cmd": "send", "slot": "B2", "text": "/model x", "execute": true}));
    }

    #[test]
    fn single_key_omits_the_repeat_field() {
        let one = Command::Key { slot: "A1".into(), key: "esc".into(), repeat: 1 };
        assert_eq!(one.to_value(), json!({"cmd": "key", "slot": "A1", "key": "esc"}));
        let two = Command::Key { slot: "A1".into(), key: "shift-tab".into(), repeat: 2 };
        assert_eq!(two.to_value()["repeat"], json!(2));
    }

    #[test]
    fn create_agent_sends_only_the_fields_it_has() {
        let c = Command::CreateAgent { model: Some("opus[1m]".into()), args: None, cwd: Some(String::new()), slot: None };
        assert_eq!(c.to_value(), json!({"cmd": "createAgent", "model": "opus[1m]"}));
    }
}
