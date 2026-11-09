//! Tests zu `vscode_cleanup.rs`, alle gegen Wegwerf-Ordner. Der echte Ordner des
//! Nutzers ist gesperrt (letzter Test).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;

fn ours(rel: &str) -> Value {
    json!({"identifier": {"id": EXT_ID}, "version": "0.1.1", "relativeLocation": rel,
           "location": {"$mid": 1, "path": format!("/c:/x/{rel}"), "scheme": "file"}})
}

fn foreign(id: &str) -> Value {
    json!({"identifier": {"id": id}, "version": "1.0.0", "relativeLocation": format!("{id}-1.0.0")})
}

/// Wegwerf-Extensions-Ordner mit Registratur und je einem Ordner pro Eintrag.
fn rig(entries: &[Value]) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let ext = d.path().join("extensions");
    std::fs::create_dir_all(&ext).unwrap();
    for e in entries {
        let dir = ext.join(e["relativeLocation"].as_str().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), "{}").unwrap();
    }
    std::fs::write(ext.join(REGISTRY), serde_json::to_string(entries).unwrap()).unwrap();
    (d, ext)
}

fn registry(ext: &Path) -> Vec<Value> {
    serde_json::from_slice(&std::fs::read(ext.join(REGISTRY)).unwrap()).unwrap()
}

fn backups(ext: &Path) -> usize {
    std::fs::read_dir(ext).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".bak-")).count()
}

#[test]
fn only_our_entry_and_folder_go_and_foreign_extensions_keep_their_order() {
    let (_d, ext) = rig(&[foreign("a.one"), ours(EXT_DIR), foreign("b.two")]);
    let r = remove_extension(&ext);
    assert!(r.is_ok() && r.changed, "{r:?}");
    assert_eq!(registry(&ext), [foreign("a.one"), foreign("b.two")]);
    assert!(!ext.join(EXT_DIR).exists());
    assert!(ext.join("a.one-1.0.0").is_dir() && ext.join("b.two-1.0.0").is_dir());
    assert_eq!(backups(&ext), 1, "Sicherung vor dem Schreiben");
}

#[test]
fn a_second_run_changes_nothing_and_writes_no_further_backup() {
    let (_d, ext) = rig(&[ours(EXT_DIR), foreign("a.one")]);
    remove_extension(&ext);
    let raw = std::fs::read(ext.join(REGISTRY)).unwrap();
    let again = remove_extension(&ext);
    assert!(again.is_ok() && !again.changed, "{again:?}");
    assert_eq!(std::fs::read(ext.join(REGISTRY)).unwrap(), raw);
    assert_eq!(backups(&ext), 1);
}

#[test]
fn a_renamed_folder_is_recognised_by_its_name_and_removed_too() {
    let mut renamed = ours("agent-deck-bridge.alt");
    renamed["identifier"]["id"] = "local.irgendwas".into();
    let (_d, ext) = rig(&[renamed, foreign("a.one")]);
    std::fs::create_dir_all(ext.join(EXT_DIR)).unwrap();
    remove_extension(&ext);
    assert_eq!(registry(&ext), [foreign("a.one")]);
    assert!(!ext.join("agent-deck-bridge.alt").exists() && !ext.join(EXT_DIR).exists());
}

#[test]
fn a_folder_without_entry_is_removed_and_the_registry_stays_untouched() {
    let (_d, ext) = rig(&[foreign("a.one")]);
    std::fs::create_dir_all(ext.join(EXT_DIR)).unwrap();
    let raw = std::fs::read(ext.join(REGISTRY)).unwrap();
    let r = remove_extension(&ext);
    assert!(r.changed && !ext.join(EXT_DIR).exists());
    assert_eq!(std::fs::read(ext.join(REGISTRY)).unwrap(), raw);
    assert_eq!(backups(&ext), 0);
}

#[test]
fn an_unreadable_registry_is_not_touched_and_the_folder_stays() {
    let (_d, ext) = rig(&[]);
    std::fs::write(ext.join(REGISTRY), "{kein json[").unwrap();
    std::fs::create_dir_all(ext.join(EXT_DIR)).unwrap();
    let r = remove_extension(&ext);
    assert_eq!((r.fails(), r.warns(), r.changed), (0, 1, false), "{r:?}");
    assert_eq!(std::fs::read_to_string(ext.join(REGISTRY)).unwrap(), "{kein json[");
    assert!(ext.join(EXT_DIR).is_dir());
}

#[test]
fn without_vs_code_there_is_nothing_to_do() {
    let d = tempfile::tempdir().unwrap();
    let r = remove_extension(&d.path().join("fehlt"));
    assert!(r.is_ok() && !r.changed, "{r:?}");
}

#[test]
fn the_written_registry_is_one_compact_line_with_umlauts_unescaped() {
    let mut f = foreign("a.one");
    f["location"] = json!({"path": "/c:/Users/Jörg/.vscode/extensions/a.one-1.0.0"});
    let (_d, ext) = rig(&[f, ours(EXT_DIR)]);
    remove_extension(&ext);
    let text = std::fs::read_to_string(ext.join(REGISTRY)).unwrap();
    assert!(!text.contains('\n') && !text.contains("\": ") && text.contains("Jörg"), "{text}");
}

#[test]
fn the_users_real_extensions_folder_is_locked_in_tests() {
    let r = remove_extension(&extensions_dir());
    assert!(r.lines.iter().any(|l| l.grade == Grade::Fail && l.text.contains("gesperrt")), "{r:?}");
    assert!(!r.changed);
}
