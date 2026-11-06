"""Fenster und Slots pflegen: Bindungen nachziehen, geschlossene Fenster abraeumen,
neuen Agenten ihren Startmodus antreiben.

Laeuft im Poll-Takt von refresh.py, ist aber ein eigenes Thema: hier geht es nicht um
Anzeige, sondern um die Frage, welches VS-Code-Fenster noch da ist und welcher Slot zu
wem gehoert.

Ein neuer Agent bekommt seinen Startmodus NICHT nach dem Prinzip feuern-und-vergessen:
erst ein Readiness-Gate (der erste Hook muss da sein), dann Bestaetigen und Nachfassen -
sonst bleibt er auf dem Weg haengen.
"""
import os
import time
from typing import Any

from deck.claude import settings as cset
from deck.domain import config as cfg
from deck.domain import paths as dp
from deck.domain import status_model as sm
from deck.domain.binding import is_placeholder_ws as _is_placeholder_ws
from deck.domain.binding import repo_from_title as _repo_from_title
from deck.domain.binding import strip_remote_tag as _strip_remote_tag
from deck.platform import focus as wf


class WindowSyncMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def _ensure_home_terminal(self) -> None:
        """Den Heimat-Block sicherstellen (config.TERMINAL_HOME): einen Windows-Terminal-
        Block ueber dem Ordner aller Repos, der beim Start schon da ist - ohne offenen
        Agenten, das Fenster oeffnet erst das ＋. Gibt es ihn schon (wt_hosts.json), wird
        er wiederverwendet und behaelt seinen Buchstaben; sonst bekommt er den ersten
        freien. Laeuft einmal beim Start: wer ihn per Rechtsklick vergisst, ist ihn bis
        zum naechsten Start los."""
        self.home_win = None
        folder = self.settings.get("terminal_home") or cfg.TERMINAL_HOME
        if not folder or not os.path.isdir(folder):
            return
        folder = os.path.normpath(folder)
        key = os.path.normcase(folder)
        win = next((w for w, f in self.wt.hosts.items()
                    if os.path.normcase(os.path.normpath(f)) == key), None)
        if win is None:
            win = next((w for w in cfg.WINDOWS
                        if not self.bindings.get(w) and not self.wt.has(w)), None)
            if win is None:
                return
            self.wt.open_repo(win, folder)
        name = os.path.basename(folder)
        if self.bindings.get(win) != name:
            self.bindings[win] = name
            self.store.save_bindings()
        self.home_win = win

    def _sync_bindings(self) -> None:
        """Auto-Verknuepfung + Auto-Bind: eine gemerkte Extension bekommt ihren
        Buchstaben; jede verbundene Extension OHNE Buchstaben das naechste freie
        Fenster -> alles verbindet sich selbst."""
        for g, repo in self.bindings.items():
            if repo and not self.broker.connected(g):
                self.broker.assign(repo, g)
        bound = set(self.bindings.values())
        for ws in self.broker.workspaces():
            if _is_placeholder_ws(ws) or ws in bound:
                continue
            # Bevorzugt den Buchstaben, den die vorhandenen Terminals dieses Clients
            # schon tragen (Slot-Namen wie 'C1') -> der Buchstabe bleibt ueber
            # forget/neu-verbinden stabil und die Kacheln bleiben ansprechbar.
            slots = self.broker.workspace_slots(ws)
            pref = slots[0][0].upper() if (slots and slots[0]) else None
            free = pref if (pref in cfg.WINDOWS and not self.bindings.get(pref)) else \
                next((w for w in cfg.WINDOWS if not self.bindings.get(w)), None)
            if not free:
                break
            self.bindings[free] = ws
            bound.add(ws)
            self.store.save_bindings()
            self.broker.assign(ws, free)
            self._last_sig = None

    def _open_vscode_repos(self) -> Any:
        """Repo-/Ordnernamen (lowercased) ALLER aktuell offenen VS-Code-Fenster, aus den
        Fenstertiteln gezogen. None, wenn die Win32-Enumeration fehlschlaegt -> der Aufrufer
        raeumt dann NICHT ab (lieber eine tote Kachel stehen lassen als eine lebende
        faelschlich abraeumen)."""
        try:
            titles = wf.list_titles(cfg.VSCODE_MARKER)
        except Exception:
            return None
        repos = set()
        for title in titles:
            repo = _repo_from_title(title)
            if repo and not _is_placeholder_ws(repo) and repo != cfg.VSCODE_MARKER:
                repos.add(repo.lower())
        return repos

    def _cleanup_closed_windows(self) -> None:
        """Ein gebundenes Fenster automatisch abraeumen (Bindung vergessen -> Kachel weg),
        sobald sein VS-Code-Fenster WIRKLICH geschlossen wurde. Abgrenzung zum blossen
        Reload/kurzen Verbindungsabriss: bei einem Reload bleibt das native VS-Code-Fenster
        offen, sein Titel (mit dem Repo-Namen) also sichtbar -> wir raeumen NUR ab, wenn KEIN
        offenes VS-Code-Fenster mehr zu diesem Repo existiert. Ein kurzer Grace
        (cfg.STALE_WINDOW_S) faengt den Socket-zu/HWND-noch-da-Moment und Titel-Aussetzer ab. Ein
        noch lebendes Fenster bindet sich wie gehabt automatisch neu (_sync_bindings)."""
        pending = [w for w in cfg.WINDOWS
                   if self.bindings.get(w) and not self.broker.connected(w)]
        if not pending:
            self._gone_since.clear()
            return
        open_repos = self._open_vscode_repos()
        if open_repos is None:
            return                                  # Enumeration nicht verfuegbar -> nicht abraeumen
        now = time.time()
        changed = False
        for w in pending:
            repo = self.bindings.get(w)
            # Gebundener Name traegt das Remote-Tag, die Titelliste ist nackt (beide
            # Tags entstehen unabhaengig) -> hier abstreifen, sonst gilt ein lebendes
            # Remote-Fenster nach STALE_WINDOW_S als geschlossen und die Kachel geht.
            if repo and _strip_remote_tag(repo).lower() in open_repos:
                self._gone_since.pop(w, None)       # Fenster lebt (z.B. Reload) -> Uhr ruecksetzen
                continue
            t0 = self._gone_since.get(w)
            if t0 is None:
                self._gone_since[w] = now           # erstmals als "weg" gesehen -> Uhr starten
            elif now - t0 >= cfg.STALE_WINDOW_S:
                # Fenster ist wirklich zu. Wurde es NICHT ueber die Deck-Knoepfe geschlossen
                # (Alt+F4/OS-X/Absturz), lief close_window nie -> hier dieselbe Slot-Aufraeumung
                # nachholen wie dort, sonst bleiben worktree + Marker/Ticket verwaist (und ein
                # spaeter denselben Slot-Namen erbender Agent koennte am alten Marker haengen).
                for slot in self._slots_for_window(w):
                    self._cleanup_worktrees(slot)
                    if self.tickets.pop(slot, None) is not None:
                        self.store.save_tickets()
                    self._clear_found_ticket(slot)
                    self._forget_slot(slot)
                del self.bindings[w]                # Bindung vergessen -> Kachel abraeumen
                self._gone_since.pop(w, None)
                if self.active_slot and self.active_slot[0] == w:
                    self.active_slot = None
                changed = True
        # Wieder verbundene Fenster aus der Uhr nehmen (Dict sauber halten).
        for w in list(self._gone_since):
            if w not in pending:
                self._gone_since.pop(w, None)
        if changed:
            self.store.save_bindings()
            self._last_sig = None                   # Layout sofort neu zeichnen

    def _autofocus_new(self) -> None:
        """Neuen "＋"-Chat automatisch auswaehlen + fokussieren, sobald sein Slot da ist.
        Das Vormerken fuer den Auto-Startmodus (_register_pending_auto: nur Dict-Eintrag +
        Datei-Lesen, KEIN Fokus) geschieht sofort bei Erst-Erkennung — moeglichst FRUEH, vor
        dem SessionStart-Report des Agenten, damit dessen baseline-ts stimmt. Das FOKUS-Holen
        dagegen NICHT waehrend eines modalen Dialogs: focus_slot holt per Win32 das VS-Code-
        Fenster nach vorn (SetForegroundWindow) und wuerde dem Button-Dialog mitten im Tippen
        den OS-Fokus klauen -> bei offenem Dialog Auto-Fokus auslassen (Vormerkung steht)."""
        if not self._await_new:
            return
        win, before, ts = self._await_new
        fresh_slots = [s for s in self.broker.terminals(win) if s not in before]
        if fresh_slots:
            self._register_pending_auto(fresh_slots)   # neue Agenten -> Auto-Startmodus (fokusfrei)
            self._await_new = None
            if not self._modal:
                self.focus_slot(fresh_slots[-1])   # der zuletzt angelegte
        elif not self._modal and time.time() - ts > 8:
            self._await_new = None             # nichts erschienen -> aufgeben

    def _register_pending_auto(self, slots) -> None:
        """Frisch per ＋ angelegte Slots fuer den Auto-Startmodus (config.NEW_AGENT_MODE)
        vormerken. Je Slot ein Fortschritts-Dict:
          • base_ts  = ts einer evtl. noch herumliegenden ALTEN Zustands-Datei (0, wenn
            keine da). _apply_pending_auto treibt erst bei einem NEUEREN Hook -> die alte
            Restdatei (Slot-Reuse) loest NICHT faelschlich aus, und der Wechsel greift auch,
            wenn die Vormerkung (z.B. bei offenem Dialog) erst NACH dem SessionStart-Report
            passiert (base ist die ALTE ts, nicht 'jetzt').
          • reg_ts   = jetzt, nur als Anker fuer cfg.PENDING_AUTO_TTL (Geduld ab Vormerkung).
          • ready_ts = 0; wird auf 'jetzt' gesetzt, sobald der erste frische Hook da ist
            (Anker fuer cfg.AUTO_READY_GRACE, damit die TUI-Eingabe erst warmlaeuft).
          • sent_ts  = 0; Zeitpunkt, zu dem wir zuletzt Shift+Tab geschickt haben (0 = noch
            nie). Trennt 'erst-antreiben' von 'bestaetigen/nachfassen'.
          • tries    = Anzahl bisheriger (Nach-)Antriebe (gedeckelt per cfg.AUTO_MAX_TRIES).
        Ohne gesetzten NEW_AGENT_MODE passiert nichts (Automatik aus)."""
        if not cfg.NEW_AGENT_MODE:
            return
        # Sobald die globale settings.json einen Start-Permission-Modus vorgibt
        # (permissions.defaultMode – vom Einstellungs-Fenster gesetzt), startet jeder
        # frische claude nativ in diesem Modus. Die Shift+Tab-Automatik wuerde von
        # MODE_START aus NOCHMAL weiterschalten und ueberschiessen -> hier aus.
        try:
            if cset.read_values().get("mode"):
                return
        except Exception:
            pass
        now = time.time()
        for slot in slots:
            prev = dp.load_json(dp.state_path(slot), {}) or {}
            self._pending_auto[slot] = {
                "base_ts": prev.get("ts", 0), "reg_ts": now,
                "ready_ts": 0.0, "sent_ts": 0.0, "tries": 0,
            }

    def _apply_pending_auto(self, states, now, cycle) -> None:
        """Neu per ＋ erzeugte Agenten in den Wunsch-Startmodus (config.NEW_AGENT_MODE,
        z.B. 'auto') treiben, sobald ihr erster Hook feuert (mit SessionStart-Hook beim
        Oeffnen, sonst beim ersten Prompt) – NICHT feuern-und-vergessen, sondern:

          1) Readiness-Gate: nach dem ersten frischen Hook erst cfg.AUTO_READY_GRACE warten,
             DANN blind ab MODE_START die noetigen Shift+Tab schicken. Der SessionStart-Hook
             feuert sehr frueh, oft bevor die Claude-TUI die Back-Tab-Sequenz verarbeitet ->
             ohne die kurze Wartezeit gehen einzelne Taps verloren und der Agent 'bleibt auf
             dem Weg haengen' (accept/plan statt auto).
          2) Bestaetigen/Nachfassen: der Slot bleibt NACH dem Senden vorgemerkt. Meldet ein
             Hook NACH sent_ts einen echten Ist-Modus (rep_ts > sent_ts – so faellt der
             leere/vererbte SessionStart-Modus bewusst raus), gilt: im Ziel -> fertig; kurz
             gelandet -> vom gemeldeten Ist-Modus die Rest-Taps nachschicken (bis
             cfg.AUTO_MAX_TRIES). Ohne echtes Signal (Agent im Leerlauf) bleibt der Blind-Antrieb
             stehen, bis cfg.PENDING_AUTO_TTL abgelaufen ist.

        Nur fuer im Deck angelegte Slots; sobald das Ziel EINMAL bestaetigt (oder das
        Zeitfenster zu) ist, wird der Slot vergessen -> ein manueller Moduswechsel danach
        bleibt unangetastet."""
        if not self._pending_auto:
            return
        target = cfg.NEW_AGENT_MODE
        if not (target and target in cycle):
            self._pending_auto.clear()             # nichts Sinnvolles zu tun (Automatik faktisch aus)
            return
        tgt_idx = cycle.index(target)
        start = cfg.MODE_START
        start_idx = cycle.index(start) if start in cycle else 0
        for slot, p in list(self._pending_auto.items()):
            if now - p["reg_ts"] > cfg.PENDING_AUTO_TTL:
                del self._pending_auto[slot]       # Zeitfenster abgelaufen -> Automatik aufgeben
                continue
            st = states.get(slot)
            # Erst arbeiten, wenn ein FRISCHER Zustand NACH dem Anlegen kam (Claude lebt).
            if not (st and st.get("ts", 0) > p["base_ts"]):
                continue                            # noch kein neuer Hook -> weiter warten
            if not p["ready_ts"]:
                p["ready_ts"] = now                 # ersten frischen Hook gesehen -> Readiness-Uhr starten

            if not p["sent_ts"]:
                # ── Erst-Antrieb: TUI warmlaufen lassen, dann bewusst ab MODE_START rechnen.
                # (SessionStart meldet KEINEN Modus; ein doch vorhandener koennte von einem
                # gleichnamigen Vorgaenger vererbt sein -> nicht darauf verlassen.) Erst bei
                # ERFOLGREICHEM Senden sent_ts setzen; scheitert es (Verbindungsabriss), bleibt
                # sent_ts=0 -> der naechste Poll versucht es erneut.
                if now - p["ready_ts"] < cfg.AUTO_READY_GRACE:
                    continue                        # noch in der Warmlaufzeit
                if self._set_slot_mode(slot, target, cycle, current=start_idx):
                    p["sent_ts"] = now
                    p["tries"] += 1
                continue

            # ── Bestaetigen/Nachfassen: nur ein echtes Ist-Signal NACH unserem Senden zaehlt.
            rmode = st.get("mode")
            if not (rmode in cycle and st.get("ts", 0) > p["sent_ts"]):
                continue                            # (noch) keine neue Ist-Meldung -> geduldig warten
            if cycle.index(rmode) == tgt_idx:
                del self._pending_auto[slot]        # im Ziel angekommen -> fertig
            elif p["tries"] >= cfg.AUTO_MAX_TRIES:
                del self._pending_auto[slot]        # gibt auf (falscher MODE_CYCLE/Account?)
            elif self._set_slot_mode(slot, target, cycle, current=cycle.index(rmode)):
                p["sent_ts"] = now                  # kurz gelandet -> vom Ist-Modus nachtreiben
                p["tries"] += 1

    def _mode_cycle(self) -> list[str]:
        """Shift+Tab-Reihenfolge der Agenten. Steht Bypass als Startmodus in
        settings.json, starten neue Agenten darin, und Claude Code nimmt Bypass in ihre
        Reihenfolge auf (sm.mode_cycle). Wechselt die Reihenfolge, zeigen die gemerkten
        Modus-Nummern auf die falschen Namen: dann vergessen, _adopt_hook_modes holt sie
        aus den letzten Hook-Meldungen neu."""
        cycle = sm.mode_cycle(cfg.MODE_CYCLE, cset.default_mode() == cset.BYPASS_MODE)
        if cycle != getattr(self, "_last_cycle", cycle):
            self.slot_mode.clear()
            self._mode_ts.clear()
        self._last_cycle = cycle
        return cycle

    def _adopt_hook_modes(self, states, cycle) -> None:
        """Ist-Permission-Mode aus den Hooks uebernehmen (self-correcting): jeder
        neue Hook-Event (neuere ts) mit gueltigem Modus setzt die Deck-Annahme."""
        for slot, st in states.items():
            got = sm.adopt_hook_mode(self._mode_ts.get(slot, 0), st, cycle)
            if got:
                self.slot_mode[slot], self._mode_ts[slot] = got
