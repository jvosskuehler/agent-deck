"""Der Neustart-Plan: was nach dem Beenden von VS Code wieder aufgebaut wird.

Zwei Dinge werden hier festgenagelt. Erstens die Flag-Zeile: sie geht als Text in eine
Shell (`claude --resume "<id>" …`), also muss beweisbar sein, dass nur harmlose Werte
hineinkommen - eine Session-ID ist eine UUID oder nichts. Zweitens die Zuordnung: der
Plan ist die einzige Stelle, an der „Kachel A2" und „Unterhaltung 78cbe9cd-…"
zusammengehalten werden, und ein Zahlendreher darin bedeutet, dass nach dem Neustart
fremde Chats auf den Kacheln stehen.
"""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import restart_plan as rp

SID = "78cbe9cd-67e4-4330-a2f3-34a9ee6bc20a"
SID2 = "11111111-2222-3333-4444-555555555555"
REPO = r"C:\Users\jv\Repos\orpheus"
WT = r"C:\Users\jv\Repos\orpheus.wt\ticket-42"


# ── Flag-Zeile ────────────────────────────────────────────────────────────
def test_resume_nimmt_die_session_id_in_anfuehrungszeichen():
    assert rp.flags(SID) == f'--resume "{SID}"'


def test_ohne_session_id_gibt_es_kein_resume():
    """Eine Kachel, die noch nie gefragt wurde, hat keine ID - sie startet leer.
    Ein `--resume` ohne Wert wuerde stattdessen den interaktiven Auswaehler oeffnen."""
    assert rp.flags(None) == ""
    assert rp.flags("") == ""


def test_was_keine_uuid_ist_wird_nicht_durchgelassen():
    """Der Wert landet in einer Shell-Zeile. Alles, was keine UUID ist, faellt weg -
    auch und gerade dann, wenn es wie ein Kommando aussieht."""
    for boese in ('x" ; rm -rf /', 'a b', "$(whoami)", "'", '"', SID + " extra",
                  "78cbe9cd67e443 30a2f334a9ee6bc20a", 42, None, ["x"]):
        assert rp.flags(boese) == "", boese
        assert not rp.valid_session(boese), boese


def test_grossschreibung_und_leerzeichen_in_der_id_sind_ok():
    """Eine UUID bleibt eine UUID; Rand-Whitespace aus einer Datei wird getrimmt."""
    assert rp.valid_session(SID.upper())
    assert rp.flags("  " + SID + "  ") == f'--resume "{SID}"'


def test_modell_behaelt_sein_1m_suffix_in_anfuehrungszeichen():
    """Ohne Anfuehrungszeichen frisst PowerShell die eckigen Klammern - dasselbe
    Argument wie beim Start eines neuen Agenten in extension.js."""
    assert rp.flags(SID, "opus[1m]") == f'--resume "{SID}" --model "opus[1m]"'


def test_modell_mit_sonderzeichen_faellt_weg():
    assert rp.flags(None, 'opus"; calc') == ""
    assert rp.flags(None, "opus 1m") == ""


def test_ultracode_faellt_auf_seinen_effort_level_zurueck():
    """'ultracode' ist kein CLI-Level, sondern xhigh plus ein Flag, das die CLI nicht
    kennt (siehe claude/settings.py). xhigh ist naeher am Soll als der Default - und
    der Merker in slot_effort.json bleibt unberuehrt, die Karte zeigt weiter ultracode."""
    assert rp.flags(None, None, "ultracode") == "--effort xhigh"
    assert rp.flags(None, None, "Ultracode") == "--effort xhigh"


def test_nur_gueltige_effort_level_werden_gesetzt():
    for lvl in ("low", "medium", "high", "xhigh", "max"):
        assert rp.flags(None, None, lvl) == f"--effort {lvl}"
    for muell in ("mittel", "XHIGH ; ls", "", None, 7):
        assert "--effort" not in rp.flags(None, None, muell), muell


def test_permission_mode_wird_vom_deck_namen_uebersetzt():
    """report.py schreibt Deck-Namen ('manual'), die CLI will ihre eigenen."""
    assert rp.flags(None, None, None, "manual") == "--permission-mode default"
    assert rp.flags(None, None, None, "accept") == "--permission-mode acceptEdits"
    assert rp.flags(None, None, None, "auto") == "--permission-mode auto"
    assert rp.flags(None, None, None, "plan") == "--permission-mode plan"


def test_bypass_und_dontask_werden_nicht_erzwungen():
    """Berechtigungen per Automatik AUFZUWEITEN waere der falsche Fehler - und die
    CLI-Schreibweise dieser beiden ist im Repo nicht belegt. Wer so lief, startet im
    Modus der wiederaufgenommenen Session."""
    assert rp.flags(None, None, None, "bypass") == ""
    assert rp.flags(None, None, None, "dontask") == ""


def test_die_reihenfolge_der_flags_liegt_fest():
    got = rp.flags(SID, "sonnet", "high", "auto")
    assert got == (f'--resume "{SID}" --model "sonnet" --effort high '
                   "--permission-mode auto")


# ── Ein Slot ──────────────────────────────────────────────────────────────
def test_slot_nimmt_cwd_aus_dem_zustand_mit():
    """Nicht Bequemlichkeit: Claude findet eine Session ueber ihr Projektverzeichnis,
    und ein Ticket-Agent lief im worktree, nicht in der Repo-Wurzel."""
    e = rp.slot_entry("A2", {"session_id": SID, "cwd": WT})
    assert e["cwd"] == WT and e["slot"] == "A2" and not e["fresh"]


def test_slot_ohne_zustand_ist_frisch():
    """Kein Hook gefeuert -> keine Datei -> keine ID. Die Kachel kommt leer zurueck,
    aber sie kommt zurueck."""
    e = rp.slot_entry("A3", None)
    assert e["fresh"] and e["args"] == "" and e["cwd"] == ""


def test_gemerktes_effort_schlaegt_das_gemeldete():
    """slot_effort.json unterscheidet ultracode von xhigh, die Hook-Meldung nicht."""
    e = rp.slot_entry("A1", {"session_id": SID, "effort": "xhigh"}, effort="ultracode")
    assert "--effort xhigh" in e["args"]
    e2 = rp.slot_entry("A1", {"session_id": SID, "effort": "low"})
    assert "--effort low" in e2["args"]


def test_eine_id_ohne_transcript_gilt_als_keine_session():
    """Gemessen an den echten Zustandsdateien: eine ID kann eine geloeschte
    Unterhaltung nennen (die Datei in state/ ueberlebt das Aufraeumen in
    ~/.claude/projects). `claude --resume` startet darauf gar nicht - die Kachel kaeme
    mit einer Fehlermeldung statt mit einem Chat zurueck. Also lieber leer starten."""
    e = rp.slot_entry("A1", {"session_id": SID, "cwd": REPO},
                      can_resume=lambda sid, cwd: False)
    assert e["fresh"] and "--resume" not in e["args"]
    ok = rp.slot_entry("A1", {"session_id": SID, "cwd": REPO},
                       can_resume=lambda sid, cwd: True)
    assert not ok["fresh"] and "--resume" in ok["args"]


def test_das_praedikat_bekommt_id_und_verzeichnis():
    """Beides ist noetig: dasselbe Transcript liegt je cwd in einem anderen Ordner."""
    gefragt = []
    rp.slot_entry("A1", {"session_id": SID, "cwd": WT},
                  can_resume=lambda sid, cwd: gefragt.append((sid, cwd)) or True)
    assert gefragt == [(SID, WT)]


# ── Der ganze Plan ────────────────────────────────────────────────────────
CLIENTS = [
    {"window": "A", "workspace": "orpheus", "folder": REPO, "slots": ["A1", "A2", "A3"]},
    {"window": "B", "workspace": "leer", "folder": None, "slots": ["B1"]},
    {"window": None, "workspace": "unverbunden", "folder": REPO, "slots": []},
]
STATES = {
    "A1": {"session_id": SID, "cwd": REPO, "mode": "auto", "effort": "xhigh"},
    "A2": {"session_id": SID2, "cwd": WT, "mode": "plan"},
}


def test_fenster_ohne_ordner_wird_uebersprungen_nicht_geraten():
    """Ein leeres VS-Code-Fenster kann man nicht wieder aufmachen - es gibt nichts zu
    oeffnen. Es steht in `skipped`, damit der Dialog es benennen kann, statt es
    stillschweigend zu verlieren."""
    plan = rp.build(CLIENTS, STATES)
    assert [w["window"] for w in plan["windows"]] == ["A"]
    assert plan["skipped"] == ["B"]


def test_verbundenes_fenster_ohne_buchstaben_zaehlt_nicht_mit():
    """Ohne Buchstaben gibt es keine Kachel und keine Adresse zum Senden - so ein
    Client ist weder Plan noch Verlust."""
    plan = rp.build(CLIENTS, STATES)
    assert len(plan["windows"]) == 1 and plan["skipped"] == ["B"]


def test_die_gezogene_kachel_reihenfolge_bestimmt_die_startreihenfolge():
    """Die Extension vergibt Slot-Namen fortlaufend und jeder Chat entsteht als Split
    des vorherigen - die Chats muessen also in genau der Reihenfolge wieder entstehen,
    in der sie stehen. Sonst zeigen die Kacheln danach fremde Unterhaltungen."""
    plan = rp.build(CLIENTS, STATES, {"A": ["A3", "A1", "A2"]})
    assert [s["slot"] for s in plan["windows"][0]["slots"]] == ["A3", "A1", "A2"]


def test_unbekannte_slots_haengen_hinten_an():
    """apply_order-Vertrag: was die Extension meldet, ist die Wahrheit darueber, WAS es
    gibt - die gemerkte Reihenfolge nur darueber, in welcher Folge."""
    plan = rp.build(CLIENTS, STATES, {"A": ["A2"]})
    assert [s["slot"] for s in plan["windows"][0]["slots"]] == ["A2", "A1", "A3"]


def test_bilanz_trennt_wiederaufnehmbar_von_leer():
    """Die Zahlen im Dialog: drei Kacheln, zwei mit Session."""
    wins, keep, fresh = rp.counts(rp.build(CLIENTS, STATES))
    assert (wins, keep, fresh) == (1, 2, 1)


def test_das_deck_modell_gilt_fuer_alle_wiederhergestellten_chats():
    """Dieselbe Regel wie beim ＋-Knopf: das eingestellte Modell wird per CLI-Flag
    erzwungen, weil settings.json der schwaechste Hebel ist."""
    plan = rp.build(CLIENTS, STATES, model="opus[1m]")
    for s in plan["windows"][0]["slots"]:
        if not s["fresh"]:
            assert '--model "opus[1m]"' in s["args"]


# ── Fortsetzen nach einem Abbruch ─────────────────────────────────────────
PLAN = {"windows": [{"window": "A", "workspace": "orpheus", "folder": REPO,
                     "slots": [{"slot": "A1", "args": "", "cwd": REPO}]}]}


def test_ein_plan_dessen_fenster_alle_zu_sind_wird_fortgesetzt():
    """Genau der Zustand nach einem abgebrochenen Lauf: geschlossen, aber nicht wieder
    da. Ein frischer Plan waere jetzt leer - und wuerde den alten ueberschreiben."""
    assert rp.needs_relaunch(PLAN, [])
    assert rp.needs_relaunch(PLAN, ["C", "D"])


def test_ein_plan_mit_lebendem_fenster_wird_nicht_fortgesetzt():
    """Mindestens eines der geplanten Fenster ist wieder da -> der Lauf gilt als
    erledigt (oder der Nutzer hat selbst geoeffnet). Dann lieber neu planen."""
    assert not rp.needs_relaunch(PLAN, ["A"])
    assert not rp.needs_relaunch(PLAN, ["a"])


def test_muell_ist_kein_plan():
    for x in (None, {}, {"windows": []}, {"windows": [{"window": "A"}]},
              {"windows": [{"folder": REPO}]}, "text", 7):
        assert not rp.is_usable(x), x
        assert not rp.needs_relaunch(x, []), x


def test_speichern_und_lesen_ueberlebt_den_umweg_ueber_die_platte():
    """Die Datei ist der Grund, warum ein abgestuerztes Panel den Neustart nicht
    verliert - also muss sie auch wirklich wieder lesbar sein."""
    import os
    import tempfile
    path = os.path.join(tempfile.mkdtemp(), "restart_plan.json")
    plan = rp.build(CLIENTS, STATES, {"A": ["A2", "A1"]})
    rp.save(plan, path)
    back = rp.load(path)
    assert back is not None
    assert [s["slot"] for s in back["windows"][0]["slots"]] == ["A2", "A1", "A3"]
    rp.clear(path)
    assert rp.load(path) is None
    rp.clear(path)          # zweites Mal -> still ok, kein Fehler


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"  ok  {name}")
