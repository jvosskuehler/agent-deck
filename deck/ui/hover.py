"""Hover: Gruppe hervorheben und den Tooltip mit der Chat-Zusammenfassung zeigen.

Zwei Fallen: Enter/Leave feuern auch beim Wandern zwischen den Items EINER
Kachel (darum das verzögerte Ausblenden), und der Tooltip darf nie gegen
winfo_screenwidth geklemmt werden - das ist die Breite ALLER Monitore.
"""
import os
import tkinter as tk
from typing import Any

from deck import i18n
from deck.claude import summarize as cs
from deck.domain import config as cfg
from deck.platform import dpi
from deck.render.kit import INK, INK_3
from deck.ui.theme import PREFETCH_EVERY_S, RAIL_DIM, RAIL_HOT, RAIL_IDLE, TICKET_MAX_CHARS


class HoverMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def _highlight_group(self, win) -> None:
        """Den Repo-Block <win> hervorheben (None = alle in den Ruhezustand).

        Das ist die Antwort auf die eigentliche Frage beim Hovern: "zu welchem Repo
        gehoert die Karte unter dem Zeiger?". Statt sie im Tooltip zu BESCHREIBEN,
        antwortet die Gruppe selbst – ihre Schiene leuchtet auf, die fremden Bloecke
        treten zurueck. Angefasst werden nur Kopf und Schiene, NIE die Kacheln: deren
        Flaeche/Halo malt der GlowAnimator je Frame neu, ein Eingriff hier waere im
        naechsten Tick wieder ueberschrieben (und wuerde den Status-Kanal stoeren).

        Billig genug fuer jedes Enter (hoechstens len(cfg.WINDOWS) itemconfigs), und der
        _hot_win-Vergleich haelt den Wechsel zwischen Geschwisterkacheln kostenlos.

        Waehrend eines Zuges faellt die Hervorhebung ganz aus – und zwar HIER, an der
        einen Stelle, die faerbt. Beim Ziehen eines Blockkopfes verlaesst der Zeiger sein
        Item, und beim Ziehen wandern fremde Items unter den Zeiger; jedes dieser
        Leave/Enter-Paare wuerde sonst die angehobene Optik (BlockDrag._lift) sofort
        wieder wegraeumen. Nach dem Ablegen wird ohnehin neu gezeichnet, dort setzt
        _render_agents_slim auch _hot_win zurueck."""
        if self._dragging() or win == self._hot_win:
            return
        self._hot_win = win
        for w, it in self.win_items.items():
            dim = win is not None and w != win
            hot = win is not None and w == win
            try:
                self.deck.itemconfig(
                    it["name"],
                    fill=INK_3 if (dim or not it["connected"]) else INK)
                self.deck.itemconfig(
                    it["rail"],
                    fill=RAIL_HOT if hot else (RAIL_DIM if dim else RAIL_IDLE))
            except tk.TclError:
                pass          # Items schon weg (Redraw dazwischen) -> nichts zu tun

    def _head_enter(self, win) -> None:
        """Zeiger auf dem Repo-Namen. Der Kopf ist seit dem Klick-Binding kein bloßes
        Etikett mehr, also muss er sich auch so anfühlen: Handzeiger und derselbe
        Gruppen-Hinweis wie bei den Kacheln.

        Ein Kachel-Tooltip wird hier HART ausgeblendet (nicht verzögert wie in
        _hover_leave): wir sind sicher nicht mehr über einer Kachel – der Kopf ist ein
        eigenes Item, kein Nachbar unter demselben t_-Tag. _hide_prompt_tip nimmt dabei
        die Hervorhebung mit zurück, darum wird sie danach neu gesetzt."""
        if self._dragging():
            return                    # beim Umsortieren nichts umfärben
        self._hide_prompt_tip()
        self._highlight_group(win)
        self.deck.configure(cursor="hand2")

    def _head_leave(self) -> None:
        """Zeiger verlässt den Repo-Namen: Hervorhebung und Zeiger zurück. Sofort und
        ohne Timer – anders als bei den Kacheln gibt es hier keine gestapelten Items,
        zwischen denen Tk ein Leave/Enter-Paar feuern könnte.

        Beim Ziehen des Blocks feuert das Leave trotzdem (der Kopf wandert unter dem
        Zeiger weg), und dann ist es keine Aussage über den Zeiger, sondern eine Folge
        der Bewegung – Zeigerform und Hervorhebung gehören für die Dauer des Zuges
        BlockDrag."""
        if self._dragging():
            return
        self._highlight_group(None)
        self.deck.configure(cursor="")

    def _hover_enter(self, slot) -> None:
        """Zeiger betritt ein Item der Kachel. Wegen des geteilten t_-Tags feuert Tk das
        AUCH beim Wechsel zwischen den gestapelten Items DERSELBEN Kachel – dann ist
        slot == _hover_slot und wir tun nichts (kein Timer-Neustart, kein Flackern). Ein
        zuvor per Leave geplantes Ausblenden wird immer abgebrochen (wir sind ja noch
        drueber)."""
        if self._dragging():
            return                    # beim Umsortieren kein Frage-Tooltip aufpoppen
        self._cancel_tip_hide()
        if slot == self._hover_slot:
            return
        self._hover_slot = slot
        # SOFORT, nicht erst mit dem Tooltip nach cfg.HOVER_TIP_MS: die Zugehoerigkeit ist
        # die Frage, die man beim blossen Drueberfahren hat.
        self._highlight_group(slot[0])
        self._cancel_tip_show()
        self._tip_show_job = self.root.after(
            cfg.HOVER_TIP_MS, lambda s=slot: self._show_prompt_tip(s))

    def _hover_leave(self) -> None:
        """Zeiger verlaesst ein Item der Kachel. Feuert auch beim Wechsel auf ein Nachbar-
        Item DERSELBEN Kachel -> NICHT sofort ausblenden, sondern verzoegert: ein unmittel-
        bar folgendes _hover_enter derselben Kachel bricht das Ausblenden ab. Bleibt es aus
        (echtes Verlassen), verschwindet der Tooltip nach cfg.TIP_LEAVE_MS."""
        self._cancel_tip_hide()
        self._tip_hide_job = self.root.after(cfg.TIP_LEAVE_MS, self._do_hide_tip)

    def _do_hide_tip(self) -> None:
        """Verzoegertes Ausblenden faellig -> wir sind wirklich weg von der Kachel:
        geplanten Show abbrechen, Hover-Zustand loeschen, Tooltip verstecken."""
        self._tip_hide_job = None
        self._cancel_tip_show()
        self._hover_slot = None
        self._tip_visible = False
        self._highlight_group(None)
        self.prompt_tip.hide()

    def _cancel_tip_show(self) -> None:
        if self._tip_show_job is not None:
            try:
                self.root.after_cancel(self._tip_show_job)
            except Exception:
                pass
            self._tip_show_job = None

    def _cancel_tip_hide(self) -> None:
        if self._tip_hide_job is not None:
            try:
                self.root.after_cancel(self._tip_hide_job)
            except Exception:
                pass
            self._tip_hide_job = None

    def _hide_prompt_tip(self, *, keep_hover=False) -> None:
        """Tooltip hart ausblenden (Klick / Neu-Rendern / Fokusverlust): beide Timer weg,
        Tooltip versteckt. keep_hover=True behaelt _hover_slot -> ein durch die Klick-
        Animation (Skalieren der Kachel-Items) ausgeloestes erneutes Enter DERSELBEN Kachel
        wird von _hover_enter ignoriert, der Tooltip ploppt also NICHT ueber dem nach vorn
        geholten VS-Code-Fenster wieder auf. Sonst _hover_slot loeschen."""
        self._cancel_tip_show()
        self._cancel_tip_hide()
        if not keep_hover:
            self._hover_slot = None
        self._tip_visible = False
        # Auch bei keep_hover: die Hervorhebung ist ein Hinweis auf die Karte unter dem
        # Zeiger. Steht das Deck nicht mehr vorn (Klick auf eine Kachel holt VS Code
        # nach vorn), darf sie nicht als Rest stehenbleiben.
        self._highlight_group(None)
        self.prompt_tip.hide()

    def _on_focus_out(self, _e) -> None:
        """Ganze App hat den Fokus verloren (z.B. Alt+Tab OHNE Mausbewegung -> es feuert
        kein <Leave>): sonst bliebe ein sichtbarer Tooltip ueber dem neuen Fenster haengen.
        focus_displayof() ist None NUR, wenn der Fokus wirklich aus der App raus ist (nicht
        bei Fokuswechsel zwischen eigenen Widgets) -> kein Show/Hide-Flattern. keep_hover=
        True: der Zeiger steht bei Alt+Tab / beim Nach-vorn-Holen von VS Code (focus_slot)
        weiterhin PHYSISCH ueber der Kachel -> _hover_slot behalten, sonst wuerde die Klick-
        Animation den Tooltip ueber dem VS-Code-Fenster erneut aufpoppen lassen. Ein echtes
        <Leave> (Zeiger verlaesst die Kachel) raeumt _hover_slot ohnehin auf."""
        try:
            if self.root.focus_displayof() is None:
                self._hide_prompt_tip(keep_hover=True)
        except (tk.TclError, KeyError):
            pass

    def _tip_refs(self, sid) -> Any:
        """Im Chat erkannte Bezuege dieser Session: {"ticket": …, "pr": …} (leer =
        keine/Erkennung aus). Erst aus dem In-Memory-Cache (vom Hintergrund-Job
        gefuellt), sonst EINMAL aus der Cache-Datei nachladen – die ueberlebt einen
        Deck-Neustart, der Hover zeigt die IDs also sofort und nicht erst nach dem
        naechsten Scan."""
        if not (cfg.TICKET_AUTODETECT and sid):
            return {"ticket": "", "pr": ""}
        refs = self._auto_refs.get(sid)
        if refs is None:
            refs = cs.cached_refs(sid)
            self._auto_refs[sid] = refs
        return refs

    @staticmethod
    def _refs_label(refs) -> Any:
        """Bezugs-Zeile fuer den Tooltip ("Ticket: ABC-1 · PR #62"); ohne beides ein
        leerer String."""
        parts = []
        if refs.get("ticket"):
            parts.append("Ticket: " + refs["ticket"])
        if refs.get("pr"):
            parts.append("PR #" + refs["pr"])
        return " · ".join(parts)

    @staticmethod
    def _refs_card_label(refs, max_chars=TICKET_MAX_CHARS) -> Any:
        """Kompakte Fassung derselben Bezuege fuer die KARTE: "PROJ-2691 #62" – ohne das
        Wort 'Ticket' (die Zeile ist an ihrem Platz erkennbar) und nur, solange beides
        nebeneinander passt; sonst gewinnt das Ticket, weil es das Dauerhaftere ist."""
        refs = refs or {}
        tid, pr = refs.get("ticket") or "", refs.get("pr") or ""
        both = " ".join(p for p in (tid, ("#" + pr) if pr else "") if p)
        if len(both) <= max_chars:
            return both
        return tid or both[:max_chars - 1] + "…"

    def _origin_lines(self, slot) -> Any:
        """Herkunft dieser Kachel als Tooltip-Kopf: "agent-deck · Fenster A · A2" und –
        wenn der Agent per Ticket in einem eigenen worktree sitzt – darunter "↳ wt/<slug>".

        Warum ueberhaupt: die Zusammenfassung sagt, WORUM es geht, aber nie WO. Bei
        mehreren offenen Repos ist genau das die Frage am Tooltip. Und der worktree-Fall
        ist ohne diese Zeile gar nicht sichtbar: der Blockkopf nennt das Repo, der Agent
        arbeitet aber in '<repo>.wt/<slug>' daneben.
        Ohne gebundenes Repo bleibt es beim Fensterbuchstaben (mehr wissen wir dann nicht)."""
        if not slot:
            return []
        win = slot[0]
        repo = self.bindings.get(win) or ""
        parts = [p for p in (repo, f"{i18n.L('Fenster', 'Window')} {win}", slot) if p]
        lines = [" · ".join(parts)]
        wt = self._worktrees.get(slot) or ""
        if wt:
            lines.append("↳ wt/" + os.path.basename(os.path.normpath(wt)))
        return lines

    def _tip_text(self, ids, sid, slot="") -> Any:
        """Text des Hover-Tooltips zusammenbauen: zuerst die Herkunft (Repo/Fenster/Slot,
        siehe _origin_lines), dann erkanntes Ticket / erkannter PR (was im
        Chat steht) und darunter die KI-Kurzzusammenfassung 'worum es geht' bzw. – solange
        die noch erzeugt wird – ein Platzhalter. Bei HOVER_SUMMARY=False bleibt es bei der
        bisherigen 'Letzten Frage', Herkunft und Bezugs-Zeile kommen trotzdem obendrueber.
        Leerer Rueckgabewert -> nichts zu zeigen."""
        lines = self._origin_lines(slot)
        head = self._refs_label(self._tip_refs(sid))
        if head:
            lines.append(head)
        if not cfg.HOVER_SUMMARY:
            text = ids.get("prompt") or ""
            if text:
                lines.append(i18n.L("Letzte Frage:\n", "Last question:\n") + text)
            return "\n".join(lines)
        summary = cs.cached_summary(sid) if sid else None
        if summary:
            lines.append(i18n.L("Worum es geht:\n", "What it's about:\n") + summary)
        elif sid:
            lines.append(i18n.L("Zusammenfassung wird erstellt …", "Generating summary …"))
        return "\n".join(lines)

    def _show_prompt_tip(self, slot) -> None:
        """Show-Timer abgelaufen -> Tooltip zeigen (Inhalt siehe _tip_text) und im
        Hintergrund Ticket/Zusammenfassung sicherstellen; was dabei neu dazukommt, zieht
        _chat_info_ready live nach. Nur, wenn die Kachel noch gehovert ist."""
        self._tip_show_job = None
        ids = self.tiles.get(slot)
        if not ids or self._hover_slot != slot:   # inzwischen weg / andere Kachel
            return
        sid = ids.get("session_id") or ""
        text = self._tip_text(ids, sid, slot)
        if text:
            self._tip_at_pointer(text)
        if sid:
            self._ensure_chat_info(sid, ids.get("cwd") or "")
        # Ohne sid (Agent verbunden, aber noch kein Hook) gibt es nichts zu HOLEN – die
        # Herkunftszeile steht trotzdem, die kennt das Deck aus sich selbst.

    def _tip_at_pointer(self, text) -> None:
        """Tooltip mit text leicht unter/rechts vom Mauszeiger zeigen. Zeiger-Koordinaten
        (winfo_pointer*) statt Canvas-Offset: gleiches Schirm-Koordinatensystem wie
        wm_geometry -> korrekt ueber mehrere Monitore und bei DPI-Skalierung.

        Der Versatz geht als dx/dy MIT (statt vorher aufaddiert): so kann der Tooltip am
        Bildschirmrand nach links/oben um den Zeiger klappen, statt teilweise jenseits
        des Monitors zu landen – und am rechts angedockten Deck ist genau das der
        Normalfall (siehe screen_fit)."""
        self.prompt_tip.show(self.root.winfo_pointerx(), self.root.winfo_pointery(),
                             text, dx=dpi.px(14), dy=dpi.px(18))
        self._tip_visible = True

    def _prefetch_summaries(self, now) -> None:
        """Chat-Infos offener Agenten schon VOR dem Hover holen -> der Hover ist dann
        sofort da (ein claude-Aufruf dauert ~8-13 s, fast nur CLI-Startup) und die
        erkannte Ticket-ID steht ohne Hover auf der Karte. Gedrosselt auf alle
        PREFETCH_EVERY_S, damit nicht jeder 400-ms-Poll Threads spawnt; die eigentliche
        Arbeit (und ob ueberhaupt neu erzeugt wird) entscheiden chat_summary.
        ensure_refs/generate (Cache- + Wachstums-/Cooldown-Gate, Concurrency-Cap).
        Der Ticket-Scan laeuft auch ohne HOVER_SUMMARY_PREFETCH (kostet nichts ausser
        dem Lesen des Transcripts)."""
        if not (cfg.TICKET_AUTODETECT or (cfg.HOVER_SUMMARY and cfg.HOVER_SUMMARY_PREFETCH)):
            return
        if now - self._last_prefetch < PREFETCH_EVERY_S:
            return
        self._last_prefetch = now
        for ids in self.tiles.values():
            sid = ids.get("session_id") or ""
            if sid and sid not in self._summary_jobs:
                self._ensure_chat_info(sid, ids.get("cwd") or "",
                                       summary=cfg.HOVER_SUMMARY and cfg.HOVER_SUMMARY_PREFETCH)
