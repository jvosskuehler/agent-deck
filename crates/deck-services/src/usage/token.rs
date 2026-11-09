//! Woher das OAuth-Token für den Usage-Endpoint kommt (Python `claude/usage_token.py`).
//!
//! Zwei Quellen, CLI zuerst:
//! 1. Claude Code CLI: `~/.claude/.credentials.json`, Klartext-JSON. Die Quelle, die
//!    jeder Deck-Nutzer hat (ohne sie gäbe es keine Agenten).
//! 2. Claude Desktop: dessen `config.json`, verschlüsselt. Der Blob ist ein Chromium-
//!    „v10“-Paket: der AES-256-Schlüssel steckt DPAPI-geschützt in `Local State`
//!    (`os_crypt.encrypted_key`, Base64, fünf Bytes „DPAPI“ davor), das Token selbst ist
//!    AES-256-GCM (`v10` + 12 Bytes Nonce + Chiffrat + 16 Bytes Tag).
//!
//! Warum beide: die Tokens haben eigene Laufzeiten und eigene Budgets. Fällt eins aus
//! oder wird abgewiesen, trägt das andere, ohne dass der Nutzer etwas merkt. Desktop
//! aber nur für DASSELBE Konto wie die CLI (`account.rs`, seit 2026-11-08).
//!
//! Gelesen wird nicht bei jedem Abruf, sondern erst neu, wenn sich `.credentials.json`
//! ändert (Login, Token-Erneuerung: ein Kontowechsel ist so beim nächsten Blick da, nicht
//! erst, wenn das alte Token nach Stunden abgelehnt wird) oder die API ein Token abweist.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::Value;

use super::account::{TokenAccounts, select_tokens};
use super::fetch::UsageHttp;
use crate::fsutil::{now_unix, read_shared};

const CLI_TOKEN_KEYS: [&str; 3] = ["accessToken", "access_token", "token"];
const CLI_EXPIRY_KEYS: [&str; 2] = ["expiresAt", "expires_at"];

/// `.credentials.json` der CLI. `CLAUDE_CONFIG_DIR` verlegt den Ordner; wer das gesetzt
/// hat, soll nicht durchs Raster fallen.
pub fn cli_credentials_path() -> PathBuf {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| deck_core::paths::home_dir().join(".claude"));
    base.join(".credentials.json")
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.filter(|v| v.is_number()).and_then(Value::as_f64)
}

/// Geparste `.credentials.json` → noch gültige Tokens. Nachsichtig gegenüber dem Aufbau:
/// das Format gehört der CLI und ist nicht dokumentiert, ein Umbau dort darf die Anzeige
/// höchstens verstummen lassen. Bekannter Container zuerst, sonst das Objekt selbst,
/// mehrere Schreibweisen je Feld; der erste Treffer gewinnt. Fehlt die Ablaufzeit, gilt
/// das Token als gültig (ein totes kostet nur einen 401).
pub fn tokens_from_credentials(data: &Value, now_ms: f64) -> Vec<String> {
    let Some(obj) = data.as_object() else { return Vec::new() };
    let blocks = [obj.get("claudeAiOauth"), obj.get("claude_ai_oauth"), Some(data)];
    for blk in blocks.into_iter().flatten() {
        let Some(blk) = blk.as_object() else { continue };
        let tok = CLI_TOKEN_KEYS
            .iter()
            .find_map(|k| blk.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()));
        let Some(tok) = tok else { continue };
        let exp = CLI_EXPIRY_KEYS.iter().find_map(|k| num(blk.get(*k)));
        if exp.is_some_and(|e| e <= now_ms) {
            return Vec::new(); // abgelaufen: gar nicht erst senden
        }
        return vec![tok.to_string()];
    }
    Vec::new()
}

/// Mögliche Ordner von Claude Desktop. Es ist eine MSIX-App, AppData ist also
/// virtualisiert; je nach Start-Kontext ist nur EINER dieser Pfade sichtbar.
pub fn desktop_candidate_dirs(local: Option<&Path>, roaming: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = local
        && let Ok(rd) = std::fs::read_dir(local.join("Packages"))
    {
        let mut found: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("Claude_"))
            .map(|e| e.path().join("LocalCache").join("Roaming").join("Claude"))
            .filter(|p| p.is_dir())
            .collect();
        found.sort();
        dirs.extend(found);
    }
    if let Some(roaming) = roaming {
        dirs.push(roaming.join("Claude"));
    }
    dirs
}

/// Der Ordner, in dem wirklich eine `config.json` liegt (sonst der erste Kandidat).
pub fn desktop_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|d| d.join("config.json").is_file()).or(candidates.first()).cloned()
}

/// Chromium-„v10“-Blob entschlüsseln.
pub fn decrypt_v10(blob: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 3 + 12 + 16 || &blob[..3] != b"v10" {
        return Err("Unerwartetes Token-Format (App-Bound?)".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Schluessel hat nicht 32 Bytes".to_string())?;
    cipher.decrypt(Nonce::from_slice(&blob[3..15]), &blob[15..]).map_err(|_| "AES-GCM: Entschluesseln fehlgeschlagen".into())
}

/// DPAPI-Entschlüsselung (echt: `deck_win::dpapi::unprotect`).
pub type Unprotect<'a> = &'a dyn Fn(&[u8]) -> Result<Vec<u8>, String>;

/// Tokens aus den beiden Dateien von Claude Desktop, langlebigstes zuerst. Rein bis auf
/// `unprotect` (DPAPI), das hereinkommt: so ist die Kette ohne Windows-Konto prüfbar.
pub fn desktop_tokens(
    config_json: &[u8],
    local_state_json: &[u8],
    unprotect: Unprotect,
    now_ms: f64,
) -> Result<Vec<String>, String> {
    let config: Value = serde_json::from_slice(config_json).map_err(|_| "config.json ist kein JSON")?;
    let blob = config
        .get("oauth:tokenCache")
        .and_then(Value::as_str)
        .ok_or("kein oauth:tokenCache")
        .and_then(|s| B64.decode(s).map_err(|_| "tokenCache ist kein Base64"))?;
    if !blob.starts_with(b"v10") {
        return Err("Unerwartetes Token-Format (App-Bound?)".into());
    }
    let state: Value = serde_json::from_slice(local_state_json).map_err(|_| "Local State ist kein JSON")?;
    let enc = state
        .pointer("/os_crypt/encrypted_key")
        .and_then(Value::as_str)
        .ok_or("kein os_crypt.encrypted_key")
        .and_then(|s| B64.decode(s).map_err(|_| "encrypted_key ist kein Base64"))?;
    let key = unprotect(enc.get(5..).unwrap_or_default())?;
    let cache: Value = serde_json::from_slice(&decrypt_v10(&blob, &key)?).map_err(|_| "Token-Cache ist kein JSON")?;
    let entries = cache.as_object().cloned().unwrap_or_default();
    let mut tokens: Vec<(f64, String)> = entries
        .values()
        .filter_map(|e| {
            let tok = e.get("token").and_then(Value::as_str).filter(|t| !t.is_empty())?;
            let exp = num(e.get("expiresAt")).unwrap_or(0.0);
            (exp > now_ms).then(|| (exp, tok.to_string()))
        })
        .collect();
    if tokens.is_empty() {
        // Zwei Fälle, die man auseinanderhalten können muss: beim KONTOWECHSEL leert
        // Claude Desktop den Cache, beim bloßen Ablaufen stehen die Einträge noch drin.
        return Err(if entries.is_empty() { "kein Token hinterlegt" } else { "Token abgelaufen" }.into());
    }
    tokens.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(tokens.into_iter().map(|(_, t)| t).collect())
}

/// (Änderungszeit in ns, Größe) der `.credentials.json`. Die CLI schreibt die Datei bei
/// jedem Login und jeder Token-Erneuerung neu; eine geänderte Signatur ist darum das
/// Signal für einen möglichen Kontowechsel (kostet ein `stat`).
pub type CredSig = (u128, u64);

/// Liefert die Tokens des aktuellen Kontos, CLI zuerst.
pub trait TokenSource: Send + Sync {
    /// `force` liest neu statt aus dem Zwischenspeicher. `Err` nur, wenn KEINE Quelle
    /// etwas hergibt; der Text nennt beide.
    fn tokens(&self, force: bool) -> Result<Vec<String>, String>;
    /// Konto der zuletzt gelesenen Tokens (E-Mail, klein; `None` = unbekannt), ohne neu
    /// zu lesen.
    fn account(&self) -> Option<String>;
    /// Konto, für das jetzt abgefragt würde (Python `current_account`). Kostet im
    /// Normalfall nur den Blick auf die Signatur; nach einem Wechsel wird neu gelesen.
    fn current_account(&self) -> Option<String> {
        let _ = self.tokens(false);
        self.account()
    }
}

type Reader = Box<dyn Fn() -> Result<Vec<String>, String> + Send + Sync>;

/// Alles, was [`DiskTokens`] außerhalb des Prozesses liest. Tests geben Attrappen.
pub struct TokenFiles {
    /// Tokens der CLI (`.credentials.json`).
    pub cli: Reader,
    /// Tokens von Claude Desktop.
    pub desktop: Reader,
    /// Signatur der `.credentials.json`, `None` ohne Datei.
    pub signature: Box<dyn Fn() -> Option<CredSig> + Send + Sync>,
    /// Konto der CLI laut `~/.claude.json` (Rückfall, wenn das Profil nicht antwortet).
    pub cli_account: Box<dyn Fn() -> Option<String> + Send + Sync>,
}

impl TokenFiles {
    /// Die echten Dateien dieses Kontos.
    pub fn system() -> TokenFiles {
        TokenFiles {
            cli: Box::new(read_cli_tokens),
            desktop: Box::new(read_desktop_tokens),
            signature: Box::new(credentials_signature),
            cli_account: Box::new(super::account::read_cli_account),
        }
    }
}

/// Was zuletzt gelesen wurde. `sig` ist `None`, solange noch nie gelesen wurde
/// (Pythons `"unset"`), und `Some(None)`, wenn es damals keine Datei gab.
#[derive(Default)]
struct Held {
    tokens: Vec<String>,
    sig: Option<Option<CredSig>>,
    account: Option<String>,
}

/// Die echte Quelle: liest die Dateien und hält die Tokens, bis sich
/// `.credentials.json` ändert oder die API ein Token abweist.
pub struct DiskTokens {
    files: TokenFiles,
    accounts: TokenAccounts,
    held: Mutex<Held>,
}

impl DiskTokens {
    pub fn new(files: TokenFiles, accounts: TokenAccounts) -> DiskTokens {
        DiskTokens { files, accounts, held: Mutex::new(Held::default()) }
    }

    /// CLI-Datei und Claude Desktop dieses Kontos; das Konto je Token über `http`.
    pub fn system(http: Arc<dyn UsageHttp>) -> DiskTokens {
        DiskTokens::new(TokenFiles::system(), TokenAccounts::new(http))
    }
}

impl TokenSource for DiskTokens {
    fn tokens(&self, force: bool) -> Result<Vec<String>, String> {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let sig = (self.files.signature)();
        if !force && !held.tokens.is_empty() && held.sig == Some(sig) {
            return Ok(held.tokens.clone());
        }
        let mut errors = Vec::new();
        let cli = match (self.files.cli)() {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("CLI: {e}"));
                // Die Datei ist da, aber gerade nicht lesbar (die CLI schreibt sie
                // vermutlich in diesem Moment). Nicht auf Desktop ausweichen, das kann
                // ein anderes Konto sein: alten Stand behalten, beim nächsten Mal neu
                // (die Signatur bleibt dafür die alte).
                if sig.is_some() && !held.tokens.is_empty() {
                    return Ok(held.tokens.clone());
                }
                Vec::new()
            }
        };
        let desk = (self.files.desktop)().unwrap_or_else(|e| {
            errors.push(format!("Desktop: {e}"));
            Vec::new()
        });
        let account_of = |t: &str| self.accounts.account_of(t);
        let cli_acc = cli.first().and_then(|t| account_of(t).or_else(|| (self.files.cli_account)()));
        let (tokens, account) = select_tokens(&cli, cli_acc.as_deref(), &desk, &account_of);
        if tokens.is_empty() {
            return Err(if errors.is_empty() { "kein Token gefunden".into() } else { errors.join("; ") });
        }
        *held = Held { tokens: tokens.clone(), sig: Some(sig), account };
        Ok(tokens)
    }

    fn account(&self) -> Option<String> {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).account.clone()
    }
}

/// Signatur der echten `.credentials.json`.
fn credentials_signature() -> Option<CredSig> {
    let meta = std::fs::metadata(cli_credentials_path()).ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    Some((mtime, meta.len()))
}

const RETRIES: u32 = 5;
const PAUSE: Duration = Duration::from_millis(200);

fn read_cli_tokens() -> Result<Vec<String>, String> {
    let raw = read_shared(&cli_credentials_path(), RETRIES, PAUSE).map_err(|e| format!("{:?}", e.kind()))?;
    let data: Value = serde_json::from_slice(&raw).map_err(|_| "kein JSON".to_string())?;
    Ok(tokens_from_credentials(&data, now_unix() * 1000.0))
}

fn read_desktop_tokens() -> Result<Vec<String>, String> {
    let env_dir = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let cands = desktop_candidate_dirs(env_dir("LOCALAPPDATA").as_deref(), env_dir("APPDATA").as_deref());
    let dir = desktop_dir(&cands).ok_or("Claude-Desktop-Ordner nicht gefunden")?;
    let read = |name: &str| read_shared(&dir.join(name), RETRIES, PAUSE).map_err(|e| format!("{name}: {:?}", e.kind()));
    let unprotect = |d: &[u8]| deck_win::dpapi::unprotect(d).map_err(|e| e.to_string());
    desktop_tokens(&read("config.json")?, &read("Local State")?, &unprotect, now_unix() * 1000.0)
}

#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
