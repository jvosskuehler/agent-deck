//! Zu welchem Konto ein Token gehört (Python `claude/usage_token.py`, Abschnitt
//! „Kontowechsel erkennen“, Spiegelbild von `claude-usage-shared/usage_poller.py`).
//!
//! Vorgeschichte (2026-11-08): Jorrit hat zwei Claude-Konten. Nach `/login` auf das
//! zweite zeigten Tray und Deck stundenlang weiter die Zahlen des ERSTEN. Drei Gründe:
//! 1. Das Token blieb im Speicher, bis die API es abwies. Das alte Token ist nach einem
//!    Kontowechsel aber noch Stunden gültig und liefert brav 200.
//! 2. Der gemeinsame Cache wusste nicht, zu welchem Konto seine Zahl gehört.
//! 3. Bei 429 auf dem CLI-Token sprang der Abruf auf das Desktop-Token, und Claude
//!    Desktop kann mit einem ANDEREN Konto angemeldet sein.
//!
//! Hier steht Grund 3 und die Frage „wem gehört dieses Token?“ (`api/oauth/profile`,
//! einmal je Token). Grund 1 löst `token.rs`, Grund 2 `shared.rs`.
//!
//! Das Konto ist die E-Mail, getrimmt und klein, genau wie in Python: der gemeinsame
//! Cache vergleicht es zeichengenau, und der Tray verwirft sonst bei jedem Blick einen
//! Cache, den das Deck geschrieben hat.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use super::fetch::{PROFILE_URL, UsageHttp};
use crate::fsutil::read_shared;

/// E-Mail vergleichbar machen (Python `_norm_account`): getrimmt, klein, leer = `None`.
pub fn norm_account(value: Option<&Value>) -> Option<String> {
    let s = value?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_lowercase())
}

/// Konto aus der Antwort von `api/oauth/profile`: `account.email_address`, sonst
/// `account.email`. Ein leeres erstes Feld zählt wie ein fehlendes (Pythons `or`).
pub fn account_from_profile(body: &Value) -> Option<String> {
    let acc = body.get("account")?;
    let pick = |k: &str| acc.get(k).filter(|v| deck_core::pystr::truthy(v));
    norm_account(pick("email_address").or_else(|| pick("email")))
}

/// `~/.claude.json` der CLI; `CLAUDE_CONFIG_DIR` verlegt sie (dann liegt sie DARIN,
/// nicht daneben).
pub fn cli_account_path() -> PathBuf {
    cli_account_path_in(std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(), &deck_core::paths::home_dir())
}

/// Wie [`cli_account_path`], Variable und Home von außen: so ändern Tests nicht das
/// Prozess-Environment, das parallel laufende Tests mitlesen.
pub fn cli_account_path_in(config_dir: Option<&OsStr>, home: &Path) -> PathBuf {
    match config_dir.filter(|v| !v.is_empty()) {
        Some(base) => PathBuf::from(base).join(".claude.json"),
        None => home.join(".claude.json"),
    }
}

/// Konto der CLI aus der geparsten `~/.claude.json` (`oauthAccount.emailAddress`).
pub fn cli_account_from(data: &Value) -> Option<String> {
    norm_account(data.get("oauthAccount")?.get("emailAddress"))
}

/// Konto der CLI laut `~/.claude.json`. Nur Rückfall, wenn `api/oauth/profile` nicht
/// antwortet: die CLI schreibt diese Datei getrennt von `.credentials.json`, direkt
/// nach einem Login kann hier noch das alte Konto stehen.
pub fn read_cli_account() -> Option<String> {
    let raw = read_shared(&cli_account_path(), 5, Duration::from_millis(200)).ok()?;
    cli_account_from(&serde_json::from_slice(&raw).ok()?)
}

/// Welche Tokens dürfen abgefragt werden, und für welches Konto? Pur.
///
/// Die CLI bestimmt das Konto. Desktop-Tokens zählen nur, wenn sie zum selben Konto
/// gehören; sonst würde ein 429 auf dem CLI-Token still auf die Zahlen eines anderen
/// Kontos umschalten. Ist das CLI-Konto unbekannt, bleibt Desktop deshalb ganz draußen.
/// Nur ohne CLI-Login trägt Desktop allein.
pub fn select_tokens(
    cli_tokens: &[String],
    cli_acc: Option<&str>,
    desktop_tokens: &[String],
    account_of: &dyn Fn(&str) -> Option<String>,
) -> (Vec<String>, Option<String>) {
    if !cli_tokens.is_empty() {
        let mut out = cli_tokens.to_vec();
        if let Some(acc) = cli_acc {
            out.extend(desktop_tokens.iter().filter(|t| account_of(t).as_deref() == Some(acc)).cloned());
        }
        return (out, cli_acc.map(String::from));
    }
    match desktop_tokens.first() {
        Some(first) => (desktop_tokens.to_vec(), account_of(first)),
        None => (Vec::new(), None),
    }
}

/// Konto je Token, über `api/oauth/profile`. Gemerkt werden nur Erfolge: ein Fehler
/// (Netz, 429) wird beim nächsten Lesen der Tokens neu gefragt, und das ist selten
/// (nur wenn sich `.credentials.json` ändert oder ein Token abgewiesen wird).
pub struct TokenAccounts {
    http: Arc<dyn UsageHttp>,
    known: Mutex<HashMap<String, String>>,
}

impl TokenAccounts {
    pub fn new(http: Arc<dyn UsageHttp>) -> TokenAccounts {
        TokenAccounts { http, known: Mutex::new(HashMap::new()) }
    }

    /// Konto eines Tokens (E-Mail) oder `None`, wenn es sich nicht klären lässt.
    pub fn account_of(&self, token: &str) -> Option<String> {
        if let Some(acc) = self.known.lock().unwrap_or_else(|e| e.into_inner()).get(token) {
            return Some(acc.clone());
        }
        let reply = self.http.get(PROFILE_URL, token).ok().filter(|r| r.status == 200)?;
        let acc = account_from_profile(&serde_json::from_str(&reply.body).ok()?)?;
        self.known.lock().unwrap_or_else(|e| e.into_inner()).insert(token.to_string(), acc.clone());
        Some(acc)
    }
}

#[cfg(test)]
#[path = "account_tests.rs"]
pub(crate) mod tests;
