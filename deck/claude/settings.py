"""Lesen/Schreiben von Claude Codes globaler ~/.claude/settings.json.

Das Einstellungs-Fenster des Decks steuert damit die vier Defaults fuer NEU
gestartete Claude-Agenten:
  • model            -> Modell (Opus / Fable; immer als Alias = neueste Version)
  • permissions.defaultMode -> Start-Permission-Modus (auto/default/acceptEdits/plan/
                               bypassPermissions)
  • effortLevel      -> Reasoning-Effort (low/medium/high/xhigh)
  • language         -> Antwortsprache (english/german)

Wichtig: Wir MERGEN immer in die bestehende Datei (Hooks, statusLine, Plugins,
permissions.allow etc. bleiben unangetastet) und schreiben atomar + eingerueckt
(die Datei wird auch von Hand editiert). Reine Stdlib, kein Deck-Import -> von
ueberall gefahrlos nutzbar (auch aus den Tests).
"""
import json
import os
from typing import Any

# %USERPROFILE%\.claude\settings.json — dieselbe globale Datei, in der die
# report.py-Hooks und die statusLine stehen (global, weil die VS-Code-Fenster
# verschiedene Ordner haben; siehe SETUP.md Schritt 2).
SETTINGS_PATH = os.path.join(os.path.expanduser("~"), ".claude", "settings.json")

# (Anzeige-Label, in settings.json geschriebener Wert). Reihenfolge = Dropdown-
# Reihenfolge; das ERSTE Paar ist zugleich der Fallback fuer unbekannte/fehlende
# Werte.
#
# BEWUSST NUR ALIASSE, KEINE VERSIONEN – weder im Wert noch im Label:
# `claude --model` loest "opus"/"fable"/"sonnet"/"haiku" laut `claude --help`
# immer auf das NEUESTE Modell der jeweiligen Reihe auf (verifiziert 2026-07-27:
# `--model opus[1m]` -> claude-opus-5[1m], `--model opus` -> claude-opus-5).
# Eine feste ID (z.B. "claude-opus-4-8") oder ein Versions-Label wuerde beim
# naechsten Modell-Release veralten und muesste nachgepflegt werden.
# "opus[1m]" = neuestes Opus mit 1M-Kontext (derselbe Wert, den auch der
# Opus-Button des Decks per /model schickt).
MODEL_CHOICES = [("Opus (1M)", "opus[1m]"), ("Fable", "fable"),
                 ("Sonnet", "sonnet"), ("Haiku", "haiku")]
MODE_CHOICES = [("Auto", "auto"), ("Standard", "default"),
                ("Accept Edits", "acceptEdits"), ("Plan", "plan"),
                ("Bypass", "bypassPermissions")]
# Bypass = Claude Code fragt nie nach einer Erlaubnis. Beim ersten Start in diesem
# Modus zeigt Claude Code einmal einen Warnhinweis, den der Mensch selbst bestaetigt
# (danach steht skipDangerousModePermissionPrompt in settings.json). Das Deck schreibt
# diesen Schluessel bewusst NICHT: die Zustimmung gehoert dem Menschen.
BYPASS_MODE = "bypassPermissions"
LANG_CHOICES = [("Englisch", "english"), ("Deutsch", "german")]

# Effort ist ein Sonderfall: "ultracode" ist KEIN effortLevel-Wert, sondern der
# separate Boolean `ultracode` (= xhigh + Dynamic Workflows). Die Auswahl bildet
# darum auf ZWEI settings.json-Felder ab: (effortLevel, ultracode). Reihenfolge =
# Dropdown-Reihenfolge. Gueltige effortLevel (verifiziert an Claude Code 2.1.218 via
# `claude --effort <x>`): low, medium, high, xhigh, max – "ultracode" ist NICHT dabei.
EFFORT_CHOICES = ["Ultracode", "max", "xhigh", "high", "medium", "low"]
_EFFORT_MAP = {
    "Ultracode": ("xhigh", True),
    "max": ("max", False),
    "xhigh": ("xhigh", False),
    "high": ("high", False),
    "medium": ("medium", False),
    "low": ("low", False),
}


def effort_spec(label: str) -> tuple[str, bool]:
    """Anzeige-Label -> (effortLevel, ultracode). Fallback: xhigh ohne ultracode."""
    return _EFFORT_MAP.get(label, ("xhigh", False))


def effort_label(effort: str, ultracode: bool) -> str:
    """(effortLevel, ultracode) aus der Datei -> Anzeige-Label. ultracode gewinnt;
    sonst per effortLevel; nichts passt -> 'xhigh'."""
    if ultracode:
        return "Ultracode"
    for lbl in EFFORT_CHOICES:
        lvl, uc = _EFFORT_MAP[lbl]
        if not uc and lvl == effort:
            return lbl
    return "xhigh"


def load(path: str | None = None) -> dict[str, Any]:
    """settings.json als Dict lesen; fehlt/kaputt/kein Objekt -> {} (nie Exception)."""
    path = path or SETTINGS_PATH
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


# Pfad -> (st_mtime_ns, defaultMode) fuer default_mode().
_MODE_CACHE: dict[str, tuple[int, Any]] = {}


def default_mode(path: str | None = None) -> Any:
    """permissions.defaultMode (fehlt -> None), fuer den Panel-Takt: neu gelesen wird
    nur, wenn sich die Datei geaendert hat. Ist sie gerade halb geschrieben (Claude
    Code und das Deck schreiben beide hinein), gilt weiter der letzte gute Stand, statt
    fuer einen Takt auf None zu fallen; der naechste Takt liest erneut."""
    path = path or SETTINGS_PATH
    try:
        stamp = os.stat(path).st_mtime_ns
    except OSError:
        return None
    hit = _MODE_CACHE.get(path)
    if hit and hit[0] == stamp:
        return hit[1]
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        return hit[1] if hit else None
    perms = data.get("permissions") if isinstance(data, dict) else None
    mode = perms.get("defaultMode") if isinstance(perms, dict) else None
    _MODE_CACHE[path] = (stamp, mode)
    return mode


def save(data: dict[str, Any], path: str | None = None) -> None:
    """Atomar + eingerueckt schreiben: erst <path>.tmp, dann os.replace -> nie halbe
    Dateien. ensure_ascii=False haelt Umlaute/ß in den Hook-Pfaden lesbar (wie bisher).
    Legt den Zielordner bei Bedarf an."""
    path = path or SETTINGS_PATH
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
    os.replace(tmp, path)


def read_values(path: str | None = None) -> dict[str, Any]:
    """Die vier gesteuerten Werte aus der Datei ziehen (roh; nicht gesetzt -> None)."""
    d = load(path)
    raw_perms = d.get("permissions")
    perms: dict[str, Any] = raw_perms if isinstance(raw_perms, dict) else {}
    return {
        "model": d.get("model"),
        "mode": perms.get("defaultMode"),
        "effort": d.get("effortLevel"),
        "ultracode": bool(d.get("ultracode")),
        "language": d.get("language"),
    }


def write_values(model: str | None = None, mode: str | None = None,
                 effort: str | None = None, ultracode: bool | None = None,
                 language: str | None = None,
                 path: str | None = None) -> dict[str, Any]:
    """Nur die gesteuerten Keys in die BESTEHENDE Datei mergen; alles andere bleibt.
    Ein Argument = None -> diesen Key nicht anfassen. Gibt das geschriebene Dict zurueck."""
    d = load(path)
    if model is not None:
        d["model"] = model
    if effort is not None:
        d["effortLevel"] = effort
    if ultracode is not None:
        d["ultracode"] = bool(ultracode)
    if language is not None:
        d["language"] = language
    if mode is not None:
        perms = d.get("permissions")
        if not isinstance(perms, dict):
            perms = {}
        perms["defaultMode"] = mode
        d["permissions"] = perms
    save(d, path)
    return d


def label_to_value(choices: list[tuple[str, str]], label: str) -> str:
    """Anzeige-Label -> settings.json-Wert (Fallback: erster Eintrag)."""
    for lbl, val in choices:
        if lbl == label:
            return val
    return choices[0][1]


def value_to_label(choices: list[tuple[str, str]], value: str, *,
                   contains: bool = False) -> str:
    """settings.json-Wert -> Anzeige-Label. Erst exakt, dann (bei contains=True) per
    Teilstring auf dem Basis-Namen vor '[' (z.B. 'opus' aus 'opus[1m]' oder aus einer
    vollen ID wie 'claude-opus-5'). Nichts passt / value=None -> erster Eintrag."""
    if value is not None:
        s = str(value).strip()
        for lbl, val in choices:
            if val == s:
                return lbl
        if contains and s:
            sl = s.lower()
            for lbl, val in choices:
                base = val.split("[")[0].lower()
                if base and (base in sl or sl in base):
                    return lbl
    return choices[0][0]
