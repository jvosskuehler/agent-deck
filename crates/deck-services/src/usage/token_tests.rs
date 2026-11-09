//! Tests zu `token.rs` (aus `tests/test_claude_usage.py`, Abschnitt CLI-Token).

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

fn reader(r: Result<Vec<&'static str>, &'static str>) -> Reader {
    Box::new(move || r.clone().map(|v| v.into_iter().map(String::from).collect()).map_err(String::from))
}

#[test]
fn both_token_sources_are_combined_cli_first() {
    let src = DiskTokens::with_sources(vec![("CLI", reader(Ok(vec!["cli"]))), ("Desktop", reader(Ok(vec!["desk"])))]);
    assert_eq!(src.tokens(true).unwrap(), ["cli", "desk"]);
}

#[test]
fn if_one_source_fails_the_other_carries() {
    let src = DiskTokens::with_sources(vec![("CLI", reader(Ok(vec!["cli"]))), ("Desktop", reader(Err("weg")))]);
    assert_eq!(src.tokens(true).unwrap(), ["cli"]);
}

#[test]
fn only_both_sources_failing_is_an_error_naming_both() {
    let src = DiskTokens::with_sources(vec![("CLI", reader(Err("fehlt"))), ("Desktop", reader(Err("weg")))]);
    let err = src.tokens(true).unwrap_err();
    assert!(err.contains("CLI") && err.contains("Desktop"), "{err}");
}

#[test]
fn tokens_are_read_from_disk_only_when_forced_or_unknown() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let reads = Arc::new(AtomicUsize::new(0));
    let r = reads.clone();
    let src = DiskTokens::with_sources(vec![(
        "CLI",
        Box::new(move || {
            r.fetch_add(1, Ordering::SeqCst);
            Ok(vec!["t".to_string()])
        }),
    )]);
    src.tokens(false).unwrap();
    src.tokens(false).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1, "zweiter Abruf ohne force liest nicht neu");
    src.tokens(true).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 2);
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
