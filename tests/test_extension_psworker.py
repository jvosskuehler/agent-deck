"""Der PowerShell-Worker der Extension: ein Prozess fuer alle Scans, und er heilt sich.

Die Claude-Erkennung der Extension braucht alle 4 bis 8 s den Prozessbaum. Frueher
startete dafuer jeder Scan eine neue powershell.exe (gemessen am 2026-10-14: 2,6 s
Laufzeit, 1,2 s CPU, dazu ein conhost und eine Defender-Pruefung je Start). Seither
haelt `extension/psworker.js` einen Prozess offen und schickt ihm die Anfragen ueber
stdin.

Geprueft wird zweierlei. Erstens das Protokoll ohne Prozess: eine Antwort ist erst
mit ihrer Endzeile fertig, eine halbe bleibt liegen, und eine Kommandozeile im Scan
darf keine Antwort vorzeitig beenden. Zweitens ein echter Worker: dass er wirklich
EIN Prozess bleibt, Umlaute heil liefert, und nach Zeitlimit, Absturz oder Fehler im
Skript wieder antwortet. Haengt er fest, wuerde die Erkennung sonst still nichts mehr
finden, und das sieht wie "kein Claude offen" aus.

Fehlt `node`, wird die Datei uebersprungen; fehlt `powershell.exe` (kein Windows),
nur der Teil mit dem echten Worker. Beides sichtbar, mit Meldung, nicht still.
"""
import base64
import json
import os
import shutil
import subprocess

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

WORKER = os.path.join(helpers.ROOT, "extension", "psworker.js")
NODE = shutil.which("node")
POWERSHELL = shutil.which("powershell")

# Wie in test_extension_killable.py: Skript und Daten getrennt ueber argv, damit
# nichts zu escapen ist und der Pfad mit Umlauten heil bleibt.
_CALL = (
    "const w = require(process.argv[2]);"
    "const i = JSON.parse(process.argv[1]);"
    "process.stdout.write(JSON.stringify(w[i.fn].apply(null, i.args)));"
)

M = "<<agent-deck-ps:00112233aabbccdd>>"


def _call(fn, *args):
    payload = json.dumps({"fn": fn, "args": list(args)})
    out = subprocess.run([NODE, "-e", _CALL, payload, WORKER],
                         capture_output=True, text=True, encoding="utf-8", timeout=30)
    assert out.returncode == 0, f"node scheiterte: {out.stderr.strip()}"
    return json.loads(out.stdout)


# ── Protokoll ohne Prozess ──────────────────────────────────────────────────

def test_eine_antwort_ist_erst_mit_ihrer_endzeile_fertig():
    r = _call("takeResponses", f"[1,2]\n{M} 1 o", M)
    assert r["done"] == []
    assert r["rest"] == f"[1,2]\n{M} 1 o"


def test_eine_fertige_antwort_traegt_rumpf_id_und_erfolg():
    r = _call("takeResponses", f"[1,2]\n{M} 7 ok\n", M)
    assert r["done"] == [{"id": "7", "ok": True, "body": "[1,2]"}]
    assert r["rest"] == ""


def test_zwei_antworten_in_einem_block_kommen_beide_heraus():
    r = _call("takeResponses", f"a\n{M} 1 ok\nb\n{M} 2 ok\n", M)
    assert [(d["id"], d["body"]) for d in r["done"]] == [("1", "a"), ("2", "b")]


def test_eine_halbe_zweite_antwort_bleibt_fuer_den_naechsten_block_liegen():
    r = _call("takeResponses", f"a\n{M} 1 ok\n[{{\"Name\":\"no", M)
    assert [d["id"] for d in r["done"]] == ["1"]
    assert r["rest"] == "[{\"Name\":\"no"


def test_eine_fremde_marke_im_rumpf_beendet_keine_antwort():
    # Eine Kommandozeile im Scan kann beliebigen Text enthalten - nur nicht die
    # Zufallszahl DIESES Worker-Starts.
    fremd = "<<agent-deck-ps:ffffffffffffffff>>"
    r = _call("takeResponses", f"x\n{fremd} 1 ok\ny", M)
    assert r["done"] == []


def test_err_in_der_endzeile_ist_ein_fehlschlag():
    r = _call("takeResponses", f"\n{M} 3 err\n", M)
    assert r["done"] == [{"id": "3", "ok": False, "body": ""}]


def test_ein_bom_am_anfang_gehoert_nicht_zur_antwort():
    r = _call("takeResponses", f"﻿[]\n{M} 1 ok\n", M)
    assert r["done"][0]["body"] == "[]"


def test_die_anfrage_ist_eine_zeile_und_kommt_unverbogen_an():
    skript = "$a = \"Voßkühler\"\n'zwei' | Out-String"
    zeile = _call("encodeRequest", "5", skript)
    assert zeile.endswith("\n") and zeile.count("\n") == 1
    nr, b64 = zeile.rstrip("\n").split(" ")
    assert nr == "5"
    assert base64.b64decode(b64).decode("utf-8") == skript


# ── Ein echter Worker ───────────────────────────────────────────────────────

# Ein Lauf, viele Fragen: jeder Start von PowerShell kostet Sekunden, also fuehrt EIN
# node-Aufruf alle Schritte nacheinander aus und die Tests lesen daraus. Am Ende wird
# der Worker beendet und nachgesehen, ob sein Prozess wirklich weg ist.
_PLAN_DRIVER = r"""
const w = require(process.argv[2]);
const steps = JSON.parse(process.argv[1]);
(async () => {
  const pw = new w.PsWorker({ idleMs: 0 });
  const out = [];
  for (const s of steps) {
    const script = s.script === "@PROC_SCAN" ? w.PROC_SCAN : s.script;
    const value = await pw.run(script, s.timeout || 20000);
    out.push({ value, pid: pw.pid, starts: pw.starts });
  }
  const last = pw.pid;
  pw.dispose();
  let alive = !!last;
  for (let i = 0; alive && i < 30; i++) {
    await new Promise((r) => setTimeout(r, 100));
    try { process.kill(last, 0); } catch (e) { alive = false; }
  }
  process.stdout.write(JSON.stringify({ steps: out, aliveAfterDispose: alive, self: process.pid }));
})();
"""

_STEPS = [
    {"script": "@PROC_SCAN"},                                    # 0
    {"script": "@PROC_SCAN"},                                    # 1
    {"script": "'Voßkühler'"},                                   # 2
    {"script": "throw 'kaputt'"},                                # 3
    {"script": "1 + 1"},                                         # 4
    {"script": "Start-Sleep -Seconds 10; 'zu spaet'", "timeout": 1500},  # 5
    {"script": "'wieder da'"},                                   # 6
    {"script": "[Environment]::Exit(3)"},                        # 7
    {"script": "'nach dem Absturz'"},                            # 8
]

_lauf = None


def _plan():
    global _lauf
    if _lauf is None:
        out = subprocess.run([NODE, "-e", _PLAN_DRIVER, json.dumps(_STEPS), WORKER],
                             capture_output=True, text=True, encoding="utf-8", timeout=120)
        assert out.returncode == 0, f"node scheiterte: {out.stderr.strip()}"
        _lauf = json.loads(out.stdout)
    return _lauf


def _step(i):
    return _plan()["steps"][i]


def test_zwei_scans_laufen_im_selben_prozess():
    a, b = _step(0), _step(1)
    assert a["pid"] and a["pid"] == b["pid"]
    assert b["starts"] == 1


def test_der_scan_hat_genau_die_vier_felder_der_erkennung():
    procs = json.loads(_step(1)["value"])
    assert len(procs) > 10
    assert all(sorted(p) == ["CommandLine", "Name", "ParentProcessId", "ProcessId"] for p in procs)


def test_der_scan_findet_den_aufrufer_mit_namen():
    procs = json.loads(_step(1)["value"])
    selbst = [p for p in procs if p["ProcessId"] == _plan()["self"]]
    assert selbst and selbst[0]["Name"].lower() == "node.exe"


def test_umlaute_kommen_heil_zurueck():
    assert _step(2)["value"] == "Voßkühler"


def test_ein_fehler_im_skript_ist_null_und_der_worker_lebt_weiter():
    assert _step(3)["value"] is None
    assert _step(4)["value"] == "2"
    assert _step(4)["starts"] == 1


def test_nach_dem_zeitlimit_antwortet_ein_frischer_worker():
    assert _step(5)["value"] is None
    assert _step(6)["value"] == "wieder da"
    assert _step(6)["starts"] == 2


def test_nach_einem_absturz_antwortet_ein_frischer_worker():
    assert _step(7)["value"] is None
    assert _step(8)["value"] == "nach dem Absturz"
    assert _step(8)["starts"] == 3


def test_dispose_beendet_den_prozess():
    assert _plan()["aliveAfterDispose"] is False


# Ohne node laesst sich JS nicht ausfuehren, ohne PowerShell gibt es keinen echten
# Worker. Rot waere jeder Lauf auf so einem Rechner, still bestehen die schlimmere
# Luege. Also: bestehen, aber laut (siehe test_extension_killable.py).
if not NODE:
    print("  !!  extension/psworker.js UNGEPRUEFT, node ist nicht im PATH")
    for _n in [n for n in list(globals()) if n.startswith("test_")]:
        globals()[_n] = lambda: None
elif not POWERSHELL:
    print("  !!  extension/psworker.js: echter Worker UNGEPRUEFT, kein powershell im PATH")
    for _n in ("test_zwei_scans_laufen_im_selben_prozess",
               "test_der_scan_hat_genau_die_vier_felder_der_erkennung",
               "test_der_scan_findet_den_aufrufer_mit_namen",
               "test_umlaute_kommen_heil_zurueck",
               "test_ein_fehler_im_skript_ist_null_und_der_worker_lebt_weiter",
               "test_nach_dem_zeitlimit_antwortet_ein_frischer_worker",
               "test_nach_einem_absturz_antwortet_ein_frischer_worker",
               "test_dispose_beendet_den_prozess"):
        globals()[_n] = lambda: None

if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"  ok  {name}")
