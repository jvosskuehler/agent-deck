//! Kleinwerkzeug für Pfade und Dateien, das mehrere Dienste teilen.

use std::collections::hash_map::RandomState;
use std::ffi::OsString;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Unix-Sekunden als Float, wie `time.time()`.
pub fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// `os.path.normpath` unter Windows: `/` → `\`, `.` und `..` aufgelöst, doppelte und
/// abschließende Trenner weg. Groß-/Kleinschreibung bleibt.
pub fn normpath(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let p = p.replace('/', "\\");
    // Präfix: Laufwerk (`C:`), UNC (`\\server\share`) oder nichts.
    let (prefix, rest) = if let Some(unc) = p.strip_prefix("\\\\") {
        let parts: Vec<&str> = unc.splitn(3, '\\').collect();
        if parts.len() >= 2 {
            let head = format!("\\\\{}\\{}", parts[0], parts[1]);
            let tail = parts.get(2).copied().unwrap_or("");
            (head, format!("\\{tail}"))
        } else {
            (String::new(), p.clone())
        }
    } else if p.len() >= 2 && p.as_bytes()[1] == b':' {
        (p[..2].to_string(), p[2..].to_string())
    } else {
        (String::new(), p.clone())
    };
    let absolute = rest.starts_with('\\');
    let mut parts: Vec<&str> = Vec::new();
    for seg in rest.split('\\') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let body = parts.join("\\");
    let out = format!("{prefix}{}{body}", if absolute { "\\" } else { "" });
    if out.is_empty() { ".".into() } else { out }
}

/// `os.path.normcase(os.path.normpath(p))` unter Windows: zusätzlich klein.
pub fn normcase(p: &str) -> String {
    normpath(p).to_lowercase()
}

/// Letztes Wegstück eines Windows-Pfads (`os.path.basename`).
pub fn basename(p: &str) -> &str {
    p.rsplit(['\\', '/']).next().unwrap_or("")
}

/// Alles vor dem letzten Wegstück (`os.path.dirname`, ohne Normalisieren).
pub fn dirname(p: &str) -> &str {
    match p.rfind(['\\', '/']) {
        Some(0) => &p[..1],
        Some(i) => &p[..i],
        None => "",
    }
}

/// Datei lesen, ohne ihren Besitzer zu blockieren, mit ein paar Versuchen. Rusts
/// `File::open` öffnet unter Windows schon mit voller Freigabe (READ|WRITE|DELETE);
/// die Versuche fangen das kurze Fenster ab, in dem Claude Desktop beim Start seine
/// Dateien atomar ersetzt (1 bis 2 ms „nicht vorhanden“).
pub fn read_shared(path: &Path, retries: u32, pause: Duration) -> io::Result<Vec<u8>> {
    let mut last = None;
    for i in 0..retries.max(1) {
        match fs::read(path) {
            Ok(b) => return Ok(b),
            Err(e) => last = Some(e),
        }
        if i + 1 < retries {
            std::thread::sleep(pause);
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("nicht lesbar")))
}

/// Atomares Schreiben gibt es genau einmal, in `deck_core::paths`. Der Name bleibt hier
/// erreichbar, weil `usage::shared` ihn von hier holt.
pub use deck_core::paths::atomic_write_with;

/// Kopie `<path>.bak-YYYYmmdd-HHMMSS` neben die Datei legen, bevor sie geändert wird.
/// Nur bei echter Änderung aufrufen, sonst sammelt jeder Lauf eine Sicherung an.
pub fn backup(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    let mut dst: OsString = path.as_os_str().to_owned();
    dst.push(format!(".bak-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let dst = PathBuf::from(dst);
    fs::copy(path, &dst).ok().map(|_| dst)
}

/// Verzeichnis samt Inhalt löschen. Scheitert der erste Versuch (schreibgeschützte
/// Dateien, wie git sie unter `objects` anlegt), wird der Schreibschutz entfernt und
/// noch einmal versucht (Python: `rmtree(onerror=_force_writable)`). Verknüpfungen
/// (Symlinks, Junctions) werden nie verfolgt, nur selbst entfernt.
pub fn force_remove_dir(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(_) => {
            clear_readonly(path);
            fs::remove_dir_all(path)
        }
    }
}

fn clear_readonly(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else { return };
    if meta.file_type().is_symlink() {
        return;
    }
    let mut perm = meta.permissions();
    if perm.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        let _ = fs::set_permissions(path, perm);
    }
    if meta.is_dir()
        && let Ok(rd) = fs::read_dir(path)
    {
        for e in rd.flatten() {
            clear_readonly(&e.path());
        }
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Eine Zufallszahl ohne Zusatz-Crate: `RandomState` ist je Instanz neu verschlüsselt.
pub fn random_u64() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.finish()
}

/// Gleichverteilt in [-1, 1].
pub fn random_unit() -> f64 {
    (random_u64() >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

/// Pfad ohne Windows-Präfix `\\?\`, Kurznamen aufgelöst; `None`, wenn es ihn nicht gibt.
pub fn real_path(p: &Path) -> Option<PathBuf> {
    let c = std::fs::canonicalize(p).ok()?;
    let s = c.to_string_lossy();
    Some(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normpath_behaves_like_python_on_windows() {
        assert_eq!(normpath("C:/repo/my-backend.wt/abc/"), r"C:\repo\my-backend.wt\abc");
        assert_eq!(normpath(r"C:\a\.\b\..\c"), r"C:\a\c");
        assert_eq!(normpath(r"\\srv\share\x\..\y"), r"\\srv\share\y");
        assert_eq!(normpath("a//b"), r"a\b");
        assert_eq!(normpath("../x"), r"..\x");
        assert_eq!(normpath(""), ".");
        assert_eq!(normcase("C:/Repo/X"), r"c:\repo\x");
    }

    #[test]
    fn basename_and_dirname_split_at_either_slash() {
        assert_eq!(basename(r"C:\a\b.wt\slug"), "slug");
        assert_eq!(dirname(r"C:\a\b.wt\slug"), r"C:\a\b.wt");
        assert_eq!(dirname("x"), "");
    }

    #[test]
    fn random_unit_stays_in_its_interval() {
        for _ in 0..1000 {
            let r = random_unit();
            assert!((-1.0..=1.0).contains(&r));
        }
    }

    #[test]
    fn force_remove_dir_also_takes_read_only_files() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("objects");
        fs::create_dir_all(&sub).unwrap();
        let f = sub.join("pack");
        fs::write(&f, b"x").unwrap();
        let mut p = fs::metadata(&f).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(&f, p).unwrap();
        force_remove_dir(&sub).unwrap();
        assert!(!sub.exists());
    }
}
