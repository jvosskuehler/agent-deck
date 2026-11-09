//! Tests zu `vscode_glow.rs`, nur gegen Wegwerf-Dateien (nie die echte workbench.html).

use super::*;

const HTML: &str = "<!DOCTYPE html>\n<html>\n<head>\n<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; require-trusted-types-for 'script'\" />\n<title>VS Code</title>\n</head>\n<body></body>\n</html>\n";

struct Rig {
    _d: tempfile::TempDir,
    wb: PathBuf,
    css: PathBuf,
}

fn rig() -> Rig {
    let d = tempfile::tempdir().unwrap();
    let wb = d.path().join("workbench").join("workbench.html");
    std::fs::create_dir_all(wb.parent().unwrap()).unwrap();
    std::fs::write(&wb, HTML).unwrap();
    let css = d.path().join("agent-deck-glow.css");
    std::fs::write(&css, ".x { color: red }").unwrap();
    Rig { _d: d, wb, css }
}

fn backups(wb: &Path) -> usize {
    std::fs::read_dir(wb.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".bak-custom-css"))
        .count()
}

#[test]
fn file_urls_become_windows_paths() {
    assert_eq!(fileurl_to_path("file:///c%3A/Users/M%C3%BC%C3%9F/glow.css"), "c:/Users/Müß/glow.css");
    assert_eq!(fileurl_to_path("file:///C:/x/y.css"), "C:/x/y.css");
    assert_eq!(fileurl_to_path(r"C:\plain\y.css"), r"C:\plain\y.css");
}

#[test]
fn imports_are_read_without_breaking_file_urls() {
    let raw = r#"{
        // Kommentar
        "editor.fontSize": 13,
        "vscode_custom_css.imports": ["file:///C:/a/glow.css", "file:///C:/b/x.js"],
    }"#;
    assert_eq!(extract_imports(raw).unwrap(), ["file:///C:/a/glow.css", "file:///C:/b/x.js"]);
    assert_eq!(extract_imports("{}"), None);
}

#[test]
fn a_patch_sits_before_head_and_drops_the_csp() {
    let r = rig();
    assert!(patch(&r.wb, std::slice::from_ref(&r.css)).unwrap());
    let html = std::fs::read_to_string(&r.wb).unwrap();
    assert!(html.contains("<style>.x { color: red }</style>"));
    assert!(html.find(START).unwrap() < html.find("</head>").unwrap());
    assert!(!html.contains("Content-Security-Policy"));
    assert_eq!(backups(&r.wb), 1);
    assert!(status_in(std::slice::from_ref(&r.wb)).installed);
}

#[test]
fn patching_twice_leaves_one_block_and_one_backup() {
    let r = rig();
    patch(&r.wb, std::slice::from_ref(&r.css)).unwrap();
    patch(&r.wb, std::slice::from_ref(&r.css)).unwrap();
    let html = std::fs::read_to_string(&r.wb).unwrap();
    assert_eq!(html.matches(START).count(), 1);
    assert_eq!(html.matches(SESSION_MARK).count(), 1);
    assert_eq!(backups(&r.wb), 1);
}

#[test]
fn unpatch_restores_the_original_including_the_csp() {
    let r = rig();
    patch(&r.wb, std::slice::from_ref(&r.css)).unwrap();
    assert!(unpatch(&r.wb).unwrap());
    assert_eq!(std::fs::read_to_string(&r.wb).unwrap(), HTML);
    assert_eq!(backups(&r.wb), 0);
    assert!(!unpatch(&r.wb).unwrap(), "kein Patch mehr da");
    assert!(!status_in(std::slice::from_ref(&r.wb)).installed);
}

#[test]
fn without_a_backup_unpatch_still_takes_the_block_out() {
    let r = rig();
    patch(&r.wb, std::slice::from_ref(&r.css)).unwrap();
    for e in std::fs::read_dir(r.wb.parent().unwrap()).unwrap().flatten() {
        if e.file_name().to_string_lossy().ends_with(".bak-custom-css") {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    assert!(unpatch(&r.wb).unwrap());
    let html = std::fs::read_to_string(&r.wb).unwrap();
    assert!(!html.contains(START) && !html.contains(SESSION_MARK));
}

#[test]
fn unreadable_or_unknown_imports_are_skipped() {
    let r = rig();
    let txt = r.css.with_extension("txt");
    std::fs::write(&txt, "x").unwrap();
    let inject = build_inject(&[r.css.with_file_name("fehlt.css"), txt, r.css.clone()]);
    assert_eq!(inject, "<style>.x { color: red }</style>");
}

#[test]
fn a_file_without_head_is_left_unchanged() {
    let r = rig();
    std::fs::write(&r.wb, "<html><body></body></html>").unwrap();
    assert!(!patch(&r.wb, &[]).unwrap());
    assert_eq!(backups(&r.wb), 0);
}

#[test]
fn the_balance_counts_files_and_names_a_missing_installation() {
    let r = rig();
    let res = set_glow_in(std::slice::from_ref(&r.wb), true, std::slice::from_ref(&r.css), Lang::German);
    assert_eq!((res.done, res.total, res.error), (1, 1, None));
    let none = set_glow_in(&[], true, &[], Lang::English);
    assert_eq!(none.error.as_deref(), Some("No VS Code installation found."));
}

#[test]
fn the_real_installation_is_never_patched_in_tests() {
    let res = set_glow(true, Path::new("x.css"), Lang::German);
    assert_eq!((res.done, res.total), (0, 0));
    assert!(res.error.is_some());
}
