//! Wo VS Code wohnt: Installationsordner, `Code.exe`, `workbench.html`.
//!
//! Eine Quelle für „wo wohnt VS Code“, nicht zwei: der Glow-Patch und der Neustart
//! fragen beide hier (Python: `vscode_glow.vscode_bases`, `vscode_restart.code_exe`).

use std::path::{Path, PathBuf};

use crate::fsutil::which;

/// Pfad ohne Windows-Präfix `\\?\`, Kurznamen aufgelöst; `None`, wenn es ihn nicht gibt.
pub fn real_path(p: &Path) -> Option<PathBuf> {
    let c = std::fs::canonicalize(p).ok()?;
    let s = c.to_string_lossy();
    Some(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()))
}

/// Kandidaten in fester Reihenfolge: ausdrücklicher Override (`AGENT_DECK_VSCODE_DIR`),
/// die Standardorte (Benutzer, Programme, je Stable und Insiders), dann aus der `code`-
/// CLI auf dem PATH abgeleitet (`<base>\bin\code.cmd` → `<base>`, deckt portable
/// Installationen ab).
pub fn candidates() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut out = Vec::new();
    out.extend(env("AGENT_DECK_VSCODE_DIR"));
    if let Some(la) = env("LOCALAPPDATA") {
        out.push(la.join("Programs").join("Microsoft VS Code"));
        out.push(la.join("Programs").join("Microsoft VS Code Insiders"));
    }
    for k in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = env(k) {
            out.push(pf.join("Microsoft VS Code"));
            out.push(pf.join("Microsoft VS Code Insiders"));
        }
    }
    for cli in ["code", "code.cmd", "code-insiders", "code-insiders.cmd"] {
        if let Some(base) = which(cli).and_then(|p| real_path(&p)).and_then(|p| p.parent()?.parent().map(Path::to_path_buf)) {
            out.push(base);
        }
    }
    out
}

/// Nur vorhandene Ordner, ohne Doppel (nach echtem Pfad).
pub fn bases_from(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        if let Some(r) = real_path(&c).filter(|r| r.is_dir())
            && !out.iter().any(|o| o.to_string_lossy().eq_ignore_ascii_case(&r.to_string_lossy()))
        {
            out.push(r);
        }
    }
    out
}

/// Die VS-Code-Installationen dieses Rechners.
pub fn vscode_bases() -> Vec<PathBuf> {
    bases_from(candidates())
}

/// Die ausführbare Datei von VS Code in einer der Installationen.
pub fn code_exe(bases: &[PathBuf]) -> Option<PathBuf> {
    bases.iter().flat_map(|b| ["Code.exe", "Code - Insiders.exe"].map(|n| b.join(n))).find(|p| p.is_file())
}

const WORKBENCH_NAMES: [&str; 3] = ["workbench.html", "workbench.esm.html", "workbench-dev.html"];
/// Neu und alt.
const WORKBENCH_SUBDIRS: [&str; 2] = ["electron-browser", "electron-sandbox"];

/// Alle `workbench.html` der Installationen. Jedes VS-Code-Update legt sie neu an, unter
/// Windows inzwischen in einem versionierten Ordner (`<base>\<hash>\resources\…`). Gesucht
/// wird darum in `<base>` und in jedem direkten Unterordner, nicht im ganzen Baum (Python
/// lief per `**` durch alle `node_modules`; die beiden Lagen sind die, die es gibt).
pub fn find_workbench_files(bases: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for b in bases {
        roots.push(b.clone());
        if let Ok(rd) = std::fs::read_dir(b) {
            let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            subs.sort();
            roots.extend(subs);
        }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        let code = root.join("resources").join("app").join("out").join("vs").join("code");
        for sub in WORKBENCH_SUBDIRS {
            for name in WORKBENCH_NAMES {
                let p = code.join(sub).join("workbench").join(name);
                if p.is_file()
                    && let Some(r) = real_path(&p)
                    && !out.contains(&r)
                {
                    out.push(r);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_bases_count_and_each_once() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("VS Code");
        std::fs::create_dir_all(&a).unwrap();
        let bases = bases_from(vec![a.clone(), d.path().join("fehlt"), a.join(".")]);
        assert_eq!(bases.len(), 1);
        assert!(bases[0].ends_with("VS Code"));
    }

    #[test]
    fn code_exe_is_found_in_the_first_base_that_has_one() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("Code - Insiders.exe"), b"").unwrap();
        assert_eq!(code_exe(&[a.clone(), b.clone()]), Some(b.join("Code - Insiders.exe")));
        assert_eq!(code_exe(&[a]), None);
    }

    #[test]
    fn workbench_files_are_found_in_both_layouts() {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().join("Microsoft VS Code");
        let old = base.join(r"resources\app\out\vs\code\electron-sandbox\workbench\workbench.html");
        let new = base.join(r"1a2b3c\resources\app\out\vs\code\electron-browser\workbench\workbench.html");
        for f in [&old, &new] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "<html></html>").unwrap();
        }
        let found = find_workbench_files(&bases_from(vec![base]));
        assert_eq!(found.len(), 2, "{found:?}");
    }
}
