"""claude_settings: write_values darf NUR die vier gesteuerten Keys anfassen -
Hooks und statusLine muessen die Runde ueberleben.
"""

import json
import os

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.claude import settings as cset


def test_claude_settings_write_merges_and_preserves():
    """write_values darf NUR die vier Keys anfassen – Hooks, statusLine und vor allem
    permissions.allow muessen unangetastet bleiben (sonst zerschiesst das Deck die
    handgepflegte settings.json)."""
    import shutil
    import tempfile
    base = tempfile.mkdtemp(prefix="csettings_")
    p = os.path.join(base, "settings.json")
    try:
        with open(p, "w", encoding="utf-8") as f:
            json.dump({"hooks": {"Stop": [1]}, "statusLine": {"x": 1},
                       "model": "sonnet",
                       "permissions": {"allow": ["Read"], "defaultMode": "plan"}}, f)
        cset.write_values(model="opus[1m]", mode="auto", effort="xhigh",
                          language="english", path=p)
        with open(p, encoding="utf-8") as f:
            d = json.load(f)
        assert d["model"] == "opus[1m]"
        assert d["effortLevel"] == "xhigh"
        assert d["language"] == "english"
        assert d["permissions"]["defaultMode"] == "auto"
        assert d["hooks"] == {"Stop": [1]}                 # Fremd-Keys unangetastet …
        assert d["statusLine"] == {"x": 1}
        assert d["permissions"]["allow"] == ["Read"]       # … inkl. permissions.allow!
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_claude_settings_write_creates_file_and_is_partial():
    """Fehlende Datei/Ordner wird angelegt; None-Argumente lassen ihren Key weg
    (kein leeres permissions:{} wenn mode=None)."""
    import shutil
    import tempfile
    base = tempfile.mkdtemp(prefix="csettings2_")
    p = os.path.join(base, "nested", "settings.json")
    try:
        cset.write_values(model="fable", path=p)
        with open(p, encoding="utf-8") as f:
            d = json.load(f)
        assert d == {"model": "fable"}
        vals = cset.read_values(path=p)
        assert vals["model"] == "fable" and vals["mode"] is None
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_claude_settings_read_missing_is_safe():
    vals = cset.read_values(path=os.path.join(os.path.dirname(__file__), "nope.json"))
    assert vals == {"model": None, "mode": None, "effort": None,
                    "ultracode": False, "language": None}


def test_claude_settings_label_value_mapping():
    assert cset.value_to_label(cset.MODE_CHOICES, "acceptEdits") == "Accept Edits"
    assert cset.label_to_value(cset.MODE_CHOICES, "Plan") == "plan"
    # Modell-Fuzzy: nacktes "opus" und volle ID -> Opus(1M); "fable" -> Fable.
    assert cset.value_to_label(cset.MODEL_CHOICES, "opus", contains=True) == "Opus (1M)"
    assert cset.value_to_label(cset.MODEL_CHOICES, "claude-opus-5", contains=True) == "Opus (1M)"
    # Aeltere/kuenftige IDs derselben Reihe greifen genauso (Basis-Name entscheidet).
    assert cset.value_to_label(cset.MODEL_CHOICES, "claude-opus-4-8", contains=True) == "Opus (1M)"
    assert cset.value_to_label(cset.MODEL_CHOICES, "fable", contains=True) == "Fable"
    # Neue Aliasse (exakt).
    assert cset.value_to_label(cset.MODEL_CHOICES, "sonnet") == "Sonnet"
    assert cset.value_to_label(cset.MODEL_CHOICES, "haiku") == "Haiku"
    # Unbekannt/None -> erster Eintrag (Fallback).
    assert cset.value_to_label(cset.MODE_CHOICES, "bogus") == "Auto"
    assert cset.value_to_label(cset.LANG_CHOICES, "english", contains=True) == "Englisch"


def test_bypass_ist_waehlbar_und_bleibt_beim_speichern_erhalten():
    """Fehlte Bypass in der Liste, machte das Einstellungs-Fenster beim naechsten
    Speichern still 'Auto' daraus (unbekannter Wert -> erster Eintrag)."""
    assert cset.value_to_label(cset.MODE_CHOICES, "bypassPermissions") == "Bypass"
    assert cset.label_to_value(cset.MODE_CHOICES, "Bypass") == cset.BYPASS_MODE
    assert cset.BYPASS_MODE == "bypassPermissions"   # Schreibweise laut `claude --help`


def test_default_mode_liest_neu_nur_nach_aenderung_und_haelt_den_letzten_guten_stand():
    import shutil
    import tempfile
    base = tempfile.mkdtemp(prefix="csettings3_")
    p = os.path.join(base, "settings.json")
    try:
        assert cset.default_mode(path=p) is None                    # Datei fehlt
        cset.write_values(mode="bypassPermissions", path=p)
        assert cset.default_mode(path=p) == "bypassPermissions"
        # Halb geschrieben: letzter guter Stand gilt weiter, statt auf None zu fallen.
        with open(p, "w", encoding="utf-8") as f:
            f.write('{"permissions": {"defaultMo')
        os.utime(p, (1000, 1000))
        assert cset.default_mode(path=p) == "bypassPermissions"
        cset.write_values(mode="auto", path=p)
        os.utime(p, (2000, 2000))
        assert cset.default_mode(path=p) == "auto"
        with open(p, "w", encoding="utf-8") as f:
            json.dump({"model": "fable"}, f)
        os.utime(p, (3000, 3000))
        assert cset.default_mode(path=p) is None                    # Schluessel fehlt
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_model_choices_stay_version_free():
    """Damit das Deck IMMER das neueste Modell startet, duerfen weder Wert noch Label
    eine Modellversion einfrieren: `claude --model` loest die nackten Aliasse
    ("opus", "fable", …) selbst auf das jeweils neueste Modell der Reihe auf. Eine
    feste ID ("claude-opus-4-8") oder ein Versions-Label ("Opus 4.8") wuerde beim
    naechsten Release still veralten – genau der Fall, den dieser Test verhindert."""
    import re
    for label, val in cset.MODEL_CHOICES:
        base = val.split("[")[0]                 # "opus[1m]" -> "opus"
        assert base in {"opus", "fable", "sonnet", "haiku"}, f"keine reine Alias-Angabe: {val!r}"
        assert not re.search(r"\d+[.\-]\d", val), f"Version im Wert: {val!r}"
        # Im Label ist "(1M)" (Kontextgroesse) erlaubt, "Opus 4.8"/"Fable 5" nicht.
        assert not re.search(r"[A-Za-z]\s+\d", label), f"Version im Label: {label!r}"


def test_claude_settings_effort_and_ultracode():
    # Label -> (effortLevel, ultracode)
    assert cset.effort_spec("max") == ("max", False)
    assert cset.effort_spec("Ultracode") == ("xhigh", True)
    assert cset.effort_spec("xhigh") == ("xhigh", False)
    assert cset.effort_spec("unbekannt") == ("xhigh", False)     # Fallback
    # (effortLevel, ultracode) -> Label; ultracode gewinnt.
    assert cset.effort_label("max", False) == "max"
    assert cset.effort_label("xhigh", True) == "Ultracode"
    assert cset.effort_label("high", False) == "high"
    assert cset.effort_label("bogus", False) == "xhigh"          # Fallback


def test_claude_settings_effort_ultracode_roundtrip():
    import shutil
    import tempfile
    base = tempfile.mkdtemp(prefix="csettings3_")
    p = os.path.join(base, "settings.json")
    try:
        cset.write_values(effort="xhigh", ultracode=True, path=p)
        v = cset.read_values(path=p)
        assert v["effort"] == "xhigh" and v["ultracode"] is True
        cset.write_values(effort="max", ultracode=False, path=p)      # Ultracode aus, max an
        v = cset.read_values(path=p)
        assert v["effort"] == "max" and v["ultracode"] is False
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_reenable_glow_pure_helpers():
    from deck.ops import vscode_glow as rg
    assert rg.fileurl_to_path("file:///C:/a%20b/x.css") == "C:/a b/x.css"
    assert rg.fileurl_to_path("C:/plain.css") == "C:/plain.css"
    imports = rg._extract_imports(
        '{"vscode_custom_css.imports": ["file:///C:/x.css", "file:///C:/y.js"]}')
    assert imports == ["file:///C:/x.css", "file:///C:/y.js"]
    assert rg._extract_imports("{}") is None
    # clear_existing entfernt einen Marker-Block restlos wieder.
    cleared = rg.clear_existing("<head>keep" + rg.START + "junk" + rg.END + "</head>")
    assert rg.START not in cleared and rg.END not in cleared and "keep" in cleared
