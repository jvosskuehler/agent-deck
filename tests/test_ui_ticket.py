"""Ticket-Prompts: EINZEILIG und alle Platzhalter gefuellt.

Ein mehrzeiliger Prompt wird von der Extension als mehrere Eingaben
abgeschickt - der Agent bekommt dann Bruchstuecke.
"""

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.domain import config as cfg


# Der Text geht per sendText(execute=True) in den pty; ein \n wuerde ihn zerreissen,
# ein uebrig gebliebenes {…} wuerde der Agent woertlich sehen.
def _assert_clean_prompt(p, must_contain):
    assert "\n" not in p and "\r" not in p, "Prompt ist mehrzeilig"
    assert "{" not in p and "}" not in p, "unaufgeloester Platzhalter"
    for s in must_contain:
        assert s in p, f"fehlt im Prompt: {s}"


def test_ticket_prompt_single_line_and_filled():
    wt = "C:/Users/x/AppData/Local/claude-agent-deck/state/A1.worktree"
    p = cfg.TICKET_PROMPT.format(ticket="ABC-123", jira_key="ABC-123",
                                 branch="ticket/abc-123", slug="abc-123",
                                 wt_marker=wt, task="mach was")
    # jira_key + Jira-Lookup/Kurzvorstellung muessen drinstehen.
    _assert_clean_prompt(p, ["ABC-123", "ticket/abc-123", wt, "mach was", "Jira"])


def test_ticket_search_prompt_single_line_and_filled():
    marker = "C:/x/state/A1.ticket"
    wt = "C:/x/state/A1.worktree"
    s = cfg.TICKET_SEARCH_PROMPT.format(prefix="ticket/", marker=marker,
                                        wt_marker=wt, task="mach was")
    _assert_clean_prompt(s, ["ticket/", marker, wt, "mach was", "Jira"])


def test_heimat_block_bekommt_den_prompt_ueber_allen_repos_ohne_marker():
    """Im Ordner ueber allen Repos gibt es kein git rev-parse: der Agent bestimmt die
    Repos am Ticket und legt wt-<Nummer>-<Kuerzel> an. KEINE Marker-Datei - sonst
    raeumte das Deck die Worktrees beim Schliessen per --force weg."""
    from deck.ui.ticket import _hub_prompt
    p = _hub_prompt("ORPH-4116", r"C:\Users\x\Repos", "mach was")
    _assert_clean_prompt(p, ["ORPH-4116", "C:/Users/x/Repos/wt-4116-", "mach was", "Jira"])
    assert ".worktree" not in p and "git rev-parse" not in p


def test_nur_ein_block_ohne_eigenes_repo_gilt_als_heimat():
    import os
    import tempfile
    from deck.ui.ticket import TicketMixin

    hub = tempfile.mkdtemp(prefix="hub_")
    repo = tempfile.mkdtemp(prefix="repo_")
    os.mkdir(os.path.join(repo, ".git"))
    t = TicketMixin()
    t.wt = type("W", (), {"hosts": {"B": hub, "C": repo}})()
    assert t._hub_root("B1") == hub
    assert t._hub_root("C1") is None            # WT-Block in einem echten Repo
    assert t._hub_root("A1") is None            # VS-Code-Block
