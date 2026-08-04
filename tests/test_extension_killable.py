"""Wen der Abraeum-Befehl (Ctrl+Alt+K) anbieten darf - und wen NIE.

Die Logik liegt in JavaScript (`extension/killable.js`), weil sie in der Extension
laeuft; VS Code laedt nur JS. Getestet wird sie trotzdem hier, ueber `node`: die
Regeln entscheiden, ob ein Klick einen haengengebliebenen Dev-Server beendet oder
die eigene Claude-Session, und diese Frage darf nicht am Handbetrieb haengen.

Der Schwerpunkt liegt auf dem NICHT-Anbieten. Unter einem VS-Code-Fenster haengen
schnell 100 Prozesse, darunter die Agenten des Decks; wird einer davon angeboten
und angeklickt, sind Arbeit und Kontext weg. Ein zu enger Filter kostet dagegen nur
einen Blick in den Task-Manager.

Fehlt `node`, wird die Datei uebersprungen - sichtbar, mit Meldung, nicht still.
"""
import json
import os
import shutil
import subprocess

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

KILLABLE = os.path.join(helpers.ROOT, "extension", "killable.js")
NODE = shutil.which("node")

# Ruft eine Funktion aus killable.js auf und gibt ihr Ergebnis als Python-Objekt
# zurueck. Skript und Daten kommen als getrennte argv-Werte herein: kein Escaping,
# und der Pfad mit Umlauten bleibt heil.
# Ein Set kommt aus JSON.stringify als {} heraus - hier zu Array aufloesen, sonst
# prueft der Baum-Test gegen ein leeres Objekt und besteht scheinbar.
_DRIVER = (
    "const k = require(process.argv[2]);"
    "const i = JSON.parse(process.argv[1]);"
    "const r = k[i.fn].apply(null, i.args);"
    "process.stdout.write(JSON.stringify(r instanceof Set ? [...r] : r));"
)


def _call(fn, *args):
    payload = json.dumps({"fn": fn, "args": list(args)})
    out = subprocess.run([NODE, "-e", _DRIVER, payload, KILLABLE],
                         capture_output=True, text=True, encoding="utf-8", timeout=30)
    assert out.returncode == 0, f"node scheiterte: {out.stderr.strip()}"
    return json.loads(out.stdout)


def _p(pid, ppid, name, cmd="", start=None, sess=1):
    # sess=1 ist die Nutzersitzung - dort laeuft alles, was ein Entwickler startet.
    return {"ProcessId": pid, "ParentProcessId": ppid, "Name": name,
            "CommandLine": cmd, "Start": start, "SessionId": sess}


def _l(port, pid):
    return {"LocalPort": port, "OwningProcess": pid}


# Ein VS-Code-Fenster, wie es wirklich aussieht: die Shell des Terminals, darunter
# der Dev-Server, daneben die Claude-Session. Dazu zwei Prozesse AUSSERHALB des
# Baums - ein fremder Server und das Panel selbst auf dem Broker-Port.
CODE, SHELL = 100, 200
SZENE = {
    "procs": [
        _p(1, 0, "explorer.exe"),
        _p(CODE, 1, "Code.exe"),
        _p(SHELL, CODE, "pwsh.exe"),
        _p(300, SHELL, "node.exe", r"node C:\proj\fe\server.js", "2026-08-03T09:09:16"),
        _p(400, SHELL, "claude.exe", "claude --model opus"),
        _p(500, SHELL, "dotnet.exe", r"dotnet run --project C:\proj\be"),
        _p(600, 1, "node.exe", r"node C:\anderes\ding.js"),
        _p(700, 1, "pythonw.exe", r"pythonw C:\repo\agent_deck.py"),
    ],
    "listen": [
        _l(3001, 300),
        _l(5000, 500), _l(5000, 500), _l(5001, 500),   # v4 + v6 + zweiter Port
        _l(4200, 600),
        _l(8765, 700),
        _l(445, 4),                                     # System haelt SMB
    ],
}

OPTS = {"skipPorts": [8765]}


def _pids(liste):
    return [e["pid"] for e in liste]


# ── Was angeboten wird ──────────────────────────────────────────────────────

def test_dev_server_unter_vs_code_wird_angeboten():
    assert 300 in _pids(_call("portBlockers", SZENE, OPTS))


def test_ein_prozess_mit_mehreren_ports_steht_nur_einmal_in_der_liste():
    treffer = [e for e in _call("portBlockers", SZENE, OPTS) if e["pid"] == 500]
    assert len(treffer) == 1
    assert treffer[0]["ports"] == [5000, 5001]


def test_die_liste_ist_nach_dem_kleinsten_port_sortiert():
    liste = _call("portBlockers", SZENE, dict(OPTS, all=True))
    ports = [e["ports"][0] for e in liste]
    assert ports == sorted(ports)


# ── Was NIE angeboten wird ──────────────────────────────────────────────────

def test_claude_session_wird_niemals_angeboten():
    szene = json.loads(json.dumps(SZENE))
    szene["listen"].append(_l(9999, 400))       # selbst wenn sie einen Port haelt
    assert 400 not in _pids(_call("portBlockers", szene, OPTS))


def test_claude_als_node_prozess_wird_am_pfad_erkannt():
    # Je nach Installation heisst der Prozess node.exe und laeuft auf cli.js. Das
    # Uebersehen waere teuer, darum greift hier der Pfad-Blick statt des Befehls.
    p = _p(800, SHELL, "node.exe", r"node C:\Users\x\.claude\local\cli.js")
    assert _call("keepReason", p, {}) is not None


def test_projektname_mit_claude_darin_bleibt_killbar():
    # Die Gegenprobe zum Pfad-Blick: `claude-experiments` ist ein Projekt, kein
    # Claude. Waere es geschuetzt, liesse sich der Blocker nie abraeumen.
    p = _p(801, SHELL, "node.exe", r"node C:\dev\claude-experiments\fe\server.js")
    assert _call("keepReason", p, {}) is None


def test_die_shell_des_terminals_wird_nicht_angeboten():
    # Sie zu beenden schliesst den Pane samt Session, gibt aber keinen Port frei.
    szene = json.loads(json.dumps(SZENE))
    szene["listen"].append(_l(9998, SHELL))
    assert SHELL not in _pids(_call("portBlockers", szene, OPTS))


def test_das_deck_selbst_wird_nicht_angeboten():
    assert 700 not in _pids(_call("portBlockers", SZENE, dict(OPTS, all=True)))


def test_der_broker_port_steht_nie_zur_auswahl():
    # Ihn zu kappen sieht aus wie ein Absturz des Panels - mitten im Gebrauch.
    szene = json.loads(json.dumps(SZENE))
    szene["procs"].append(_p(900, 1, "python.exe", "python -m http.server"))
    szene["listen"].append(_l(8765, 900))
    assert 900 not in _pids(_call("portBlockers", szene, dict(OPTS, all=True)))


def test_system_prozesse_bleiben_aussen_vor():
    assert 4 not in _pids(_call("portBlockers", SZENE, dict(OPTS, all=True)))


def test_dienste_aus_sitzung_null_werden_nie_angeboten():
    # Gemessen am 2026-08-03: der weite Modus bot lsass.exe, services.exe und
    # wininit.exe an - alle halten Ports (RPC), und lsass zu beenden ist ein
    # sofortiger Bluescreen. Sitzung 0 trennt sie sauber von allem, was ein
    # Entwickler startet (Docker und WSL liegen gemessen in Sitzung 1).
    p = _p(802, 1, "lsass.exe", "", sess=0)
    assert _call("keepReason", p, {}) == "Systemdienst (Sitzung 0)"


def test_kernprozesse_bleiben_auch_ohne_sitzungsangabe_gesperrt():
    # Der Guertel zur Sitzungs-Regel: fehlt das Feld, darf lsass trotzdem nicht
    # in der Liste landen. Eine fehlende SessionId schuetzt bewusst NICHT alles.
    p = dict(_p(803, 1, "lsass.exe"))
    del p["SessionId"]
    assert _call("keepReason", p, {}) == "Windows-Kernprozess"


def test_ein_dev_server_in_der_nutzersitzung_bleibt_killbar():
    # Die Gegenprobe: die Sitzungs-Regel darf nicht die ganze Liste leerraeumen.
    assert _call("keepReason", _p(804, 1, "node.exe", "node server.js"), {}) is None


def test_dieses_vs_code_fenster_bleibt_verschont():
    szene = json.loads(json.dumps(SZENE))
    szene["procs"].append(_p(950, CODE, "node.exe", "extension host"))
    szene["listen"].append(_l(9229, 950))
    assert 950 not in _pids(_call("portBlockers", szene, dict(OPTS, selfPid=950)))


# ── Der Zuschnitt: eng vs. weit ─────────────────────────────────────────────

def test_ohne_all_bleibt_alles_ausserhalb_von_vs_code_draussen():
    assert _pids(_call("portBlockers", SZENE, OPTS)) == [300, 500]


def test_mit_all_kommt_der_fremde_server_dazu_aber_kein_geschuetzter():
    # 300 (:3001), 600 (:4200), 500 (:5000) - nach kleinstem Port sortiert.
    assert _pids(_call("portBlockers", SZENE, dict(OPTS, all=True))) == [300, 600, 500]


# ── Robustheit ──────────────────────────────────────────────────────────────

def test_listener_ohne_prozess_stuerzt_nicht_ab():
    # Zwischen den zwei Abfragen kann ein Prozess sterben - dann steht ein Port
    # ohne Besitzer da. Er faellt weg, der Rest der Liste bleibt.
    szene = json.loads(json.dumps(SZENE))
    szene["listen"].append(_l(7777, 4242))
    assert _pids(_call("portBlockers", szene, OPTS)) == [300, 500]


def test_ein_zyklus_in_der_elternkette_laesst_den_baum_nicht_haengen():
    # Eine ParentProcessId, die im Kreis zeigt, gibt es real (recycelte PIDs).
    procs = [_p(10, 11, "a.exe"), _p(11, 10, "b.exe")]
    assert sorted(_call("descendantPids", [10], procs)) == [10, 11]


def test_ohne_listener_gibt_es_nichts_abzuraeumen():
    assert _call("portBlockers", {"procs": SZENE["procs"], "listen": []}, OPTS) == []


# Ohne node laesst sich JS nicht ausfuehren. Die Tests dann rot zu machen hiesse,
# jeden Lauf auf einem Rechner ohne node zu blockieren; sie STILL bestehen zu lassen
# waere die schlimmere Luege. Also: bestehen, aber laut - die Zeile steht ueber der
# Bilanz von tests/run.py.
if not NODE:
    print("  !!  extension/killable.js UNGEPRUEFT — node ist nicht im PATH")
    for _n in [n for n in list(globals()) if n.startswith("test_")]:
        globals()[_n] = lambda: None

if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"  ok  {name}")
