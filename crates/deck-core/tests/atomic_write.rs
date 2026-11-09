//! Atomares Schreiben: eine Umsetzung für alle Schichten (`deck_core::paths`).

use std::fs;

use deck_core::paths::{atomic_write, atomic_write_with};

#[test]
fn a_write_replaces_the_file_and_leaves_no_temporary_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("x.json");
    atomic_write(&path, b"alt").unwrap();
    atomic_write_with(&path, b"neu", ".deck-tmp").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"neu");
    let names: Vec<String> =
        fs::read_dir(path.parent().unwrap()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["x.json"]);
}

#[test]
fn a_failed_replace_cleans_up_its_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    // Ein Ordner am Zielort: das Ersetzen scheitert, die Zwischendatei muss weg.
    let path = dir.path().join("ziel");
    fs::create_dir_all(path.join("inhalt")).unwrap();
    assert!(atomic_write_with(&path, b"x", ".deck-tmp").is_err());
    assert!(!dir.path().join("ziel.deck-tmp").exists());
}
