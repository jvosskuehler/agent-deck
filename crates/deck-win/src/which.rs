//! Programme auf dem PATH finden wie `shutil.which` unter Windows: ohne Endung werden
//! die Endungen aus `PATHEXT` probiert (`claude` → `claude.exe`, `claude.cmd`), mit
//! Endung genau dieser Name. Eine Umsetzung für alle Schichten (Zusammenfassung sucht
//! `claude`, der Windows-Terminal-Host `wt.exe` und `pwsh.exe`).

use std::ffi::OsStr;
use std::path::PathBuf;

/// Vorgabe, wenn `PATHEXT` fehlt (wie Python).
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Programm auf dem PATH suchen.
pub fn which(name: &str) -> Option<PathBuf> {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.into());
    let path = std::env::var_os("PATH")?;
    which_in(name, &path, &pathext)
}

/// [`which`] über einen gegebenen PATH und PATHEXT (für Tests ohne die echte Umgebung).
pub fn which_in(name: &str, path: &OsStr, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<String> = pathext.split(';').filter(|e| !e.is_empty()).map(str::to_lowercase).collect();
    let lower = name.to_lowercase();
    let has_ext = exts.iter().any(|e| lower.ends_with(e.as_str()));
    std::env::split_paths(path).find_map(|dir| {
        if has_ext {
            return Some(dir.join(name)).filter(|p| p.is_file());
        }
        exts.iter().map(|ext| dir.join(format!("{name}{ext}"))).find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_name_without_extension_tries_pathext_in_order_and_the_path_in_order() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        fs::write(b.path().join("claude.cmd"), "").unwrap();
        fs::write(b.path().join("claude.exe"), "").unwrap();
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(which_in("claude", &path, ".EXE;.CMD"), Some(b.path().join("claude.exe")));
        assert_eq!(which_in("claude", &path, ".CMD"), Some(b.path().join("claude.cmd")));
        assert_eq!(which_in("fehlt", &path, ".EXE"), None);
    }

    #[test]
    fn a_name_with_its_extension_is_taken_as_is() {
        let a = tempfile::tempdir().unwrap();
        fs::write(a.path().join("wt.exe"), "").unwrap();
        let path = std::env::join_paths([a.path()]).unwrap();
        assert_eq!(which_in("wt.exe", &path, ".COM;.EXE"), Some(a.path().join("wt.exe")));
        assert_eq!(which_in("wt.exe", &path, ""), None, "ohne PATHEXT keine bekannte Endung");
    }
}
