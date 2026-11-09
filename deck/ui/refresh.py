"""Der Poll-Takt: Slot-Zustände lesen, Kacheln nachziehen, Bindungen pflegen.

Der „gesehen“-Merker muss über den Poll hinaus halten - in der State-Datei
steht weiterhin done, sonst leuchtet eine gelesene Antwort wieder auf.
"""
import time

from deck import i18n
from deck.domain import config as cfg
from deck.domain import prompt_cache as pc
from deck.domain import slot_state as dc
from deck.domain import status_model as sm
from deck.ops import instance as si
from deck.render.kit import CARD_BORDER, CARD_FILL, INK_3
from deck.render.kit import mix as _mix
from deck.render.kit import short_model as _short_model
from deck.ui.theme import (
    CACHE_INK,
    GLOW_STYLE,
    LOST_FILL,
    LOST_GLOW,
    SEL_BORDER,
    SLIDE_RETRY_MS,
    TICKET_AUTO_INK,
    TICKET_INK,
    TICKET_MAX_CHARS,
    WAIT_BORDER,
    status_label,
)


class RefreshMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def refresh(self) -> None:
        """Poll-Schleife (alle cfg.POLL_MS): Zweitstart-Wunsch bedienen, Verbindungen
        synchronisieren, Layout bei Bedarf neu zeichnen, neuen Chat auto-fokussieren,
        dann Zustaende einlesen und jede Kachel aktualisieren. In benannte Schritte
        zerlegt (jeweils unten)."""
        self._beat()        # VOR jedem vorzeitigen return: sonst gilt ein langes
                            # Kachel-Ziehen dem Waechter als haengendes Panel
        # Laeuft ein VS-Code-Neustart, wird er hier mitgeschoben - auch dann, wenn
        # dieser Poll gleich vorzeitig aussteigt (siehe unten). `step()` ist
        # idempotent und entscheidet selbst, ob sein Zeitfenster offen ist.
        self.restarter.step()
        restarting = self.restarter.busy()
        if self._dragging():
            # Waehrend eines laufenden Kachel-Drags NICHT neu zeichnen (c.delete("all")
            # wuerde das Ziehen zerreissen). Poll pausiert kurz; _tile_release zeichnet
            # danach sauber neu. Timer aber weiterlaufen lassen.
            self.root.after(cfg.POLL_MS, self.refresh)
            return
        if self.dock is not None and self.dock.sliding():
            # Das Deck gleitet gerade an den Rand oder heraus. Diese Bewegung laeuft im
            # SELBEN Thread wie dieser Poll – und ein Durchlauf kostet gemessen ~7 ms,
            # mit kalten Zustandsdateien ueber 40. Das sind ein bis vier ausgefallene
            # Bilder mitten in einer nur ~270 ms kurzen Bewegung, also genau die Art
            # Ruckler, die man dem Rechner zuschreibt. Der Kachel-Animator wird aus
            # demselben Grund fuer die Dauer des Slides angehalten
            # (edge_dock._anim_hold); dieser Poll war der letzte Mitbewerber.
            #
            # Kurz nachfassen statt cfg.POLL_MS abzuwarten: die Bewegung ist gleich vorbei,
            # und danach soll die Anzeige unverzueglich stimmen. Der Leerlauf-Durchlauf
            # kostet nichts, und haengen kann das nicht – ein Slide endet garantiert
            # (Notbremse + Watchdog in edge_dock).
            self.root.after(SLIDE_RETRY_MS, self.refresh)
            return
        self._serve_reveal_request()    # ein zweiter Programmstart will uns sehen
        # Auf einen anders skalierten Monitor geschoben? Dann Oberflaechenfaktor
        # nachziehen (zeichnet selbst neu und passt die Fenstergroesse an).
        self._sync_ui_scale()
        self._sync_bindings()
        # Waehrend eines Neustarts NICHT abraeumen: die Fenster sind absichtlich weg.
        # _cleanup_closed_windows wuerde nach cfg.STALE_WINDOW_S die Bindungen
        # vergessen - dann bekaeme ein Fenster einen anderen Buchstaben als im Plan,
        # und die Chats landeten auf fremden Kacheln.
        if not restarting:
            self._cleanup_closed_windows()
        sig = self._layout_sig()
        if sig != self._last_sig:                 # nur bei Aenderung neu zeichnen (Flackern)
            self._render_agents()
            self._last_sig = sig
        self._autofocus_new()
        self._mark_seen_read()          # in VS Code angeklickte Panes: 'ungelesen' -> 'idle'
        states = dc.read_all()
        live = dc.read_live()
        self._found = dc.read_found_tickets()   # vom Agenten gemeldete Ticket-IDs (Such-Modus)
        self._worktrees = dc.read_found_worktrees()  # gemeldete worktree-Pfade (Ticket-Anzeige + Orphan-Sweep haengen daran)
        now = time.time()
        cycle = self._mode_cycle()
        if not restarting:
            # Gleiche Begruendung wie oben, mit hoeherem Preis: waehrend eines Neustarts
            # lebt zu KEINEM Slot ein Agent, der Sweep hielte also jeden Ticket-worktree
            # fuer verwaist und wuerde ihn loeschen - samt der Arbeit darin.
            self._sweep_orphan_worktrees(now)       # (marker-getrieben) worktrees ohne lebenden Agenten abraeumen
            self._sweep_disk_worktrees(now, states) # (fs-getrieben) verwaiste '<repo>.wt'-worktrees OHNE Marker abraeumen
        self._adopt_hook_modes(states, cycle)
        self._apply_pending_auto(states, now, cycle)
        self._update_tiles(states, live, now, cycle)
        if not restarting:
            self.compactor.step(list(self.tiles), states, live, now)   # Cache laeuft gleich ab
        self._prefetch_summaries(now)   # Ticket-ID + Zusammenfassung im Hintergrund vorwaermen
        self.root.after(cfg.POLL_MS, self.refresh)

    def _beat(self) -> None:
        """Lebenszeichen fuer den Waechter (watchdog.py), gedrosselt auf BEAT_EVERY_S.

        Bewusst hier in der Poll-Schleife und nicht in einem eigenen Timer: der
        Herzschlag soll genau das bezeugen, was zaehlt – dass refresh() noch laeuft.
        Ein Panel, dessen Prozess lebt, dessen Schleife aber steht, ist fuer den
        Nutzer genauso tot wie ein abgestuerztes."""
        now = time.time()
        if now - self._last_beat < si.BEAT_EVERY_S:
            return
        self._last_beat = now
        si.beat()

    def _serve_reveal_request(self) -> None:
        """Den Wunsch eines zurueckgetretenen Zweitstarts bedienen: »zeig dich«.

        Ein erneuter Programmstart oeffnet absichtlich KEIN zweites Panel
        (single_instance) und holt stattdessen dieses hier nach vorn. Am Rand
        angedockt war davon nichts zu sehen: sichtbar ist dann nur der 12 px
        schmale Griff, fokussiert wurde also genau der – fuer den Nutzer sah der
        zweite Start damit aus wie »das Deck laesst sich nicht mehr oeffnen«.
        Also hier aufklappen (mit Haltefrist, der Zeiger steht ja nicht auf dem
        Deck); schwebend genuegt nach vorn holen."""
        if not si.take_reveal_request():
            return
        if self.dock is not None and self.dock.current_edge() != "off":
            self.dock.reveal_for_request()
            return
        try:                            # schwebend: evtl. minimiert/verdeckt
            self.root.deiconify()
            self.root.lift()
        except Exception:
            pass

    def _mark_seen_read(self) -> None:
        """Panes, die du direkt in VS Code angeklickt hast, als gelesen markieren.
        Die Extension meldet solche Fokuswechsel als 'seen' an den Broker; hier holen
        wir die Slots ab und schalten 'ungelesen' (done) -> 'idle' – dieselbe Geste
        wie ein Klick auf die Deck-Kachel (siehe focus_slot). Nur done wird angefasst;
        ein denkender/laufender Agent bleibt unberuehrt. Vor read_all(), damit dieselbe
        Poll-Runde die Kachel schon grau statt gruen zeichnet."""
        seen = self.broker.drain_seen()
        if not seen:
            return
        states = dc.read_all()
        for slot in seen:
            st = states.get(slot)
            if st and st.get("status") == "done":
                dc.write_state(slot, "idle")
        # Die Kachel des Panes, das du gerade vorgeholt hast, wird zur ausgewaehlten -
        # so zeigt das Deck immer, wo deine Tastatur gerade ist, auch wenn du direkt im
        # Terminal gewechselt hast statt ueber die Kachel. Bei mehreren Meldungen in
        # einem Takt gewinnt eine bekannte Kachel; die Reihenfolge ist dann egal.
        shown = [s for s in sorted(seen) if s in self.tiles]
        if shown:
            self.active_slot = shown[-1]

    def _update_tiles(self, states, live, now, cycle) -> None:
        """Pro Kachel den Status interpretieren (status_model) und die Optik setzen:
        Glow-Ziele + Farbton-Ziel, betonte Kante, Modell/Effort/Status/Modus-Text.
        Das Faden/Atmen selbst macht der GlowAnimator (wir setzen nur die Ziele)."""
        skeys = []
        for slot, ids in self.tiles.items():
            st = states.get(slot)
            lv = live.get(slot) or {}
            # Ohne Meldung heisst gerendert = verbundener Agent -> "idle" (hell). Nur bei
            # einem Remote-Fenster nicht: dessen Hooks schreiben drueben, hier kaeme auch
            # eine Rueckfrage nie an -> "unbekannt" statt einer beruhigenden Behauptung.
            status = sm.initial_status(st, bool(self.broker.remote(slot[0])))
            fresh = sm.is_fresh(st, now, cfg.STALE_S)
            status = sm.normalize_status(status, fresh, GLOW_STYLE)
            lost = sm.is_lost(status, fresh, self.broker.connected(slot[0]))
            label = i18n.L("getrennt", "disconnected") if lost else status_label(status)
            # Status-Glow + Farbton der Karte (rot = im Panel berechneter Verlust).
            if lost:
                gcolor, gintensity, gpulse, gfill = LOST_GLOW, 1.0, False, LOST_FILL
            else:
                gcolor, gintensity, gpulse, gfill = GLOW_STYLE[status]
            ids["glow_color"] = gcolor
            ids["glow_intensity"] = gintensity
            ids["glow_pulse"] = gpulse
            ids["fill_target"] = _mix(CARD_FILL, gcolor, gfill)   # Ziel-Tönung der Fläche
            # Statuswechsel -> kurzes Aufleuchten (bloom); der Farbton fadet im Animator.
            skey = "lost" if lost else status
            skeys.append(skey)
            prev_skey = ids.get("status_key")
            if prev_skey != skey:
                ids["status_key"] = skey
                # KEIN Bloom, wenn eine "ungelesene" Antwort nur als gelesen quittiert
                # wird (done -> idle): das ist deine eigene Geste (Klick aufs Deck bzw.
                # Pane-Fokus in VS Code), kein neuer Agent-Zustand. Der Bloom (~1,25 s
                # Abkling) legt sich sonst additiv auf den Klick-Surge und laesst den Halo
                # ~1 s "ausrasten", obwohl die Kachel gerade RUHIGER wird. Das Klick-
                # Feedback traegt bereits surge() (0,4 s); nur echte Zustandswechsel leuchten.
                if not (prev_skey == "done" and skey == "idle"):
                    ids["bloom"] = cfg.BLOOM_ON_CHANGE
            # Kartenkante: Auswahl / Rückfrage / Verlust betont, sonst dezent getönt.
            if slot == self.active_slot:
                border, bw = SEL_BORDER, 2                # ausgewählte Kachel
            elif lost:
                border, bw = LOST_GLOW, 2                 # Verbindung weg fällt auf
            elif status == "waiting":
                border, bw = WAIT_BORDER, 2               # Rückfrage fällt auf
            else:
                border, bw = _mix(gcolor, CARD_BORDER, 0.5), 1
            # Karteninhalt: Modell (statusLine) + Effort (Hooks) + Status unten links.
            # Textfarben sind fest (INK/INK_2) – der Status läuft über den Glow.
            model = _short_model(lv.get("model") or "")
            live_eff = lv.get("effort") or (st or {}).get("effort") or ""
            effort = sm.resolve_effort(live_eff, self.slot_effort.get(slot))
            mi = self.slot_mode.get(slot)
            mode = cycle[mi] if (mi is not None and mi < len(cycle)) else ((st or {}).get("mode") or "")
            # Hover-Tooltip: letzte Frage (Fallback) + Transcript-Adresse der Session.
            ids["prompt"] = (st.get("prompt") if st else "") or ""
            ids["session_id"] = (st.get("session_id") if st else "") or ""
            ids["cwd"] = (st.get("cwd") if st else "") or ""

            # Bezugs-Zeile auf der Karte, zwei Quellen mit unterschiedlicher Verbindlichkeit:
            #  1) ZUGEWIESEN (manuell self.tickets, im Such-Modus gemeldet self._found) –
            #     nur mit worktree-Marker (state/<slot>.worktree), denn erst dann ist das
            #     Ticket wirklich an den Agenten gebunden. Volles Violett.
            #  2) ERKANNT: Ticket und/oder PR, die chat_summary aus dem Transcript gelesen
            #     hat (_auto_refs, vom Hintergrund-Job gefuellt). Kein worktree dahinter ->
            #     gedimmt, und nur solange keine zugewiesene ID die Zeile belegt.
            # Platz ist knapp (rechts steht das Effort): beides zusammen nur, wenn es in
            # TICKET_MAX_CHARS passt, sonst gewinnt das Ticket; ein zu langer Rest wird
            # abgeschnitten.
            tink = TICKET_INK
            if slot in self._worktrees:
                tid = self.tickets.get(slot) or self._found.get(slot, "")
            else:
                tid = ""
            if not tid and cfg.TICKET_AUTODETECT_ON_CARD:
                tid = self._refs_card_label(self._auto_refs.get(ids["session_id"]))
                tink = TICKET_AUTO_INK
            if len(tid) > TICKET_MAX_CHARS:
                tid = tid[:TICKET_MAX_CHARS - 1] + "…"
            # fill NICHT hier setzen – die Fläche fadet im Animator zur fill_target.
            # Die Kante geht durch den Animator: im Bildmodus wird sie mitgerendert
            # (weiche Rundung), im Fallback bleibt es das Polygon-Outline.
            self.anim.set_border(ids, border, bw)
            # Erst jetzt malen: Glow-Ziele UND Kante stehen: im Bildmodus wird die
            # Kachel damit in EINEM Durchgang fertig (statt zweimal je Poll).
            self.anim.apply_glow(slot, self.anim.pulse_factor())
            self.deck.itemconfig(ids["model"], text=model)
            self.deck.itemconfig(ids["effort"], text=(effort if effort else ""))
            # Prompt-Cache: Ablaufzeit schreibt die Statuszeile in live.json, gezaehlt
            # wird hier gegen die eigene Uhr. So laeuft der Countdown im Poll-Takt des
            # Decks weiter, auch wenn die Statuszeile gerade nicht neu gezeichnet wird.
            # Typpruefung, weil die Datei von einem anderen Prozess kommt: ein Fehler
            # hier haelte die ganze Poll-Schleife an, nicht nur diese Zeile.
            until, ttl = lv.get("cache_until"), lv.get("cache_ttl")
            if (isinstance(until, (int, float)) and isinstance(ttl, (int, float)) and ttl > 0
                    and not pc.working(st, now)):          # arbeitet er, laeuft kein Timer
                ctext, clevel = pc.view(until - now, ttl, i18n.L("kalt", "cold"))
                self.deck.itemconfig(ids["cache"], text=ctext, fill=CACHE_INK[clevel])
            else:
                self.deck.itemconfig(ids["cache"], text="")
            # Mit Ticket -> die ID im Violett (zugewiesen) bzw. gedimmt (nur erkannt);
            # ohne Ticket bleibt die Zeile leer (Zuweisung laeuft ueber das Rechtsklick-
            # Menue -> kein Platzhalter).
            if tid:
                self.deck.itemconfig(ids["ticket"], text=tid, fill=tink)
            else:
                self.deck.itemconfig(ids["ticket"], text="", fill=INK_3)
            self.deck.itemconfig(ids["act"], text=label)   # unten links: nur der Status
            self.deck.itemconfig(ids["mode"], text=mode)
        self._update_dock_glow(skeys)

    def _update_dock_glow(self, skeys) -> None:
        """Den Griff-Balken des angedockten Decks in der Farbe des DRINGLICHSTEN
        Kachel-Status leuchten lassen (Rueckfrage > ungelesen > getrennt > denkt >
        idle): eingeklappt sieht man so am Rand, ob einer etwas von dir will.

        Die Farben bleiben hier (GLOW_STYLE/LOST_GLOW = eine Quelle fuer Kacheln UND
        Griff); das Dock bekommt nur Farbe/Intensitaet/Puls und kennt keine Status.
        Der Blitz beim Wechsel entscheidet sich ebenfalls hier, weil nur das Panel
        weiss, ob der neue Zustand dringlicher ist (sm.escalated) – 'ungelesen ->
        idle' ist deine Lese-Quittung und blitzt darum bewusst nicht."""
        if self.dock is None:
            return
        key = sm.dominant_status(skeys)
        prev = self._dock_key
        self._dock_key = key
        if key == "lost":
            color, intensity, pulse = LOST_GLOW, 1.0, False
        else:
            color, intensity, pulse, _fill = GLOW_STYLE[key]
        self.dock.set_glow(color, intensity, pulse,
                           flash=(prev is not None and sm.escalated(prev, key)))
