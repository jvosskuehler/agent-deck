//! Tests zu `token.rs` (aus `tests/test_claude_usage.py`, Abschnitt CLI-Token).

use std::sync::{Arc, Mutex};

use super::super::account::tests::ProfileHttp;
use super::*;
use serde_json::json;

const NOW_MS: f64 = 1_800_000_000_000.0;

/// Aufbau von `~/.claude/.credentials.json`, wie am 2026-07-29 vorgefunden. Die Werte
/// sind erfunden; geprüft wird nur, dass die richtigen FELDER gelesen werden.
fn creds(token: &str, expires_at: Option<f64>, key: &str) -> Value {
    let mut inner = json!({"accessToken": token, "refreshToken": "rt", "scopes": ["user:inference"],
                           "subscriptionType": "max", "rateLimitTier": "default"});
    if let Some(e) = expires_at {
        inner["expiresAt"] = json!(e);
    }
    json!({ key: inner })
}

#[test]
fn the_cli_token_is_read() {
    assert_eq!(tokens_from_credentials(&creds("tok-a", Some(NOW_MS + 3_600_000.0), "claudeAiOauth"), NOW_MS), ["tok-a"]);
}

#[test]
fn an_expired_cli_token_is_not_sent() {
    assert!(tokens_from_credentials(&creds("tok-alt", Some(NOW_MS - 60_000.0), "claudeAiOauth"), NOW_MS).is_empty());
}

#[test]
fn a_cli_token_without_expiry_counts_as_valid() {
    // Ein totes Token kostet nur einen 401; es wegzuwerfen wäre der teurere Fehler.
    assert_eq!(tokens_from_credentials(&creds("tok-b", None, "claudeAiOauth"), NOW_MS), ["tok-b"]);
}

#[test]
fn the_cli_token_is_found_without_the_known_container_too() {
    assert_eq!(tokens_from_credentials(&json!({"accessToken": "flach"}), NOW_MS), ["flach"]);
    assert_eq!(tokens_from_credentials(&creds("snake", None, "claude_ai_oauth"), NOW_MS), ["snake"]);
    assert_eq!(tokens_from_credentials(&json!({"claudeAiOauth": {"token": "alt"}}), NOW_MS), ["alt"]);
}

#[test]
fn garbage_credentials_give_an_empty_list() {
    for junk in [
        Value::Null,
        json!({}),
        json!([]),
        json!("text"),
        json!({"claudeAiOauth": {}}),
        json!({"claudeAiOauth": {"accessToken": ""}}),
        json!({"claudeAiOauth": null}),
    ] {
        assert!(tokens_from_credentials(&junk, NOW_MS).is_empty(), "{junk}");
    }
}

#[test]
fn the_credentials_path_follows_the_environment_variable() {
    // Nur lesend am Prozess-Environment; der Wert wird wiederhergestellt.
    let old = std::env::var_os("CLAUDE_CONFIG_DIR");
    // SAFETY: Tests dieses Moduls lesen die Variable sonst nicht.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", r"X:\woanders") };
    assert_eq!(cli_credentials_path(), PathBuf::from(r"X:\woanders\.credentials.json"));
    unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
    assert!(cli_credentials_path().ends_with(r".claude\.credentials.json"));
    if let Some(v) = old {
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) };
    }
}

/// Attrappe für alles, was `DiskTokens` außerhalb des Prozesses fragt: die Dateien der
/// beiden Quellen, ihre Signatur und das Konto je Token (über die Profil-Attrappe, sonst
/// ginge der Test ans Netz). Die Felder lassen sich zwischen zwei Abrufen umstellen.
#[derive(Clone)]
struct Disk(Arc<Mutex<DiskState>>);

struct DiskState {
    cli: Result<Vec<String>, String>,
    desk: Result<Vec<String>, String>,
    sig: Option<CredSig>,
    cli_reads: usize,
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|t| t.to_string()).collect()
}

impl Disk {
    fn new(cli: &[&str], desk: &[&str], sig: Option<CredSig>) -> Disk {
        Disk(Arc::new(Mutex::new(DiskState { cli: Ok(s(cli)), desk: Ok(s(desk)), sig, cli_reads: 0 })))
    }

    fn set(&self, f: impl FnOnce(&mut DiskState)) {
        f(&mut self.0.lock().unwrap());
    }

    fn cli_reads(&self) -> usize {
        self.0.lock().unwrap().cli_reads
    }

    /// `DiskTokens` über dieser Attrappe; `accounts` = Token → Konto laut Profil.
    fn source(&self, accounts: Vec<(&'static str, &'static str)>) -> DiskTokens {
        let (a, b, c) = (self.clone(), self.clone(), self.clone());
        let files = TokenFiles {
            cli: Box::new(move || {
                let mut st = a.0.lock().unwrap();
                st.cli_reads += 1;
                st.cli.clone()
            }),
            desktop: Box::new(move || b.0.lock().unwrap().desk.clone()),
            signature: Box::new(move || c.0.lock().unwrap().sig),
            cli_account: Box::new(|| None),
        };
        DiskTokens::new(files, TokenAccounts::new(Arc::new(ProfileHttp::new(accounts))))
    }
}

#[test]
fn both_token_sources_are_combined_cli_first() {
    // Der Kern des Rückfalls: fällt EINE Quelle aus, trägt die andere. Erst wenn beide
    // nichts liefern, ist es ein Fehler, und der nennt beide Quellen. Desktop zählt nur
    // für dasselbe Konto.
    let disk = Disk::new(&["cli"], &["desk"], None);
    let src = disk.source(vec![("cli", "a@x.de"), ("desk", "a@x.de")]);
    assert_eq!(src.tokens(true).unwrap(), ["cli", "desk"]);
    disk.set(|d| d.cli = Err("weg".into()));
    assert_eq!(src.tokens(true).unwrap(), ["desk"], "ohne Datei trägt Desktop");
    disk.set(|d| d.desk = Err("auch weg".into()));
    let err = src.tokens(true).unwrap_err();
    assert!(err.contains("CLI") && err.contains("Desktop"), "{err}");
}

#[test]
fn a_desktop_token_of_another_account_is_not_asked() {
    let disk = Disk::new(&["cli"], &["desk"], Some((1, 1)));
    let src = disk.source(vec![("cli", "b@x.de"), ("desk", "a@x.de")]);
    assert_eq!(src.tokens(false).unwrap(), ["cli"]);
    assert_eq!(src.current_account().as_deref(), Some("b@x.de"));
}

#[test]
fn an_account_switch_rereads_the_tokens_at_once() {
    // Das alte Token ist nach dem Wechsel noch Stunden gültig und liefert 200. Signal
    // ist die geänderte .credentials.json, nicht erst ein 401.
    let disk = Disk::new(&["tok-a"], &[], Some((1, 1)));
    let src = disk.source(vec![("tok-a", "a@x.de"), ("tok-b", "b@x.de")]);
    assert_eq!(src.tokens(false).unwrap(), ["tok-a"]);
    assert_eq!(src.account().as_deref(), Some("a@x.de"));
    disk.set(|d| {
        d.cli = Ok(s(&["tok-b"]));
        d.sig = Some((2, 1));
    });
    assert_eq!(src.current_account().as_deref(), Some("b@x.de"));
    assert_eq!(src.tokens(false).unwrap(), ["tok-b"]);
}

#[test]
fn without_a_change_the_files_are_not_read_again() {
    let disk = Disk::new(&["cli"], &[], Some((1, 1)));
    let src = disk.source(vec![("cli", "a@x.de")]);
    src.tokens(false).unwrap();
    src.tokens(false).unwrap();
    src.current_account();
    assert_eq!(disk.cli_reads(), 1, "ohne Änderung der Datei nicht neu lesen");
    src.tokens(true).unwrap();
    assert_eq!(disk.cli_reads(), 2, "force liest neu");
}

#[test]
fn half_written_credentials_do_not_switch_to_desktop() {
    let disk = Disk::new(&["cli"], &[], Some((1, 1)));
    let src = disk.source(vec![("cli", "a@x.de"), ("desk", "z@x.de")]);
    src.tokens(false).unwrap();
    disk.set(|d| {
        d.desk = Ok(s(&["desk"]));
        d.sig = Some((2, 1));
        d.cli = Err("kein JSON".into());
    });
    assert_eq!(src.tokens(false).unwrap(), ["cli"], "alter Stand bleibt");
    assert_eq!(src.account().as_deref(), Some("a@x.de"));
    disk.set(|d| d.cli = Ok(s(&["cli"])));
    src.tokens(false).unwrap();
    assert_eq!(disk.cli_reads(), 3, "die alte Signatur bleibt stehen, beim nächsten Blick wird neu gelesen");
}

#[test]
fn without_an_answer_from_the_profile_the_cli_account_file_decides() {
    let disk = Disk::new(&["cli"], &["desk"], Some((1, 1)));
    let (a, b, c) = (disk.clone(), disk.clone(), disk.clone());
    let files = TokenFiles {
        cli: Box::new(move || a.0.lock().unwrap().cli.clone()),
        desktop: Box::new(move || b.0.lock().unwrap().desk.clone()),
        signature: Box::new(move || c.0.lock().unwrap().sig),
        cli_account: Box::new(|| Some("a@x.de".into())),
    };
    // Das Profil kennt nur das Desktop-Token: das CLI-Konto kommt aus ~/.claude.json.
    let src = DiskTokens::new(files, TokenAccounts::new(Arc::new(ProfileHttp::new(vec![("desk", "a@x.de")]))));
    assert_eq!(src.tokens(false).unwrap(), ["cli", "desk"]);
    assert_eq!(src.account().as_deref(), Some("a@x.de"));
}

/// Ein Claude-Desktop-Paar (config.json, Local State) bauen, verschlüsselt wie Chromium.
fn desktop_files(cache: Value) -> (Vec<u8>, Vec<u8>) {
    let key = [7u8; 32];
    let nonce = [9u8; 12];
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), cache.to_string().as_bytes()).unwrap();
    let mut blob = b"v10".to_vec();
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    let config = json!({"oauth:tokenCache": B64.encode(&blob)});
    // Die Attrappe von DPAPI gibt einfach zurück, was hinter „DPAPI“ steht.
    let mut enc = b"DPAPI".to_vec();
    enc.extend_from_slice(&key);
    let state = json!({"os_crypt": {"encrypted_key": B64.encode(&enc)}});
    (config.to_string().into_bytes(), state.to_string().into_bytes())
}

fn fake_dpapi(d: &[u8]) -> Result<Vec<u8>, String> {
    Ok(d.to_vec())
}

#[test]
fn desktop_tokens_are_decrypted_and_sorted_longest_lived_first() {
    let (cfg, st) = desktop_files(json!({
        "a": {"token": "kurz", "expiresAt": NOW_MS + 1000.0},
        "b": {"token": "lang", "expiresAt": NOW_MS + 9000.0},
        "c": {"token": "tot", "expiresAt": NOW_MS - 1.0},
    }));
    assert_eq!(desktop_tokens(&cfg, &st, &fake_dpapi, NOW_MS).unwrap(), ["lang", "kurz"]);
}

#[test]
fn an_emptied_desktop_cache_is_told_apart_from_an_expired_one() {
    let (cfg, st) = desktop_files(json!({}));
    assert_eq!(desktop_tokens(&cfg, &st, &fake_dpapi, NOW_MS).unwrap_err(), "kein Token hinterlegt");
    let (cfg, st) = desktop_files(json!({"a": {"token": "x", "expiresAt": 1.0}}));
    assert_eq!(desktop_tokens(&cfg, &st, &fake_dpapi, NOW_MS).unwrap_err(), "Token abgelaufen");
}

#[test]
fn a_wrong_key_is_an_error_not_a_panic() {
    let (cfg, st) = desktop_files(json!({"a": {"token": "x", "expiresAt": NOW_MS + 1.0}}));
    let wrong = |_: &[u8]| Ok(vec![1u8; 32]);
    assert!(desktop_tokens(&cfg, &st, &wrong, NOW_MS).is_err());
    let short = |_: &[u8]| Ok(vec![1u8; 3]);
    assert!(desktop_tokens(&cfg, &st, &short, NOW_MS).is_err());
}

#[test]
fn a_blob_without_the_v10_prefix_is_refused() {
    let cfg = json!({"oauth:tokenCache": B64.encode(b"v20xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")}).to_string();
    let err = desktop_tokens(cfg.as_bytes(), b"{}", &fake_dpapi, NOW_MS).unwrap_err();
    assert!(err.contains("App-Bound"), "{err}");
}

#[test]
fn the_desktop_folder_with_a_config_wins_over_the_first_candidate() {
    let d = tempfile::tempdir().unwrap();
    let pkg = d.path().join("local").join("Packages").join("Claude_abc").join("LocalCache").join("Roaming").join("Claude");
    std::fs::create_dir_all(&pkg).unwrap();
    let roaming = d.path().join("roaming");
    std::fs::create_dir_all(roaming.join("Claude")).unwrap();
    std::fs::write(roaming.join("Claude").join("config.json"), "{}").unwrap();
    let cands = desktop_candidate_dirs(Some(&d.path().join("local")), Some(&roaming));
    assert_eq!(cands, vec![pkg.clone(), roaming.join("Claude")]);
    assert_eq!(desktop_dir(&cands), Some(roaming.join("Claude")));
    std::fs::remove_file(roaming.join("Claude").join("config.json")).unwrap();
    assert_eq!(desktop_dir(&cands), Some(pkg), "ohne config.json: erster Kandidat");
}
