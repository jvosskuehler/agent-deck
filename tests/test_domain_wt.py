"""Windows Terminal als Host: Namen, Kommandozeile, Titel, PID-Datei, Eingabe-Schritte."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import wt


def test_naechster_slot_ist_der_hoechste_vergebene_plus_eins():
    assert wt.next_slot("A", ["A1", "A3", "B7"]) == "A4"
    assert wt.next_slot("A", []) == "A1"
    assert wt.next_slot("a", ["A2"]) == "A3"


def test_wunschname_gilt_nur_frei_und_mit_eigenem_buchstaben():
    assert wt.wanted_or_next("A", "A2", {"A1"}) == "A2"
    assert wt.wanted_or_next("A", "A1", {"A1"}) == "A2"
    assert wt.wanted_or_next("A", "B2", set()) == "A1"
    assert wt.wanted_or_next("A", None, {"A1", "A2"}) == "A3"


def test_slots_eines_buchstabens_sind_numerisch_sortiert():
    assert wt.slots_of("A", ["A10", "A2", "B1", "x", "A"]) == ["A2", "A10"]


def test_flagzeile_wird_wie_von_einer_shell_zerlegt():
    assert wt.claude_argv(args='--resume "abc" --model "opus[1m]"') == \
        ["--resume", "abc", "--model", "opus[1m]"]
    assert wt.claude_argv(model="opus") == ["--model", "opus"]
    assert wt.claude_argv() == []
    assert wt.claude_argv(model="opus", args="  ") == ["--model", "opus"]


def test_spec_kommt_unversehrt_durch_wt_und_powershell():
    spec = wt.launch_spec("A1", r"C:\Users\Voßkühler\state", ["--model", "opus[1m]"])
    assert ";" not in spec and '"' not in spec and " " not in spec
    assert wt.decode_spec(spec) == {"slot": "A1", "state": r"C:\Users\Voßkühler\state",
                                    "args": ["--model", "opus[1m]"]}


def test_jeder_agent_bekommt_ein_eigenes_fenster():
    """Kein split-pane mehr: Windows Terminal kann Panes nur halbieren, nie angleichen."""
    for slot in ("A1", "A2"):
        argv = wt.wt_argv("wt.exe", slot, r"C:\r", "pwsh.exe", "s.ps1", "SPEC")
        assert argv[:4] == ["wt.exe", "-w", "deck-" + slot, "new-tab"]
        assert "split-pane" not in argv
        assert argv[argv.index("--title") + 1] == slot and "--suppressApplicationTitle" in argv
        assert argv[argv.index("-d") + 1] == r"C:\r"
        assert argv[-2:] == ["-Spec", "SPEC"] and "-NoExit" in argv
        assert argv[argv.index("-p") + 1] == wt.PROFILE


def test_ohne_profil_steht_kein_p_schalter_da():
    argv = wt.wt_argv("wt.exe", "A1", "C:\\", "pwsh.exe", "s", "S", profile=None)
    assert "-p" not in argv


def test_fenstertitel_ist_der_slot_des_aktiven_panes():
    assert wt.slot_from_title("A3") == "A3"
    assert wt.slot_from_title(" b12 ") == "B12"
    assert wt.slot_from_title("✳ Claude Code") is None
    assert wt.slot_from_title("") is None and wt.slot_from_title(None) is None


def test_pid_datei_wird_nur_vollstaendig_geglaubt():
    assert wt.parse_pidfile({"pid": 5, "created": 7, "slot": "A1"}) == (5, 7)
    assert wt.parse_pidfile({"pid": 0, "created": 7}) is None
    assert wt.parse_pidfile({"pid": 5}) is None
    assert wt.parse_pidfile({"pid": "x", "created": 1}) is None
    assert wt.parse_pidfile(None) is None


def test_startzeit_darf_um_bruchteile_einer_sekunde_abweichen():
    t = 133_000_000_000_000_000
    assert wt.same_start(t, t + 5_000_000)
    assert not wt.same_start(t, t + 20_000_000)


def test_langer_prompt_kommt_als_paste_mit_eigenem_enter():
    assert wt.send_steps("hi", submit=True) == [
        ("text", "\x1b[200~hi\x1b[201~"), ("sleep", wt.SUBMIT_ENTER_DELAY_S), ("text", "\r")]
    assert wt.send_steps("/model opus") == [("text", "/model opus\r")]
    assert wt.send_steps("x", execute=False) == [("text", "x")]


def test_shift_tab_mehrfach_mit_pausen_dazwischen():
    assert wt.key_steps("shift-tab", 2) == [
        ("text", "\x1b[Z"), ("sleep", wt.KEY_REPEAT_DELAY_S), ("text", "\x1b[Z")]
    assert wt.key_steps("esc") == [("text", "\x1b")]
    assert wt.key_steps("q", 0) == [("text", "q")]
