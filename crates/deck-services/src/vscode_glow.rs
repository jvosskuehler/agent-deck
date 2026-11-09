//! Der Glow (leuchtender Ring um den fokussierten Chat) in VS Code: ein, aus, Stand
//! (Python `ops/vscode_glow.py`).
//!
//! Repliziert, was die Extension „Custom CSS and JS Loader“ bei „Enable Custom CSS and
//! JS“ tut: die unter `vscode_custom_css.imports` eingetragenen CSS/JS-Dateien als
//! `<style>`/`<script>` in VS Codes `workbench.html` setzen. Nötig, weil jedes VS-Code-
//! Update die `workbench.html` ersetzt und der Patch dann weg ist, obwohl die
//! settings.json unverändert stimmt. Die CSS liegt in `resources/agent-deck-glow.css`.
//! Wirkung erst nach „Developer: Reload Window“.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use deck_core::i18n::Lang;
use regex::Regex;

use crate::fsutil::{atomic_write_with, uuid4};
use crate::guard;
use crate::vscode_install::{find_workbench_files, vscode_bases};

pub const START: &str = "<!-- !! VSCODE-CUSTOM-CSS-START !! -->";
pub const END: &str = "<!-- !! VSCODE-CUSTOM-CSS-END !! -->";
const SESSION_MARK: &str = "VSCODE-CUSTOM-CSS-SESSION-ID";

static BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?s){}.*?{}[\r\n]*", regex::escape(START), regex::escape(END))).expect("BLOCK"));
static SESSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!-- !! VSCODE-CUSTOM-CSS-SESSION-ID [\w-]+ !! -->[\r\n]*").expect("SESSION"));
static INDICATOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<script>/\* eslint-env browser \*/.*?__CUSTOM_CSS_JS_INDICATOR_CLS.*?</script>[\r\n]*").expect("INDICATOR")
});
static CSP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<meta\s+http-equiv="Content-Security-Policy"[\s\S]*?/>"#).expect("CSP"));
static SESSION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"VSCODE-CUSTOM-CSS-SESSION-ID ([0-9a-fA-F-]+)").expect("SESSION_ID"));
static IMPORTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)"vscode_custom_css\.imports"\s*:\s*\[(.*?)\]"#).expect("IMPORTS"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]*)""#).expect("QUOTED"));

/// `file:///C:/x/y.css` → `C:/x/y.css` (prozentkodiert gelesen); anderes bleibt.
pub fn fileurl_to_path(u: &str) -> String {
    let Some(rest) = u.strip_prefix("file:") else { return u.to_string() };
    // Wie urlparse: `//<host>` vor dem Pfad abschneiden (bei Dateien meist leer).
    let path = match rest.strip_prefix("//") {
        Some(after) => after.find('/').map_or("", |i| &after[i..]),
        None => rest,
    };
    let mut p = percent_decode(path);
    let b = p.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        p.remove(0);
    }
    p
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Das `imports`-Array gezielt aus dem settings-Text ziehen. Bewusst KEIN JSONC-Parser:
/// ein naiver Kommentar-Entferner zerstörte das `//` in `file:///`-Adressen.
pub fn extract_imports(raw: &str) -> Option<Vec<String>> {
    let body = IMPORTS.captures(raw)?.get(1)?.as_str();
    Some(QUOTED.captures_iter(body).map(|c| c[1].to_string()).collect())
}

/// Die Imports aus VS Codes settings.json (Stable, dann Insiders); sonst `default`.
pub fn read_imports(default: &[PathBuf]) -> Vec<PathBuf> {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else { return default.to_vec() };
    for prod in ["Code", "Code - Insiders"] {
        let Ok(raw) = std::fs::read_to_string(appdata.join(prod).join("User").join("settings.json")) else { continue };
        if let Some(imports) = extract_imports(&raw).filter(|i| !i.is_empty()) {
            return imports.iter().map(|u| PathBuf::from(fileurl_to_path(u))).collect();
        }
    }
    default.to_vec()
}

/// Einen vorhandenen Patch (auch einen der Original-Extension) entfernen.
pub fn clear_existing(html: &str) -> String {
    let h = BLOCK.replace_all(html, "");
    let h = SESSION.replace_all(&h, "");
    INDICATOR.replace_all(&h, "").into_owned()
}

/// Die einzusetzenden Blöcke. Unlesbares (auch Nicht-UTF-8) und unbekannte Typen werden
/// übersprungen, statt den ganzen Patch abzubrechen.
pub fn build_inject(imports: &[PathBuf]) -> String {
    let mut out = String::new();
    for p in imports {
        let Ok(content) = std::fs::read_to_string(p) else { continue };
        match p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
            Some("css") => out += &format!("<style>{content}</style>"),
            Some("js") => out += &format!("<script>{content}</script>"),
            _ => {}
        }
    }
    out
}

fn sweep_backups(dir: &Path, keep: Option<&str>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".bak-custom-css") && Some(name.as_str()) != keep {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Eine `workbench.html` patchen (idempotent). `false` = kein `</head>` gefunden.
pub fn patch(wb: &Path, imports: &[PathBuf]) -> io::Result<bool> {
    let html = clear_existing(&std::fs::read_to_string(wb)?);
    let dir = wb.parent().unwrap_or(Path::new("."));
    let session = uuid4();
    let backup_name = format!("workbench.{session}.bak-custom-css");
    let backup = dir.join(&backup_name);
    // Sicherung = das BEREINIGTE html, vor dem Entfernen der CSP (wie die Extension).
    std::fs::write(&backup, &html)?;
    // Die CSP-Zeile muss weg, sonst stört `require-trusted-types-for` das Skript.
    let html = CSP.replace(&html, "").into_owned();
    let Some(at) = html.find("</head>") else {
        let _ = std::fs::remove_file(&backup);
        return Ok(false);
    };
    let block = format!("<!-- !! {SESSION_MARK} {session} !! -->\n{START}\n{}{END}\n</head>", build_inject(imports));
    let new_html = format!("{}{block}{}", &html[..at], &html[at + "</head>".len()..]);
    // Atomar: die geladene workbench.html bleibt bei einem Abbruch unversehrt.
    if let Err(e) = atomic_write_with(wb, new_html.as_bytes(), ".glow-tmp") {
        let _ = std::fs::remove_file(&backup);
        return Err(e);
    }
    sweep_backups(dir, Some(&backup_name));
    Ok(true)
}

/// Patch entfernen: die Sicherung zurücklegen, sonst wenigstens den Block herausnehmen.
/// `false` = es war kein Patch da.
pub fn unpatch(wb: &Path) -> io::Result<bool> {
    let html = std::fs::read_to_string(wb)?;
    let Some(id) = SESSION_ID.captures(&html).map(|c| c[1].to_string()) else { return Ok(false) };
    let dir = wb.parent().unwrap_or(Path::new("."));
    let bak = dir.join(format!("workbench.{id}.bak-custom-css"));
    let restored = std::fs::read_to_string(&bak).unwrap_or_else(|_| clear_existing(&html));
    atomic_write_with(wb, restored.as_bytes(), ".glow-tmp")?;
    sweep_backups(dir, None);
    Ok(true)
}

/// Stand des Glow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlowStatus {
    /// In mindestens einer `workbench.html` steht der Patch.
    pub installed: bool,
    /// Gefundene `workbench.html` (0 = kein VS Code gefunden).
    pub workbench_files: usize,
}

pub fn status_in(files: &[PathBuf]) -> GlowStatus {
    let installed = files.iter().any(|f| std::fs::read_to_string(f).is_ok_and(|h| h.contains(SESSION_MARK)));
    GlowStatus { installed, workbench_files: files.len() }
}

/// Stand über alle VS-Code-Installationen dieses Rechners (nur lesend).
pub fn status() -> GlowStatus {
    status_in(&find_workbench_files(&vscode_bases()))
}

/// Ergebnis von „Glow an/aus“: wie viele Dateien geändert, wie viele gefunden, Fehler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlowResult {
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
}

/// Glow in den genannten Dateien an- oder abschalten. Wirft nicht.
pub fn set_glow_in(files: &[PathBuf], enabled: bool, imports: &[PathBuf], lang: Lang) -> GlowResult {
    if files.is_empty() {
        let e = lang.l("Keine VS-Code-Installation gefunden.", "No VS Code installation found.");
        return GlowResult { done: 0, total: 0, error: Some(e.into()) };
    }
    let mut done = 0;
    let mut error = None;
    for wb in files {
        match if enabled { patch(wb, imports) } else { unpatch(wb) } {
            Ok(true) => done += 1,
            Ok(false) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                error = Some(
                    lang.l("Keine Schreibrechte. VS Code schließen und erneut versuchen.", "No write permission. Close VS Code and try again.")
                        .to_string(),
                );
            }
            Err(e) => error = Some(e.to_string()),
        }
    }
    GlowResult { done, total: files.len(), error }
}

/// Glow in ALLEN VS-Code-Installationen an- oder abschalten. `default_css` gilt, wenn in
/// VS Codes settings.json keine Imports stehen (die App reicht ihre Ressource herein).
pub fn set_glow(enabled: bool, default_css: &Path, lang: Lang) -> GlowResult {
    if let Err(e) = guard::check("workbench.html patchen") {
        return GlowResult { done: 0, total: 0, error: Some(e.to_string()) };
    }
    let imports = if enabled { read_imports(&[default_css.to_path_buf()]) } else { Vec::new() };
    set_glow_in(&find_workbench_files(&vscode_bases()), enabled, &imports, lang)
}

#[cfg(test)]
#[path = "vscode_glow_tests.rs"]
mod tests;
