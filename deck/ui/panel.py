"""Agent Deck - Dashboard fuer Claude-Agents in VS-Code-Fenstern.

- Startet OHNE Agent-Kacheln. Pro verbundenem Fenster erscheint dynamisch eine
  Kachel je offenem Claude-Terminal + eine "＋"-Kachel fuer einen neuen Chat.
- Kachelfarbe = Live-Status (aus den Hook-Meldungen, state/<slot>.json)
- Klick auf "Fenster A/B" -> danach das VS-Code-Fenster anklicken = verbinden
  (Repo-Name wird gemerkt/angezeigt; nochmal klicken = neu verbinden)
- Klick auf Kachel -> Fenster nach vorn (Win32) + Pane fokussiert (Extension)
- Klick auf "＋" -> die Extension oeffnet ein weiteres Claude-Terminal
- Aktions-Buttons -> Kommandos an die Extension (kein Fokus-Klau)

Architektur: STATUS = Hooks -> State-Files (dieses Panel liest sie).
             ACTIONS/FOCUS = ueber den Broker an die VS-Code-Extension.
             Win32 nur noch, um das richtige der 2 Fenster nach vorn zu holen.

Start:  python agent_deck.py

Abhaengigkeiten: Stdlib. Optional Pillow – damit werden Kachelflaeche und Halo
gerendert statt als Canvas-Polygon gezeichnet (weiche Rundungen; Tk-Canvas kann
kein Antialiasing, siehe card_render.py). Fehlt Pillow, faellt das Deck
automatisch auf den bisherigen Polygon-Weg zurueck und laeuft normal weiter.
"""
import os
import queue
import subprocess
import sys
import threading
import tkinter as tk
import tkinter.font as tkfont
from typing import Any

from deck import i18n
from deck.claude import summarize as cs
from deck.dock.controller import EdgeDock
from deck.domain import config as cfg
from deck.domain import paths as dp
from deck.domain.binding import BindStore
from deck.net.broker import Broker
from deck.net.commands import BrokerCommands
from deck.ops import instance as si
from deck.ops import log
from deck.ops import vscode_glow as rg
from deck.platform import dpi
from deck.platform import focus as wf
from deck.render import kit as ck
from deck.render.glow import GlowAnimator
from deck.render.kit import BG
from deck.ui.actions import ActionsMixin
from deck.ui.connect import ConnectMixin
from deck.ui.hover import HoverMixin
from deck.ui.layout import LayoutMixin
from deck.ui.refresh import RefreshMixin
from deck.ui.reorder import TileDrag, bind_dragging
from deck.ui.reorder_blocks import BlockDrag
from deck.ui.settings_dialog import SettingsDialog
from deck.ui.ticket import TicketMixin
from deck.ui.tile_draw import TileRenderer
from deck.ui.tiles import TilesMixin
from deck.ui.uithread import UiThreadMixin
from deck.ui.windows import WindowSyncMixin
from deck.ui.worktree_sweep import WorktreeSweepMixin


class AgentDeck(
        # Fenstergroesse und Skalierung
        LayoutMixin,
        # Kacheln: anordnen, zeichnen, umsortieren
        TilesMixin,
        # Interaktion: Hover-Tooltip, Fenster binden, Klick-Wirkungen
        HoverMixin, ConnectMixin, ActionsMixin,
        # Takt: Slot-Zustände lesen, Fenster/Slots pflegen, Thread-Rückweg
        RefreshMixin, WindowSyncMixin, UiThreadMixin,
        # Ticket-Arbeit und Dialoge
        TicketMixin, WorktreeSweepMixin,
):
    def __init__(self) -> None:
        self.active_slot = None
        self._await_new = None         # (win, slots-vorher, ts) – neuen "＋"-Chat auto-fokussieren
        self.slot_mode = {}            # Slot -> Permission-Mode-Index (Ist aus Hooks, sonst Annahme)
        self._mode_ts = {}             # Slot -> ts des zuletzt uebernommenen Hook-Modus
        self._pending_auto = {}        # Slot -> Fortschritts-Dict: neu per ＋ angelegt, wartet/treibt auf Auto-Startmodus (siehe _register_pending_auto)
        # Persistenz (bindings.json + slot_effort.json) liegt in BindStore; wir
        # halten die Dicts direkt und mutieren sie in place, danach store.save_*().
        self.store = BindStore()
        self.bindings = self.store.bindings       # {"A": repo, "B": repo}
        self.slot_effort = self.store.effort      # Slot -> gemerktes Effort ("xhigh"/"ultracode")
        self.tickets = self.store.tickets         # Slot -> manuell zugewiesene Ticket-ID
        self.order = self.store.order             # {win: [slot, …]} vom Nutzer gezogene Reihenfolge
        self.win_order = self.store.win_order     # [repo, …] gezogene Reihenfolge der Repo-Bloecke
        self._found = {}                          # Slot -> vom Agenten gemeldete ID (state/<slot>.ticket)
        self._worktrees = {}                      # Slot -> gemeldeter worktree-Pfad (state/<slot>.worktree); Ticket-Anzeige haengt daran
        self._wt_gone_since = {}                  # Slot -> ts, seit wann worktree-Marker ohne lebenden Agenten (Orphan-Grace)
        self._wt_disk_gone_since = {}             # worktree-Pfad(normcase) -> ts, seit wann als verwaister '.wt'-Ordner gesehen (Disk-Sweep-Grace)
        self._known_repos = set()                 # je in dieser Session gesehene Repo-Roots (aus cwd/Marker) -> deren '<repo>.wt' wird gefegt
        self._last_disk_sweep = 0.0               # ts des letzten Disk-Sweeps (Throttle auf WT_DISK_SWEEP_INTERVAL_S)
        self._disk_sweep_busy = False             # laeuft gerade ein Disk-Sweep-Thread? -> keinen zweiten parallel starten
        self.settings = self.store.settings       # Panel-Einstellungen (persistent)
        i18n.refresh()                             # Deck-Sprache aus settings.json (english/german) lesen
        self._modal = False            # True, solange der Ticket-/Einstellungs-Dialog offen ist (pausiert Auto-Fokus)
        self.binding_group = None      # "A"/"B" waehrend "klick-zum-Verbinden"
        self._bind_deadline = 0
        self._gone_since = {}          # Fenster -> ts, seit wann getrennt UND VS-Code-Fenster zu (Auto-Abraeumen)

        self.broker = Broker(cfg.BROKER_HOST, cfg.BROKER_PORT)
        self.broker.start()
        self.cmds = BrokerCommands(self.broker)   # typisierte Fassade fuers Senden

        # HiDPI: MUSS vor dem ersten Tk-Aufruf stehen. Ohne das zeichnet Tk in
        # logischen Pixeln und Windows streckt das fertige Fensterbild auf die
        # echte Aufloesung – dann ist alles weich und die runden Ecken treppen
        # (siehe dpi.py). Der Oberflaechenfaktor holt die Groesse zurueck:
        # gezeichnet wird ab jetzt in Geraetepixeln, aber um 1.5 groesser.
        # Vorlaeufig die System-Skalierung; sobald das Fenster existiert, zaehlt
        # die seines Monitors (_sync_ui_scale).
        dpi.enable()
        dpi.set_ui(dpi.system_factor())

        self.root = tk.Tk()
        # Punkt->Pixel-Umrechnung an den Faktor koppeln: daran haengen Dialoge,
        # Menues und alle Widgets mit Punkt-Schrift (die wachsen damit von selbst
        # mit). Der Canvas geht bewusst einen anderen Weg – Pixelschriften ueber
        # dpi.fontpx(), damit die Kachelschrift exakt dem Kachelraster folgt.
        dpi.sync_tk_scaling(self.root)
        self.root.title("Agent Deck")
        self._apply_icon()
        self.root.configure(bg=BG)
        self.root.attributes("-topmost", True)
        self._apply_transparency()
        self.tiles = {}          # {slot: record}; wird ge-clear()-t, NIE ersetzt
        # Je Repo-Block die beiden Items, die seine ZUGEHOERIGKEIT tragen: der Kopf
        # (Repo-Name) und die Schiene links daneben. {win: {"name","rail","connected"}}.
        # Wird beim Neuzeichnen geleert (die Item-IDs sterben mit dem delete('all')).
        self.win_items = {}
        self._hot_win = None     # Repo-Block, der gerade hervorgehoben ist (None = keiner)
        # TileDrag und TileRenderer entstehen NICHT hier, sondern in _build() beim
        # Canvas - siehe dort. Beide hängen an ihm, und der existiert noch nicht.
        self.prompt_tip = ck.Tooltip(self.root)  # Hover-Kachel -> KI-Chat-Zusammenfassung
        # Hover-Zustand fuer den Tooltip (Shared-Tag-sicher, siehe _hover_enter):
        self._hover_slot = None   # Kachel, ueber der der Zeiger gerade ist (None = keine)
        self._tip_show_job = None # geplanter Show-Timer (Hover-Verzoegerung)
        self._tip_hide_job = None # geplanter Hide-Timer (verzoegertes Ausblenden)
        self._tip_visible = False # ist der Tooltip GERADE sichtbar? (fuer die async
                                  # Zusammenfassung: nach einem Klick NICHT wieder aufpoppen)
        # Rueckweg der Daemon-Threads auf den Tk-Thread (siehe _post/_ui_pump).
        # Absichtlich eine Queue und NICHT root.after(0, …) aus dem Thread.
        self._ui_q = queue.Queue()
        log.hook_tk(self.root)  # Tk-Callback-Fehler ins Log statt ins Leere
        self._summary_jobs = set()  # Sessions, deren Chat-Info gerade geholt wird
        self._last_prefetch = 0.0   # letzter Prefetch-Scan (gedrosselt, siehe _prefetch_summaries)
        self._last_beat = 0.0       # letztes Lebenszeichen fuer den Waechter (siehe _beat)
        # session_id -> im Chat erkannte Bezuege {"ticket": …, "pr": …} (leer = keine).
        # Vom Hintergrund-Job gefuellt, damit Tooltip UND Karte sie ohne Datei-I/O im
        # 400-ms-Poll haben.
        self._auto_refs = {}
        if cfg.HOVER_SUMMARY or cfg.TICKET_AUTODETECT:
            cs.prune()            # alte Cache-Dateien laengst geschlossener Sessions weg
        # Alt+Tab ohne Mausbewegung feuert kein <Leave> -> beim App-Fokusverlust ausblenden.
        self.root.bind("<FocusOut>", self._on_focus_out)
        self._last_sig = None    # letztes gezeichnetes Agent-Layout (gegen Flackern)
        # Slim-Modus skaliert statt abzuschneiden: natuerliche (ungescalte) Inhaltsgroesse,
        # aktueller Fit-Faktor und ein Guard gegen Re-Entrancy beim Resize-Neuzeichnen.
        # nat = (0, 0) = "noch kein echter Render" -> die <=0-Guards lassen einen sehr
        # fruehen <Configure> (vor dem ersten Zeichnen) NICHT mit Riesenfaktor loslaufen.
        self._slim_nat = (0.0, 0.0)
        # Ruhefaktor = die Skalierung dieses Monitors: bei 150 % zeichnet das Deck
        # jede Design-Einheit 1.5 Pixel gross. Zieht der Nutzer das Fenster, weicht
        # der Faktor davon ab (Zoom) – der Bezug bleibt aber dpi.ui().
        self._slim_scale = dpi.ui()
        self._slim_relayout = False
        self.dock = None            # EdgeDock (Andocken/Auto-Hide) – erst nach dem Build
        self._dock_key = None       # zuletzt an den Griff-Balken gemeldeter Gesamtstatus
        self._build()
        self.root.update_idletasks()
        self.my_hwnd = wf.toplevel_hwnd(self.root.winfo_id())
        # Jetzt gibt es ein Fenster -> die Skalierung SEINES Monitors gilt (die drei
        # Schirme hier laufen auf 150/100/125 %). Noch ohne Neuzeichnen: das
        # _seed_slim_size weiter unten rendert ohnehin gleich frisch.
        self._sync_ui_scale(redraw=False)
        # Frostpane: die native Titelleiste BLEIBT, wird aber per Win11-DWM dunkel
        # + Cyan-Rand + runde Ecken (kein grauer Standard-Balken mehr).
        wf.style_titlebar(self.my_hwnd, dark=True, border="#7ecbff",
                          caption="#15151c", text="#cfd3dc", round_corners=True)
        # Groesse nur noch an der Ecke unten-rechts ziehbar (Seiten/obere Ecken tot);
        # das Bewegen an der Titelleiste und das programmatische _fit_slim_window
        # bleiben unberuehrt.
        wf.restrict_resize_to_corner(self.my_hwnd)
        # Animator teilt sich self.tiles und den Deck-Canvas mit dem Panel.
        self.anim = GlowAnimator(self.root, self.deck, self.tiles)
        # Es gibt nur noch die schlanke Ansicht: Fenster gleich auf die natuerliche
        # Inhaltsgroesse (Faktor 1.0) bringen; ab da skaliert _on_deck_configure bei
        # jedem Resize.
        self._seed_slim_size()
        self.root.update_idletasks()
        self.refresh()
        self._ui_pump()          # Ergebnisse der Hintergrund-Threads abholen (Queue)
        self.anim.start()        # schneller Timer: Farbton-Crossfade + Glow-Atmen
        self._glow_self_heal()   # Ring nach VS-Code-Update ggf. still neu einspielen
        # Am-Rand-andocken (Auto-Hide): gespeicherten Rand anwenden; ist einer gesetzt,
        # klappt das Deck sofort auf den Griff-Balken ein.
        self.dock = EdgeDock(self)
        self.dock.apply_initial()

    def _glow_self_heal(self) -> None:
        """Ist der Ring aktiviert (deck_settings 'glow'), aber der Patch fehlt (z.B.
        nach einem VS-Code-Update, das die workbench.html ersetzt hat), ihn im
        Hintergrund still neu einspielen. Best effort: Fehler (VS Code offen / keine
        Schreibrechte) werden geschluckt – der Nutzer kann im ⚙-Dialog manuell nachlegen.
        Laeuft in einem Daemon-Thread (Datei-I/O + Glob ueber die VS-Code-Ordner) und
        ruehrt bewusst kein tk an."""
        if not self.settings.get("glow"):
            return

        def work() -> None:
            try:
                installed, n = rg.status()
                if n and not installed:
                    rg.set_glow(True)
            except Exception:
                pass

        threading.Thread(target=work, daemon=True).start()

    def _set_modal(self, v) -> None:
        # Der Ticket-/Einstellungs-Dialog ruft das, solange er offen ist -> refresh()
        # pausiert den Auto-Fokus (sonst klaut ein neu erscheinender Agent dem Dialog
        # den Tastaturfokus). In finally IMMER wieder False.
        self._modal = v

    def _apply_icon(self) -> None:
        """Roboterkopf als Fenster-/Taskbar-Icon (assets/robot.ico, gezeichnet im
        Frost-/Cyan-Look; siehe assets/make_robot.py zum Neu-Generieren).

        Drei Schichten, jede fuer sich defensiv – ein fehlendes Asset darf das Deck
        NIE am Start hindern:
          • AppUserModelID: sonst gruppiert Windows uns unter python.exe und die
            Taskbar zeigt das Python-Feder-Icon statt unseres. Muss VOR dem ersten
            Sichtbarwerden gesetzt werden (hier direkt nach Tk()).
          • iconbitmap(default=…): setzt Titelleisten- UND Taskbar-Icon, und dank
            default= auch alle spaeteren Dialoge (Ticket-/Confirm-Fenster).
          • iconphoto: Fallback, falls iconbitmap scheitert. Die PhotoImage MUSS als
            Attribut ueberleben, sonst raeumt der GC sie weg und das Icon verschwindet.
        """
        base = dp.REPO_ROOT
        ico = os.path.join(base, "assets", "robot.ico")
        png = os.path.join(base, "assets", "robot_64.png")
        try:
            import ctypes
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(
                "agentdeck.panel")
        except Exception:
            pass
        try:
            if os.path.exists(ico):
                self.root.iconbitmap(default=ico)
        except tk.TclError:
            pass
        try:
            if os.path.exists(png):
                self._icon_img = tk.PhotoImage(file=png)   # Referenz halten!
                self.root.iconphoto(True, self._icon_img)
        except tk.TclError:
            pass

    def _apply_transparency(self) -> None:
        """Windows: Hintergrund durchsichtig (transparentcolor = BG) und/oder ganzes
        Fenster halbtransparent (alpha). Faellt bei fehlender Unterstuetzung leise
        auf ein normales Fenster zurueck."""
        if cfg.TRANSPARENT_BG:
            try:
                self.root.attributes("-transparentcolor", BG)
            except tk.TclError:
                pass
        try:
            alpha = float(cfg.WINDOW_ALPHA)
            if alpha < 1.0:
                self.root.attributes("-alpha", alpha)
        except (tk.TclError, TypeError, ValueError):
            pass

    # ── UI aufbauen ─────────────────────────────────────
    def _build(self) -> None:
        # Deck: pro verbundenem Fenster ein Block (kleiner Repo-Name als Kopf, darunter
        # die Agenten-Kacheln). Fuellt das Fenster und skaliert beim Resize.
        self.agent_area = tk.Frame(self.root, bg=BG)
        self.deck = tk.Canvas(self.agent_area, bg=BG, highlightthickness=0,
                              height=dpi.px(44))
        # Configure = Fenster/Canvas neu vermessen -> Deck passend skalieren.
        self.deck.bind("<Configure>", self._on_deck_configure)

        # Die zwei Kachel-Objekte gehören an DIESEN Canvas und entstehen darum hier, nicht
        # im __init__: TileDrag braucht ihn, und der Zeichner braucht TileDrag.press als
        # Rückruf. Als Mixins stellte sich die Frage nie - jetzt steht die Reihenfolge
        # sichtbar da, statt in der Method Resolution Order zu verschwinden.
        self.drag = TileDrag(
            self.root, self.deck, self.tiles, self.order, self.store,
            focus=self.focus_slot, repaint=self._paint_once,
            ordered_slots=self._ordered_slots, hide_tip=self._hide_prompt_tip)
        # Dasselbe eine Ebene höher: die Repo-Blöcke untereinander. Griff ist der
        # Repo-Name - Klick holt das Fenster nach vorn, Ziehen sortiert.
        self.blocks = BlockDrag(
            self.root, self.deck, self.win_items, self.win_order, self.store,
            raise_window=self.show_window, repaint=self._paint_once,
            ordered_windows=self._ordered_windows, block_key=self._block_key,
            hide_tip=self._hide_prompt_tip)
        # Sechs Interaktionen hat eine Kachel - hier stehen sie an einer Stelle, statt in
        # tag_bind-Zeilen mitten im Zeichencode.
        self.tile_renderer = TileRenderer(
            self.tiles, TilesMixin._SLIM_ADD_W,
            on_new=self.create_agent, on_close=self.close_agent,
            on_press=self.drag.press, on_menu=self._card_menu,
            on_enter=self._hover_enter, on_leave=self._hover_leave)
        # Drag&Drop: Motion/Release EINMAL fest am Canvas für BEIDE Dragger (Begründung
        # in reorder.bind_dragging). Die Press-Bindings liegen dort, wo der Griff ist -
        # auf jeder Kachel (_draw_tile) und auf jedem Repo-Namen (_render_agents_slim).
        bind_dragging(self.deck, (self.drag, self.blocks))

        # Untere Leiste: EIN durchgehender Streifen ueber die volle Breite (kein
        # freistehendes Chip-Paar mehr). Links die Claude-Nutzung (Session-Auslastung,
        # Hover = Rest), rechts das Zahnrad zu den Einstellungen. Bleibt dauerhaft am
        # unteren Rand sichtbar. Die Leiste ist selbst defensiv gebaut: ein fehlendes/
        # kaputtes Usage-Modul oder ein nicht laufendes Claude Desktop laesst nur die
        # linke Nutzungsanzeige weg -> das Deck startet trotzdem, das Zahnrad bleibt da.
        from deck.ui.bottombar import BottomBar
        self.bottombar = BottomBar(
            self.root, self.root,
            on_settings=self._open_settings,
            show_usage=cfg.SHOW_USAGE,
            poll_seconds=cfg.USAGE_POLL_SECONDS)
        # Packbares Widget fuer _apply_slim_layout (das Canvas IST die Leiste).
        self.bottom_bar = self.bottombar.canvas

        # Schrift des kleinen Fensternamens – EIN wiederverwendetes Font-Objekt (nicht je
        # Render neu), dessen Metriken die Zeilenhoehe/Breite liefern.
        self._slim_name_font = tkfont.Font(family="Segoe UI", size=12, weight="bold")

        # Rahmen in fester Reihenfolge packen (Deck fuellt, Leiste bleibt unten).
        self._apply_slim_layout()

    # ── Panel neu starten ───────────────────────────────
    def _dragging(self) -> Any:
        """Zieht der Nutzer gerade? Schmale Fassade auf die zwei Dragger (Kacheln
        waagerecht, Repo-Blöcke senkrecht).

        Vier Stellen fragen das - Hover, Layout, der Poll-Takt und das Dock über
        app._dragging(). Für alle vier zählt nur, DASS gezogen wird: sie halten dann ihr
        Neuzeichnen zurück, und ein delete('all') mitten im Zug zerreißt beide Gesten
        gleichermaßen. Keine davon soll wissen müssen, wo der Zustand liegt oder dass es
        zwei davon gibt."""
        return self.drag.dragging() or self.blocks.dragging()

    def _open_settings(self) -> None:
        """Den Einstellungs-Dialog aufmachen.

        Hier steht, was er anfasst - vier Werte und drei Rueckrufe. Als Mixin nahm er
        sich das still aus dem Panel-Zustand; jetzt ist es eine Zeile Verdrahtung, die
        man lesen kann."""
        SettingsDialog(self.root, self.settings, self.store, self.dock,
                       set_modal=self._set_modal, restart=self.restart,
                       place=self._place_dialog).show()

    def restart(self) -> None:
        """Das ganze Panel neu starten: eine frische Instanz mit DEMSELBEN Interpreter
        und denselben Argumenten starten, dann die aktuelle beenden. Erst wenn der neue
        Prozess erfolgreich gestartet ist, wird der Broker-Socket geschlossen (Port 8765
        frei) und os._exit gerufen – so bleibt bei einem Fehlstart die laufende Instanz
        heil. Persistente Dateien (bindings/effort) ueberleben den Neustart."""
        script = os.path.abspath(sys.argv[0])
        # RESTART_ENV im Kind setzen -> der Single-Instance-Guard erkennt das als
        # Neustart-Uebergabe (alt+neu leben kurz gleichzeitig) und tritt NICHT als
        # vermeintlicher Doppelstart zurueck.
        env = dict(os.environ)
        env[si.RESTART_ENV] = "1"
        try:
            subprocess.Popen([sys.executable, script, *sys.argv[1:]],
                             cwd=os.path.dirname(script), env=env)
        except Exception:
            return          # Fehlstart -> laufende Instanz heil lassen (Port bleibt belegt)
        # Neuer Prozess laeuft -> Port sofort freigeben und alte Instanz hart beenden.
        self.broker.stop()
        try:
            self.root.destroy()
        except Exception:
            pass
        # Spur hinterlassen, BEVOR os._exit alles abschneidet: _exit laesst weder
        # atexit noch faulthandler zum Zug kommen, dieser Neustart sah im Log also
        # aus wie ein spurloses Verschwinden ("ABGESCHOSSEN"). Bewusst KEINE
        # "normaler Exit"-Marke: kommt das Kind nicht hoch, SOLL der Waechter
        # einspringen (siehe watchdog.last_end).
        log.note("--- Panel-Ende (Neustart, Kind uebernimmt) ---")
        os._exit(0)

    def run(self) -> None:
        self.root.mainloop()
        # Regulaeres Ende: der mainloop kehrt zurueck, wenn das Fenster zerstoert
        # wurde (Schliessen/restart). Ein FEHLER kommt hier nicht durch – der fliegt
        # aus mainloop() heraus und landet im except in __main__ (und danach als
        # "UNBEHANDELTE EXCEPTION" im Log). Diese Zeile ist also kein Alarm, sondern
        # der Beleg, dass das Panel selbst gegangen ist; watchdog.last_end() liest sie
        # bewusst NICHT als Fehler.
        log.note("mainloop beendet (Fenster zerstoert) -> Panel endet regulaer")


if __name__ == "__main__":
    # Diagnose als Erstes: ein Fehlstart soll auch dann im Log stehen, wenn noch
    # gar kein Fenster existiert (unter pythonw gibt es sonst KEINE Ausgabe).
    log.install()
    # Single-Instance-Guard VOR dem Broker-Start: laeuft schon ein Panel, dieses
    # nach vorn holen und leise beenden -> kein zweites (totes) Panel, das alle
    # Fenster faelschlich als "nicht verbunden" zeigt.
    if not si.acquire_or_focus():
        log.note("schon ein Panel da -> dieses tritt zurueck")
        sys.exit(0)
    try:
        AgentDeck().run()
    except BaseException:
        log.exc("Panel-Start/Lauf")
        raise
