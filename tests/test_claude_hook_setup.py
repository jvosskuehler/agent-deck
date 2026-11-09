"""Der Hook-Merge in Claudes settings.json.

Diese Datei fasst die eine Stelle an, an der ein Fehler den AGENTEN blockiert: ein
kaputter Eintrag unter UserPromptSubmit oder PreToolUse gilt Claude Code als Veto
gegen Prompt bzw. Tool-Aufruf. Darum wird hier nicht nur geprueft, dass der Merge
schreibt, was er soll, sondern auch, dass er fremde Eintraege in Ruhe laesst und dass
ein zweiter Lauf ein Nulldurchgang ist.
"""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.claude import hook_setup as hs

ROOT = r"C:\repos\agent-deck"
OTHER = r"D:\alt\agent-deck"


def _fremd(cmd="python C:\\tools\\lint.py"):
    """Eine Hook-Gruppe, die jemand anderem gehoert."""
    return {"hooks": [{"type": "command", "command": cmd}]}


def _cmds(settings, event):
    """Alle Kommandos, die unter einem Event stehen - unsere und fremde."""
    out = []
    for g in settings.get("hooks", {}).get(event, []):
        out += [h.get("command") for h in g.get("hooks", [])]
    return out


def _texte(notes):
    """Nur die Meldungstexte aus den (grad, text)-Paaren."""
    return [t for _g, t in notes]


def test_frische_datei_bekommt_alle_sechs_hooks_und_die_statusline():
    out, notes = hs.merge({}, ROOT)
    assert set(out["hooks"]) == {e for e, _s, _m in hs.HOOKS}
    assert out["statusLine"]["command"] == hs.statusline_command(ROOT)
    assert len(notes) == len(hs.HOOKS) + 1


def test_jeder_hook_endet_auf_der_aeusseren_schale():
    """Ohne `|| exit 0` blockiert ein nicht startender Hook den Agenten."""
    out, _ = hs.merge({}, ROOT)
    for event, _status, _m in hs.HOOKS:
        for cmd in _cmds(out, event):
            assert cmd.rstrip().endswith("|| exit 0"), event


def test_nur_die_beiden_tool_events_haben_einen_matcher():
    out, _ = hs.merge({}, ROOT)
    mit = {e for e, _s, m in hs.HOOKS if m}
    for event, _status, _m in hs.HOOKS:
        hat = any("matcher" in g for g in out["hooks"][event])
        assert hat == (event in mit), event


def test_zweiter_lauf_aendert_nichts():
    """Idempotenz ist die Bedingung dafuer, dass der Installer wiederholbar ist."""
    once, _ = hs.merge({}, ROOT)
    twice, notes = hs.merge(once, ROOT)
    assert twice == once
    assert notes == []


def test_fremde_hooks_in_anderen_events_bleiben_stehen():
    vorher = {"hooks": {"PreCompact": [_fremd()]}}
    out, _ = hs.merge(vorher, ROOT)
    assert out["hooks"]["PreCompact"] == [_fremd()]


def test_fremder_hook_im_selben_event_bleibt_neben_unserem():
    vorher = {"hooks": {"Stop": [_fremd()]}}
    out, _ = hs.merge(vorher, ROOT)
    cmds = _cmds(out, "Stop")
    assert "python C:\\tools\\lint.py" in cmds
    assert hs.hook_command(ROOT, "done") in cmds


def test_fremdes_kommando_in_unserer_gruppe_ueberlebt():
    """Beides in EINER Gruppe: unser Eintrag faellt raus, der fremde bleibt in ihr."""
    vorher = {"hooks": {"Stop": [{"hooks": [
        {"type": "command", "command": hs.hook_command(OTHER, "done")},
        {"type": "command", "command": "python C:\\tools\\lint.py"},
    ]}]}}
    out, _ = hs.merge(vorher, ROOT)
    cmds = _cmds(out, "Stop")
    assert "python C:\\tools\\lint.py" in cmds
    assert hs.hook_command(ROOT, "done") in cmds
    assert hs.hook_command(OTHER, "done") not in cmds


def test_verschobenes_repo_wird_ersetzt_statt_verdoppelt():
    """Zwei Hook-Saetze wuerden in zwei state-Ordner melden - das Deck haengt dann am
    falschen, und zwar ohne Fehlermeldung."""
    alt, _ = hs.merge({}, OTHER)
    neu, notes = hs.merge(alt, ROOT)
    for event, status, _m in hs.HOOKS:
        assert _cmds(neu, event) == [hs.hook_command(ROOT, status)], event
    assert any("ersetzt" in t for t in _texte(notes))


def test_leere_fremde_gruppe_wird_nicht_aufgeraeumt():
    """Fremde Struktur ist nicht unsere Baustelle - auch wenn sie sinnlos aussieht."""
    vorher = {"hooks": {"Stop": [{"matcher": "*", "hooks": []}]}}
    out, _ = hs.merge(vorher, ROOT)
    assert {"matcher": "*", "hooks": []} in out["hooks"]["Stop"]


def test_fremde_statusline_bleibt_stehen_und_wird_als_warnung_gemeldet():
    vorher = {"statusLine": {"type": "command", "command": "python C:\\tools\\bar.py"}}
    out, notes = hs.merge(vorher, ROOT)
    assert out["statusLine"] == vorher["statusLine"]
    assert ("warn", next(t for t in _texte(notes) if "anderen Werkzeug" in t)) in notes


def test_getanes_ist_info_und_ausgelassenes_ist_warn():
    """Der Grad trennt "erledigt" von "bewusst nicht angefasst" - install.ps1 zaehlt
    daran, ohne unsere Meldungstexte zu lesen."""
    _out, notes = hs.merge({}, ROOT)
    assert {g for g, _t in notes} == {"info"}
    _out, notes = hs.merge(
        {"statusLine": {"type": "command", "command": "python C:\\tools\\bar.py"}}, ROOT)
    assert sum(1 for g, _t in notes if g == "warn") == 1


def test_eigene_statusline_mit_zusatz_wird_nicht_neu_geschrieben():
    """Wer `|| exit 0` an seine Statuszeile gehaengt hat, hat eine funktionierende -
    und der Installer schreibt sie nicht bei jedem Lauf um. Bei den sechs Hooks ist es
    umgekehrt: dort ist die exakte Form der Zweck.
    """
    eigen = hs.statusline_command(ROOT) + " || exit 0"
    vorher = {"statusLine": {"type": "command", "command": eigen,
                             "refreshInterval": hs.REFRESH_S}}
    out, notes = hs.merge(vorher, ROOT)
    assert out["statusLine"]["command"] == eigen
    assert not any("statusLine" in t for t in _texte(notes))


def test_statusline_bekommt_einen_takt_damit_der_cache_countdown_laeuft():
    """Ohne refreshInterval zeichnet Claude Code die Zeile nur bei Ereignissen neu -
    im Leerlauf, wenn der Cache abläuft, stuende der Countdown still."""
    frisch, _ = hs.merge({}, ROOT)
    assert frisch["statusLine"]["refreshInterval"] == hs.REFRESH_S
    alt = {"statusLine": {"type": "command", "command": hs.statusline_command(ROOT)}}
    out, notes = hs.merge(alt, ROOT)
    assert out["statusLine"]["refreshInterval"] == hs.REFRESH_S
    assert out["statusLine"]["command"] == hs.statusline_command(ROOT)
    assert any("statusLine" in t for t in _texte(notes))
    # Ein selbst gesetzter Takt bleibt stehen.
    eigen = {"statusLine": {**alt["statusLine"], "refreshInterval": 60}}
    out, notes = hs.merge(eigen, ROOT)
    assert out["statusLine"]["refreshInterval"] == 60
    assert not any("statusLine" in t for t in _texte(notes))


def test_eigene_statusline_aus_einem_anderen_repo_wird_ersetzt():
    vorher = {"statusLine": {"type": "command", "command": hs.statusline_command(OTHER)}}
    out, _ = hs.merge(vorher, ROOT)
    assert out["statusLine"]["command"] == hs.statusline_command(ROOT)


def test_force_ersetzt_die_fremde_statusline():
    vorher = {"statusLine": {"type": "command", "command": "python C:\\tools\\bar.py"}}
    out, _ = hs.merge(vorher, ROOT, force_statusline=True)
    assert out["statusLine"]["command"] == hs.statusline_command(ROOT)


def test_andere_schluessel_der_datei_bleiben_unberuehrt():
    vorher = {"model": "opus[1m]", "permissions": {"allow": ["Bash(git*)"]}}
    out, _ = hs.merge(vorher, ROOT)
    assert out["model"] == "opus[1m]"
    assert out["permissions"] == {"allow": ["Bash(git*)"]}


def test_merge_laesst_die_eingabe_unveraendert():
    """Rein: der Aufrufer vergleicht alt gegen neu, um zu entscheiden, ob er schreibt."""
    vorher = {"hooks": {"Stop": [_fremd()]}}
    kopie = {"hooks": {"Stop": [_fremd()]}}
    hs.merge(vorher, ROOT)
    assert vorher == kopie


# ── Entfernen ────────────────────────────────────────────────────────────────

def test_remove_nimmt_nur_unsere_eintraege_heraus():
    vorher, _ = hs.merge({"hooks": {"Stop": [_fremd()]}}, ROOT)
    out, notes = hs.remove(vorher)
    assert out["hooks"] == {"Stop": [_fremd()]}
    assert "statusLine" not in out
    assert notes


def test_remove_ist_nach_einem_leeren_merge_vollstaendig():
    voll, _ = hs.merge({}, ROOT)
    leer, _ = hs.remove(voll)
    assert leer == {}


def test_remove_laesst_eine_fremde_statusline_stehen():
    vorher = {"statusLine": {"type": "command", "command": "python C:\\tools\\bar.py"}}
    out, _ = hs.remove(vorher)
    assert out == vorher


# ── Erkennung ────────────────────────────────────────────────────────────────

def test_fremdes_report_py_gilt_nicht_als_unseres():
    assert not hs.is_ours("python C:\\tools\\my_report.py")
    assert not hs.is_ours("python C:\\tools\\report.pyc")
    assert hs.is_ours('python "C:\\x\\report.py" idle || exit 0')
    assert hs.is_ours("python C:/x/statusline.py")


def test_pfad_wird_mit_und_ohne_anfuehrungszeichen_gelesen():
    assert hs.command_path('python "C:\\a b\\report.py" idle') == "C:\\a b\\report.py"
    assert hs.command_path("python C:\\a\\report.py idle") == "C:\\a\\report.py"
    assert hs.command_path("echo nichts") is None


# ── Urteile ──────────────────────────────────────────────────────────────────

def _gut(settings=None):
    out, _ = hs.merge(settings or {}, ROOT)
    return out


def _grade(findings):
    return {g for g, _t in findings}


def test_ein_vollstaendiger_satz_ergibt_genau_ein_ok():
    findings = hs.audit(_gut(), ROOT, exists=lambda p: True)
    assert _grade(findings) == {"ok"}
    assert len(findings) == 1


def test_fehlender_hook_ist_ein_fail():
    s = _gut()
    del s["hooks"]["Notification"]
    findings = hs.audit(s, ROOT, exists=lambda p: True)
    assert any(g == "fail" and "Notification" in t for g, t in findings)


def test_cmd_slash_c_ist_ein_fail():
    """Die Falle vom 2026-07-29: Exit 0, aber kein Status - am Exit-Code unsichtbar."""
    s = _gut()
    s["hooks"]["Stop"] = [{"hooks": [{"type": "command", "command":
        'cmd /c python "C:\\repos\\agent-deck\\report.py" done || exit 0'}]}]
    findings = hs.audit(s, ROOT, exists=lambda p: True)
    assert any(g == "fail" and "cmd /c" in t for g, t in findings)


def test_fehlende_aeussere_schale_ist_ein_fail():
    s = _gut()
    s["hooks"]["UserPromptSubmit"] = [{"hooks": [{"type": "command", "command":
        'python "C:\\repos\\agent-deck\\report.py" thinking'}]}]
    findings = hs.audit(s, ROOT, exists=lambda p: True)
    assert any(g == "fail" and "exit 0" in t for g, t in findings)


def test_nicht_vorhandene_datei_ist_ein_fail():
    findings = hs.audit(_gut(), ROOT, exists=lambda p: False)
    assert any(g == "fail" and "gibt es nicht" in t for g, t in findings)


def test_hook_aus_einem_anderen_repo_ist_eine_warnung():
    s, _ = hs.merge({}, OTHER)
    findings = hs.audit(s, ROOT, exists=lambda p: True)
    assert any(g == "warn" and "anderes Repo" in t for g, t in findings)
    assert not any(g == "fail" for g, _t in findings)


def test_kurzer_und_langer_windows_pfad_sind_dasselbe_repo():
    """Gemessen beim ersten Installer-Lauf: der Hook stand unter C:\\Users\\JORRIT~1\\...,
    REPO_ROOT nennt denselben Ordner lang - und der Doctor meldete "anderes Repo".
    normcase allein loest 8.3-Kurznamen nicht auf, realpath tut es.
    """
    s, _ = hs.merge({}, r"C:\Users\JORRIT~1\deck")
    findings = hs.audit(s, r"C:\Users\Jorrit-lang\deck", exists=lambda p: True,
                        resolve=lambda p: p.replace("JORRIT~1", "Jorrit-lang"))
    assert findings == [("ok", "6 Hooks und die statusLine zeigen auf "
                               r"C:\Users\Jorrit-lang\deck")]


def test_zwei_melder_unter_einem_event_sind_eine_warnung():
    s = _gut()
    s["hooks"]["Stop"].append(
        {"hooks": [{"type": "command", "command": hs.hook_command(OTHER, "done")}]})
    findings = hs.audit(s, ROOT, exists=lambda p: True)
    assert any(g == "warn" and "gegeneinander" in t for g, t in findings)


def test_fehlende_statusline_ist_eine_warnung_kein_fail():
    """Ohne sie laeuft das Deck - nur Modell, Effort und Kontext-% bleiben leer."""
    s = _gut()
    del s["statusLine"]
    findings = hs.audit(s, ROOT, exists=lambda p: True)
    assert any(g == "warn" and "statusLine" in t for g, t in findings)
    assert not any(g == "fail" for g, _t in findings)


def test_leeres_settings_meldet_jeden_hook_einzeln():
    findings = hs.audit({}, ROOT, exists=lambda p: True)
    fails = [t for g, t in findings if g == "fail"]
    assert len(fails) == len(hs.HOOKS)
