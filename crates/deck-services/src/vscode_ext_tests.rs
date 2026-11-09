//! Tests zu `vscode_ext.rs` (aus `tests/test_ops_vscode_ext.py`). Zwei Dinge sind
//! festgenagelt: fremde Extensions überleben eine Reparatur (DIESELBE Datei), und eine
//! unlesbare Registratur wird nicht überschrieben. Geschrieben wird nur in Temp-Ordner.

use super::*;

const EXT: &str = r"C:\Users\test\.vscode\extensions";
const GHOST: &str = "agent-deck-bridge.testbackup"; // der Ordnername vom 2026-07-30

fn ext() -> PathBuf {
    PathBuf::from(EXT)
}

/// Ein Eintrag, wie VS Code ihn für eine lokal hineinkopierte Extension schreibt.
fn ours(rel: &str, ext_dir: &Path) -> Value {
    json!({"identifier": {"id": EXT_ID}, "version": "0.1.1", "location": location(ext_dir, rel), "relativeLocation": rel})
}

fn foreign() -> Value {
    let name = "ms-vscode.powershell-2025.4.0";
    json!({"identifier": {"id": "ms-vscode.powershell"}, "version": "2025.4.0", "location": location(&ext(), name), "relativeLocation": name})
}

/// Nur die genannten Ordner haben eine package.json.
fn exists(dirs: &'static [&'static str]) -> impl Fn(&Path) -> bool {
    move |p: &Path| dirs.iter().any(|d| p == Path::new(EXT).join(d).join("package.json"))
}

fn grades(f: &[(Grade, String)]) -> Vec<Grade> {
    f.iter().map(|(g, _)| *g).collect()
}

#[test]
fn a_correctly_registered_extension_is_ok() {
    let f = audit(Some(&[foreign(), ours(EXT_DIR, &ext())]), &ext(), &exists(&[EXT_DIR]));
    assert_eq!(grades(&f), [Grade::Ok]);
}

#[test]
fn an_entry_into_the_void_is_a_fail() {
    let f = audit(Some(&[ours(GHOST, &ext())]), &ext(), &exists(&[EXT_DIR]));
    assert!(grades(&f).contains(&Grade::Fail));
    assert!(f.iter().any(|(_, t)| t.contains("ins Leere")));
}

#[test]
fn a_folder_without_entry_is_a_fail() {
    let f = audit(Some(&[foreign()]), &ext(), &exists(&[EXT_DIR]));
    assert_eq!(grades(&f), [Grade::Fail]);
    assert!(f[0].1.contains("steht aber nicht"));
}

#[test]
fn nothing_installed_at_all_is_a_fail_too() {
    let f = audit(Some(&[]), &ext(), &exists(&[]));
    assert_eq!(grades(&f), [Grade::Fail]);
}

#[test]
fn two_entries_are_a_warning() {
    let f = audit(Some(&[ours(EXT_DIR, &ext()), ours(EXT_DIR, &ext())]), &ext(), &exists(&[EXT_DIR]));
    assert_eq!(grades(&f), [Grade::Warn]);
}

#[test]
fn location_contradicting_relative_location_is_noticed() {
    let mut e = ours(EXT_DIR, &ext());
    e["location"] = location(&ext(), GHOST);
    let f = audit(Some(&[e]), &ext(), &exists(&[EXT_DIR]));
    assert!(f.iter().any(|(g, t)| *g == Grade::Fail && t.contains("location.path")));
}

#[test]
fn an_entry_without_relative_location_is_a_fail() {
    let e = json!({"identifier": {"id": EXT_ID}});
    assert_eq!(grades(&audit(Some(&[e]), &ext(), &exists(&[EXT_DIR]))), [Grade::Fail]);
}

#[test]
fn an_unreadable_registry_is_only_a_warning() {
    assert_eq!(grades(&audit(None, &ext(), &exists(&[EXT_DIR]))), [Grade::Warn]);
}

#[test]
fn a_renamed_folder_is_recognised_by_its_name() {
    let e = json!({"identifier": {"id": "jemand.anders"}, "relativeLocation": GHOST});
    assert!(is_ours(&e));
    assert!(is_ours(&json!({"relativeLocation": "agent-deck-bridge/sub"})));
}

#[test]
fn a_foreign_extension_is_not_ours() {
    assert!(!is_ours(&foreign()));
    assert!(!is_ours(&json!({"relativeLocation": "agent-deck-bridgefake"})));
    assert!(!is_ours(&json!("text")));
}

#[test]
fn repair_bends_the_ghost_path_onto_the_real_folder() {
    let (after, notes) = repair(&[ours(GHOST, &ext())], &ext(), Some("0.1.1"));
    assert_eq!(after, [ours(EXT_DIR, &ext())]);
    assert!(notes[0].1.contains("umgebogen"));
}

#[test]
fn repair_creates_a_missing_entry() {
    let (after, notes) = repair(&[foreign()], &ext(), Some("0.1.1"));
    assert_eq!(after, [foreign(), ours(EXT_DIR, &ext())]);
    assert!(notes[0].1.contains("angelegt"));
}

#[test]
fn repair_merges_two_entries_into_one() {
    let (after, _) = repair(&[ours(GHOST, &ext()), ours(EXT_DIR, &ext())], &ext(), None);
    assert_eq!(after.iter().filter(|e| is_ours(e)).count(), 1);
}

#[test]
fn repair_leaves_foreign_extensions_in_place_and_order() {
    let other = json!({"identifier": {"id": "z.z"}, "relativeLocation": "z"});
    let (after, _) = repair(&[foreign(), ours(GHOST, &ext()), other.clone()], &ext(), None);
    assert_eq!(after[0], foreign());
    assert_eq!(after[2], other);
}

#[test]
fn repair_keeps_fields_vs_code_added_itself() {
    let mut e = ours(GHOST, &ext());
    e["metadata"] = json!({"installedTimestamp": 1});
    let (after, _) = repair(&[e], &ext(), None);
    assert_eq!(after[0]["metadata"], json!({"installedTimestamp": 1}));
}

#[test]
fn a_second_run_changes_nothing() {
    let (once, _) = repair(&[foreign(), ours(GHOST, &ext())], &ext(), Some("0.1.1"));
    let (twice, notes) = repair(&once, &ext(), Some("0.1.1"));
    assert_eq!(once, twice);
    assert!(notes.is_empty());
}

#[test]
fn a_repaired_entry_passes_the_audit() {
    let (after, _) = repair(&[ours(GHOST, &ext())], &ext(), None);
    assert_eq!(grades(&audit(Some(&after), &ext(), &exists(&[EXT_DIR]))), [Grade::Ok]);
}

#[test]
fn removal_takes_out_only_our_entry() {
    let (after, notes) = remove(&[foreign(), ours(EXT_DIR, &ext()), ours(GHOST, &ext())]);
    assert_eq!(after, [foreign()]);
    assert!(notes[0].1.starts_with('2'));
}

#[test]
fn the_uri_path_has_a_leading_slash_and_a_small_drive_letter() {
    assert_eq!(uri_path(r"C:\Users\x\.vscode\extensions\y"), "/c:/Users/x/.vscode/extensions/y");
    assert_eq!(uri_path("/already"), "/already");
}

#[test]
fn umlauts_in_the_path_stay_unescaped() {
    assert!(uri_path(r"C:\Users\Müßiggang\.vscode\extensions\y").contains('ß'));
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join(REGISTRY);
    save(&[json!({"p": "Müß"})], &p).unwrap();
    assert!(std::fs::read_to_string(&p).unwrap().contains("Müß"));
}

// ── Mit echter Datei: der Sicherheitsgurt ───────────────────────────────────

/// Wegwerf-Extensions-Ordner mit Quelle und installierter, unregistrierter Extension.
fn rig() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let source = d.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("package.json"), r#"{"name": "agent-deck-bridge", "version": "0.1.1"}"#).unwrap();
    std::fs::write(source.join("extension.js"), "// brücke").unwrap();
    std::fs::write(source.join("killable.js"), "// regeln").unwrap();
    let ext_dir = d.path().join("extensions");
    std::fs::create_dir_all(&ext_dir).unwrap();
    (d, ext_dir, source)
}

#[test]
fn an_unreadable_registry_is_not_overwritten() {
    let (_d, ext_dir, source) = rig();
    std::fs::write(registry_path(&ext_dir), "{kein json[").unwrap();
    let r = install(&ext_dir, &source);
    assert!(!r.is_ok() && r.lines.iter().any(|l| l.text.contains("nicht angefasst")));
    assert_eq!(std::fs::read_to_string(registry_path(&ext_dir)).unwrap(), "{kein json[");
}

#[test]
fn a_missing_registry_is_created_and_then_everything_checks_ok() {
    let (_d, ext_dir, source) = rig();
    let r = install(&ext_dir, &source);
    assert!(r.is_ok() && r.changed, "{r:?}");
    let c = check(&ext_dir, &source);
    assert_eq!((c.fails(), c.warns()), (0, 0), "{c:?}");
}

#[test]
fn check_fails_while_the_ghost_path_stands() {
    let (_d, ext_dir, source) = rig();
    install(&ext_dir, &source);
    save(&[ours(GHOST, &ext_dir)], &registry_path(&ext_dir)).unwrap();
    assert_eq!(check(&ext_dir, &source).fails(), 1);
}

#[test]
fn a_missing_side_module_is_a_fail_not_a_partial_success() {
    let (_d, ext_dir, source) = rig();
    install(&ext_dir, &source);
    std::fs::remove_file(ext_dir.join(EXT_DIR).join("killable.js")).unwrap();
    let c = check(&ext_dir, &source);
    assert!(c.lines.iter().any(|l| l.grade == Grade::Fail && l.text.contains("killable.js")), "{c:?}");
    let again = install(&ext_dir, &source);
    assert!(again.lines.iter().any(|l| l.text.contains("killable.js")), "die Bilanz nennt die nachkopierte Datei");
}

#[test]
fn without_an_installed_extension_nothing_is_registered() {
    let d = tempfile::tempdir().unwrap();
    save(&[foreign()], &registry_path(d.path())).unwrap();
    let r = register(d.path(), None);
    assert!(r.lines.iter().any(|l| l.grade == Grade::Fail && l.text.contains("nicht installiert")));
    assert_eq!(load(&registry_path(d.path())).unwrap(), [foreign()]);
}

#[test]
fn the_written_file_is_one_compact_line_without_bom() {
    let d = tempfile::tempdir().unwrap();
    let p = registry_path(d.path());
    save(&[ours(EXT_DIR, &ext()), foreign()], &p).unwrap();
    let raw = std::fs::read(&p).unwrap();
    assert!(!raw.starts_with(b"\xef\xbb\xbf"));
    assert!(!raw.contains(&b'\n'));
    let text = String::from_utf8(raw).unwrap();
    assert!(!text.contains(", ") && !text.contains("\": "));
    assert_eq!(load(&p).unwrap().len(), 2);
}

#[test]
fn a_backup_appears_only_when_something_is_really_written() {
    let (_d, ext_dir, source) = rig();
    copy_files(&source, &ext_dir.join(EXT_DIR)).unwrap();
    save(&[ours(GHOST, &ext_dir)], &registry_path(&ext_dir)).unwrap();
    let backups = || std::fs::read_dir(&ext_dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".bak-")).count();
    register(&ext_dir, Some(&source));
    assert_eq!(backups(), 1);
    register(&ext_dir, Some(&source));
    assert_eq!(backups(), 1, "Nulldurchgang: keine weitere Sicherung");
}

#[test]
fn uninstall_takes_entry_and_folder_and_leaves_the_rest() {
    let (_d, ext_dir, source) = rig();
    install(&ext_dir, &source);
    let mut entries = load(&registry_path(&ext_dir)).unwrap();
    entries.insert(0, foreign());
    save(&entries, &registry_path(&ext_dir)).unwrap();
    let r = uninstall(&ext_dir, true);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(load(&registry_path(&ext_dir)).unwrap(), [foreign()]);
    assert!(!ext_dir.join(EXT_DIR).exists());
}

#[test]
fn the_users_real_registry_is_locked_in_tests_and_throwaway_folders_are_not() {
    // Geprüft an der Schranke selbst, ohne einen Schreibversuch auf den echten Ordner.
    assert!(check_target(&extensions_dir()).is_err());
    let d = tempfile::tempdir().unwrap();
    assert!(check_target(d.path()).is_ok());
}

#[test]
fn id_and_folder_match_the_package_json_of_the_extension() {
    // EXT_ID ist publisher.name aus der package.json; VS Code bildet sie daraus.
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("extension").join("package.json");
    let pkg: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    assert_eq!(EXT_ID, format!("{}.{}", pkg["publisher"].as_str().unwrap(), pkg["name"].as_str().unwrap()));
    assert_eq!(EXT_DIR, pkg["name"].as_str().unwrap());
}
