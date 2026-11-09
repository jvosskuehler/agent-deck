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

/// Atomar schreiben über eine eigene Zwischendatei (`<path><suffix>`), dann ersetzen.
pub fn atomic_write_with(path: &Path, bytes: &[u8], suffix: &str) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut tmp: OsString = path.as_os_str().to_owned();
    tmp.push(suffix);
    let tmp = PathBuf::from(tmp);
    if let Err(e) = fs::write(&tmp, bytes).and_then(|_| fs::rename(&tmp, path)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

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

/// Programm auf dem PATH suchen wie `shutil.which` unter Windows (mit PATHEXT).
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_lowercase())
        .collect();
    let has_ext = exts.iter().any(|e| name.to_lowercase().ends_with(e));
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if has_ext {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
            continue;
        }
        for ext in &exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
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

/// Eine UUID der Version 4 als Text (Kennung des Glow-Patches, wie `uuid.uuid4()`).
pub fn uuid4() -> String {
    let (a, b) = (random_u64(), random_u64());
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..].copy_from_slice(&b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
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
    fn uuid4_has_the_canonical_shape() {
        let re = regex::Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$").unwrap();
        let (a, b) = (uuid4(), uuid4());
        assert!(re.is_match(&a), "{a}");
        assert_ne!(a, b);
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
