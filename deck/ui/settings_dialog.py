"""Der Einstellungs-Dialog: Sprache, Modell, Effort, Ring um den Chat,
Jira-Präfix, Andocken, Fenster einsammeln, VS Code neu starten - und der
Panel-Neustart.
"""
import os
import tkinter as tk
from tkinter import filedialog

from deck import i18n
from deck.claude import settings as cset
from deck.domain import config as cfg
from deck.ops import vscode_glow as rg
from deck.ops import window_gather as wg
from deck.render import kit as ck
from deck.render.kit import BG, INK, INK_2, INK_3


class SettingsDialog:
    """Der Einstellungs-Dialog als eigenes Objekt, KEIN Mixin.

    Er war eines: eine Methode auf AgentDeck, die sich über `self` aus dem gesamten
    Panel-Zustand nahm, was sie brauchte. Gemessen waren das genau vier Dinge und drei
    Rückrufe — und die stehen jetzt im Konstruktor. Der Unterschied ist nicht Kosmetik:
    vorher musste man die ganze Klasse lesen, um zu wissen, was der Dialog anfasst;
    jetzt steht es in seiner Signatur, und er ist ohne Panel konstruierbar.

    Der Rumpf von show() ist unverändert der alte — die Abhängigkeiten haben nur eine
    andere Herkunft.
    """

    def __init__(self, root, settings, store, dock, *, set_modal, restart,
                 restart_vscode, foreign_windows, place, open_terminal=None) -> None:
        self.root = root            # Tk-Root (Elternfenster des Dialogs)
        self.settings = settings    # Deck-Einstellungen (Dict, wird in place mutiert)
        self.store = store          # BindStore - speichert die Einstellungen
        self.dock = dock            # EdgeDock, für die Andock-Auswahl
        self._set_modal = set_modal    # pausiert den Auto-Fokus, solange offen
        self.restart = restart         # Panel-Neustart (Knopf im Dialog)
        self.restart_vscode = restart_vscode   # VS-Code-Neustart mit Session-Resume
        self.foreign_windows = foreign_windows # offene VS-Code-Fenster ohne Deck-Bindung
        self._place_dialog = place     # platziert das Fenster auf dem richtigen Monitor
        self.open_terminal = open_terminal     # Ordner -> Repo-Block in Windows Terminal

    def show(self) -> None:
        """Frost-gestyltes Einstellungs-Fenster (⚙ in der unteren Leiste). Steuert die
        vier Default-Werte fuer NEU gestartete Claude-Agenten direkt in Claude Codes
        globaler ~/.claude/settings.json (Modell, Permission-Modus, Effort, Antwort-
        sprache) und bietet weiterhin den Panel-Neustart. Stil + modal-Pause wie der
        Ticket-Dialog (sonst klaut ein neu erscheinender Agent den Tastaturfokus)."""
        dlg = tk.Toplevel(self.root)
        dlg.title(i18n.L("Einstellungen", "Settings"))
        dlg.configure(bg=BG)
        dlg.transient(self.root)
        dlg.resizable(False, False)
        try:
            dlg.attributes("-topmost", True)
        except tk.TclError:
            pass
        dlg.withdraw()      # erst aufbauen+platzieren, dann zeigen (siehe _place_dialog)
        tk.Label(dlg, text=i18n.L("Einstellungen", "Settings"), bg=BG, fg=INK,
                 font=("Segoe UI", 12, "bold")).grid(
                     row=0, column=0, columnspan=2, sticky="w", padx=16, pady=(14, 2))
        tk.Label(dlg, text=i18n.L(
                     "Standardwerte für neu gestartete Claude-Agenten\n"
                     "(schreibt ~/.claude/settings.json).",
                     "Defaults for newly started Claude agents\n"
                     "(writes ~/.claude/settings.json)."),
                 bg=BG, fg=INK_3, justify="left", font=("Segoe UI", 9)).grid(
                     row=1, column=0, columnspan=2, sticky="w", padx=16, pady=(0, 12))

        # Ist-Werte aus settings.json vorbelegen (fehlt/unbekannt -> erster Eintrag).
        cur = cset.read_values()
        # Modell kommt aus der DECK-eigenen Einstellung (deck_settings.json), NICHT aus
        # settings.json: dort ist 'model' der schwaechste Hebel und verwirft das '[1m]'-
        # Suffix. Fallback auf einen evtl. alten settings.json-Wert, sonst erster Eintrag.
        cur_model = self.settings.get("model") or cur["model"]
        model_var = tk.StringVar(value=cset.value_to_label(cset.MODEL_CHOICES, cur_model, contains=True))
        mode_var = tk.StringVar(value=cset.value_to_label(cset.MODE_CHOICES, cur["mode"]))
        effort_var = tk.StringVar(value=cset.effort_label(cur["effort"], cur["ultracode"]))
        # Sprache: DECK-eigene, lokalisierte Anzeige-Labels mit direkter Wert-Zuordnung
        # (die kanonischen cset.LANG_CHOICES bleiben unangetastet – von den Unit-Tests
        # gepinnt). Reihenfolge Deutsch, Englisch; unbekannt/leer -> Deutsch. Dasselbe
        # Feld steuert BEIDES: Antwortsprache der Agenten UND Sprache der Deck-Oberflaeche.
        lang_display = [(i18n.L("Deutsch", "German"), "german"),
                        (i18n.L("Englisch", "English"), "english")]
        _lang_v2d = {v: d for d, v in lang_display}
        _lang_d2v = dict(lang_display)
        lang_var = tk.StringVar(
            value=_lang_v2d.get(i18n.normalize(cur["language"]), lang_display[0][0]))

        def _row(r, label, var, labels) -> None:
            tk.Label(dlg, text=label, bg=BG, fg=INK_2, font=("Segoe UI", 10)).grid(
                row=r, column=0, sticky="w", padx=(16, 10), pady=4)
            om = tk.OptionMenu(dlg, var, *labels)
            om.configure(bg="#23232b", fg=INK, activebackground="#33333d",
                         activeforeground="#ffffff", relief="flat", bd=0,
                         highlightthickness=0, anchor="w", width=18,
                         font=("Segoe UI", 9), cursor="hand2")
            try:
                om["menu"].configure(bg="#23232b", fg=INK, activebackground="#33333d",
                                     activeforeground="#ffffff", bd=0, relief="flat")
            except tk.TclError:
                pass
            om.grid(row=r, column=1, sticky="w", padx=(0, 16), pady=4)

        _row(2, i18n.L("Modell", "Model"), model_var, [lbl for lbl, _ in cset.MODEL_CHOICES])
        _row(3, i18n.L("Modus", "Mode"), mode_var, [lbl for lbl, _ in cset.MODE_CHOICES])
        _row(4, "Effort", effort_var, cset.EFFORT_CHOICES)
        _row(5, i18n.L("Sprache", "Language"), lang_var, [d for d, _ in lang_display])

        # Statuszeile (von Speichern UND dem Ring-Schalter genutzt) – vor beiden anlegen.
        status = tk.Label(dlg, text="", bg=BG, fg=INK_3, justify="left",
                          font=("Segoe UI", 9))
        status.grid(row=12, column=0, columnspan=2, sticky="w", padx=16, pady=(8, 0))

        # ── Ring um Chat (VS-Code-Glow) ──────────────────────────────────────
        # Eigene Deck-Einstellung (deck_settings.json 'glow'), KEIN Claude-Setting:
        # patcht VS Codes workbench.html direkt (reenable_glow). Wirkt sofort beim
        # Umschalten; nach einem VS-Code-Update spielt das Deck den Patch beim Start
        # selbst wieder ein (_glow_self_heal). VS Code danach jeweils neu laden.
        glow_var = tk.BooleanVar(value=bool(self.settings.get("glow")))

        def _toggle_glow() -> None:
            on = glow_var.get()
            self.settings["glow"] = on
            self.store.save_settings()
            try:
                ok, total, err = rg.set_glow(on)
            except Exception as e:
                status.configure(text=i18n.L(f"Ring: Fehler – {e}", f"Ring: error – {e}"),
                                 fg="#ff6b6b")
                return
            if err:
                status.configure(text=f"Ring: {err}", fg="#ff6b6b")
            elif not total:
                status.configure(
                    text=i18n.L("Ring: keine VS-Code-Installation gefunden.",
                                "Ring: no VS Code installation found."),
                    fg="#ff6b6b")
            else:
                verb = i18n.L("aktiviert", "enabled") if on else i18n.L("entfernt", "removed")
                status.configure(
                    text=i18n.L(
                        f"Ring {verb} ({ok}/{total}) – in VS Code das Fenster neu laden.",
                        f"Ring {verb} ({ok}/{total}) – reload the window in VS Code."),
                    fg=("#6ee7a8" if on else INK_2))

        cb = tk.Checkbutton(dlg, text=i18n.L("Ring um Chat  (Glow um den fokussierten Chat)",
                                             "Ring around chat  (glow around the focused chat)"),
                            variable=glow_var, command=_toggle_glow, bg=BG, fg=INK_2,
                            selectcolor="#23232b", activebackground=BG,
                            activeforeground=INK, bd=0, highlightthickness=0,
                            anchor="w", font=("Segoe UI", 10), cursor="hand2")
        cb.grid(row=6, column=0, columnspan=2, sticky="w", padx=14, pady=(10, 2))

        # ── Jira-Projekt-Präfix ──────────────────────────────────────────────
        # Eigene Deck-Einstellung (deck_settings.json 'jira_prefix'), KEIN Claude-
        # Setting. Wird einer NUR als Zahl eingegebenen Ticket-ID vorangestellt
        # (z.B. "2701" -> "<PREFIX>-2701"), damit der Agent das Jira-Ticket eindeutig
        # nachschlagen kann. Leer -> reine Nummern bleiben unveraendert. Wirkt ab der
        # naechsten Ticket-Zuweisung (kein Agenten-Neustart noetig). Default aus config.
        tk.Label(dlg, text=i18n.L("Jira-Projekt-Präfix  (z. B. PROJ → PROJ-2701)",
                                  "Jira project prefix  (e.g. PROJ → PROJ-2701)"), bg=BG,
                 fg=INK_2, font=("Segoe UI", 10)).grid(
                     row=7, column=0, sticky="w", padx=(16, 10), pady=4)
        jira_var = tk.StringVar(
            value=self.settings.get("jira_prefix", cfg.JIRA_PROJECT_KEY))
        jira_entry = tk.Entry(dlg, textvariable=jira_var, bg="#23232b", fg=INK,
                              insertbackground=INK, relief="flat",
                              font=("Segoe UI", 10), width=12)
        jira_entry.grid(row=7, column=1, sticky="w", padx=(0, 16), pady=4)

        # ── Am Rand andocken (Auto-Hide) ─────────────────────────────────────
        # Eigene Deck-Einstellung (deck_settings.json 'dock_edge'). Dockt das Fenster
        # an einen Bildschirmrand; es verschwindet dann bis auf einen schmalen Griff,
        # ueber den man es per Hover wieder hervorholt. Wirkt sofort. Hinweis: angedockt
        # gibt es keine Titelleiste – zum Schliessen der App hier wieder "Aus" waehlen.
        DOCK_CHOICES = [(i18n.L("Aus", "Off"), "off"), (i18n.L("Links", "Left"), "left"),
                        (i18n.L("Rechts", "Right"), "right"), (i18n.L("Oben", "Top"), "top")]
        _dock_l2v = dict(DOCK_CHOICES)
        _dock_v2l = {v: lbl for lbl, v in DOCK_CHOICES}
        cur_edge = self.dock.current_edge() if self.dock else \
            self.settings.get("dock_edge", "off")
        dock_var = tk.StringVar(value=_dock_v2l.get(cur_edge, DOCK_CHOICES[0][0]))

        def _on_dock(label) -> None:
            edge = _dock_l2v.get(label, "off")
            if not self.dock:
                return
            if edge == "off":
                self.dock.set_edge(edge)      # abdocken: Fenster kommt zurueck, Dialog bleibt
                return
            # Angedockt verschwindet das Panel bis auf den Griff-Balken – ein weiter
            # offener Dialog (modal + topmost) haengt dann frei im Bild und blockiert
            # das Deck. Also mit dem Andocken schliessen. Beides verzoegert und in
            # dieser Reihenfolge: wir stecken hier noch im command-Callback des
            # OptionMenus (ein destroy mittendrin zerreisst das Menue-Widget), und
            # erst nach dem Schliessen ist der grab weg, wenn der Dock-Poll anlaeuft.
            def _close_then_dock() -> None:
                dlg.destroy()
                self.root.after_idle(
                    lambda: self.dock.set_edge(edge) if self.dock else None)

            self.root.after(0, _close_then_dock)

        tk.Label(dlg, text=i18n.L("Am Rand andocken  (Auto-Hide auf Griff-Balken)",
                                  "Dock to edge  (auto-hide to a handle bar)"), bg=BG,
                 fg=INK_2, font=("Segoe UI", 10)).grid(
                     row=8, column=0, sticky="w", padx=(16, 10), pady=4)
        dock_om = tk.OptionMenu(dlg, dock_var, *[lbl for lbl, _ in DOCK_CHOICES],
                                command=_on_dock)
        dock_om.configure(bg="#23232b", fg=INK, activebackground="#33333d",
                          activeforeground="#ffffff", relief="flat", bd=0,
                          highlightthickness=0, anchor="w", width=18,
                          font=("Segoe UI", 9), cursor="hand2")
        try:
            dock_om["menu"].configure(bg="#23232b", fg=INK, activebackground="#33333d",
                                      activeforeground="#ffffff", bd=0, relief="flat")
        except tk.TclError:
            pass
        dock_om.grid(row=8, column=1, sticky="w", padx=(0, 16), pady=4)

        # ── VS-Code-Fenster einsammeln ───────────────────────────────────────
        # Keine Einstellung, sondern eine Handlung: holt ALLE VS-Code-Fenster auf den
        # Windows-Hauptbildschirm und maximiert sie dort. Gedacht für den
        # Arbeitsplatzwechsel – nach dem Andocken liegen sie sonst auf dem
        # Laptop-Schirm, und jedes einzeln hinüberzuziehen ist Handarbeit für nichts.
        # Gerechnet und verschoben wird in ops/window_gather (dort auch die Regel,
        # WELCHE Fenster gemeint sind); hier steht nur die Rückmeldung.
        def _gather() -> None:
            try:
                done, total, err = wg.gather_to_primary()
            except Exception as e:
                status.configure(
                    text=i18n.L(f"Fenster: Fehler – {e}", f"Windows: error – {e}"),
                    fg="#ff6b6b")
                return
            if not done:
                status.configure(
                    text=err or i18n.L("Kein Fenster verschoben.", "No window moved."),
                    fg="#ff6b6b")
                return
            msg = i18n.L(f"{done} VS-Code-Fenster auf den Hauptmonitor geholt.",
                         f"Moved {done} VS Code window(s) to the primary monitor.")
            status.configure(text=f"{msg} {err}" if err else msg,
                             fg=("#ff6b6b" if err else "#6ee7a8"))

        tk.Label(dlg, text=i18n.L("VS-Code-Fenster einsammeln  (auf den Hauptmonitor, "
                                  "maximiert)",
                                  "Gather VS Code windows  (to the primary monitor, "
                                  "maximized)"), bg=BG,
                 fg=INK_2, font=("Segoe UI", 10)).grid(
                     row=9, column=0, sticky="w", padx=(16, 10), pady=4)
        gather_box = tk.Frame(dlg, bg=BG)
        gather_box.grid(row=9, column=1, sticky="w", padx=(0, 16), pady=4)
        # ⧉ statt eines Monitor-Emojis: 🖥 rendert in Segoe UI als winziger,
        # kaum erkennbarer Klotz (nachgesehen im Bild), ⧉ zeigt zwei
        # übereinanderliegende Fenster – genau das, was der Knopf tut.
        ck.btn(gather_box, i18n.L("⧉ Einsammeln", "⧉ Gather"), _gather)

        # ── VS Code neu starten (mit Session-Resume) ─────────────────────────
        # Auch keine Einstellung, sondern die grösste Handlung in diesem Fenster:
        # schliesst ALLE verbundenen VS-Code-Fenster, wartet, bis wirklich kein
        # Code-Prozess mehr läuft, öffnet sie wieder und nimmt in jeder Kachel ihren
        # Chat per `claude --resume` auf. Der Ablauf steht in ops/vscode_restart; hier
        # steht die Bestätigung und die Rückmeldung.
        #
        # Zwei Klicks, weil ein Fehlklick die gesamte Arbeitsumgebung zumacht. Der
        # zweite muss innerhalb von REARM_MS kommen, sonst fällt der Knopf zurück —
        # ein dauerhaft „scharfer" Knopf wäre nach fünf Minuten wieder eine Falle.
        REARM_MS = 8000
        armed = {"on": False, "timer": None}

        def _disarm() -> None:
            armed["on"] = False
            if armed["timer"]:
                try:
                    dlg.after_cancel(armed["timer"])
                except tk.TclError:
                    pass
                armed["timer"] = None
            try:
                rst_btn.configure(text=i18n.L("⟲ Neu starten", "⟲ Restart"))
            except tk.TclError:
                pass

        def _progress(text, ok=None) -> None:
            """Fortschritt des Ablaufs in die Statuszeile. Wirft absichtlich weiter,
            wenn der Dialog inzwischen zu ist (TclError) — der Ablauf nimmt das als
            „niemand schaut mehr hin" und meldet nur noch ins Log."""
            status.configure(text=text,
                             fg=("#6ee7a8" if ok else "#ff6b6b" if ok is False else INK_2))

        def _restart_code() -> None:
            if not armed["on"]:
                armed["on"] = True
                rst_btn.configure(text=i18n.L("Wirklich? Alles schließen",
                                              "Really? Close everything"))
                # Fremde Fenster NAMENTLICH nennen. Der Neustart schließt sie mit -
                # er muss, sonst läuft VS Code weiter -, aber das Deck hat dort nichts
                # gespeichert und nichts gemerkt. Wer das erst hinterher erfährt, hat
                # die Warnung zu spät bekommen.
                fremd = []
                try:
                    fremd = self.foreign_windows() or []
                except Exception:
                    fremd = []
                text = i18n.L(
                    "Schließt ALLE VS-Code-Fenster – bis kein Prozess mehr läuft – und "
                    "öffnet sie wieder. Nochmal klicken.",
                    "Closes ALL VS Code windows – until no process is left – and "
                    "reopens them. Click again.")
                if fremd:
                    text += i18n.L(
                        f"\nAuch ohne Deck-Verbindung offen: {', '.join(fremd[:4])}"
                        f"{' …' if len(fremd) > 4 else ''} – dort erst speichern!",
                        f"\nAlso open without a deck connection: "
                        f"{', '.join(fremd[:4])}{' …' if len(fremd) > 4 else ''} – "
                        f"save your work there first!")
                status.configure(text=text, fg="#f6c667")
                armed["timer"] = dlg.after(REARM_MS, _disarm)
                return
            _disarm()
            try:
                self.restart_vscode(_progress)
            except Exception as e:
                status.configure(
                    text=i18n.L(f"Neustart: Fehler – {e}", f"Restart: error – {e}"),
                    fg="#ff6b6b")

        tk.Label(dlg, text=i18n.L("VS Code neu starten  (alle Fenster, Chats werden "
                                  "wieder aufgenommen)",
                                  "Restart VS Code  (all windows, chats are resumed)"),
                 bg=BG, fg=INK_2, font=("Segoe UI", 10)).grid(
                     row=10, column=0, sticky="w", padx=(16, 10), pady=4)
        rst_box = tk.Frame(dlg, bg=BG)
        rst_box.grid(row=10, column=1, sticky="w", padx=(0, 16), pady=4)
        rst_btn = ck.btn(rst_box, i18n.L("⟲ Neu starten", "⟲ Restart"), _restart_code)

        # ── Repo in Windows Terminal öffnen ──────────────────────────────────
        # Legt einen Repo-Block an, dessen Agenten in Windows Terminal laufen
        # (ops/wt_host): ein Fenster je Repo, jeder Agent ein Pane daneben. Der erste
        # Agent startet sofort; weitere kommen wie gewohnt über das ＋ im Block.
        def _open_terminal() -> None:
            start = os.path.join(os.path.expanduser("~"), "Repos")
            folder = filedialog.askdirectory(
                parent=dlg, mustexist=True,
                initialdir=start if os.path.isdir(start) else os.path.expanduser("~"),
                title=i18n.L("Repo für Windows Terminal wählen",
                             "Choose a repo for Windows Terminal"))
            if not folder:
                return
            try:
                msg = self.open_terminal(folder)
                status.configure(text=msg, fg="#6ee7a8")
            except Exception as e:
                status.configure(text=i18n.L(f"Terminal: Fehler – {e}",
                                             f"Terminal: error – {e}"), fg="#ff6b6b")

        if self.open_terminal:
            tk.Label(dlg, text=i18n.L("Repo in Windows Terminal öffnen  (Agenten "
                                      "nebeneinander, ohne VS Code)",
                                      "Open a repo in Windows Terminal  (agents side "
                                      "by side, without VS Code)"),
                     bg=BG, fg=INK_2, font=("Segoe UI", 10)).grid(
                         row=11, column=0, sticky="w", padx=(16, 10), pady=4)
            term_box = tk.Frame(dlg, bg=BG)
            term_box.grid(row=11, column=1, sticky="w", padx=(0, 16), pady=4)
            ck.btn(term_box, i18n.L("▤ Repo öffnen …", "▤ Open repo …"), _open_terminal)

        def _save() -> None:
            try:
                # Deck-eigene Werte zuerst persistieren (unabhaengig von den Claude-
                # Settings unten). Jira-Praefix gross geschrieben, wie ein Projekt-Key.
                self.settings["jira_prefix"] = jira_var.get().strip().upper()
                # Modell DECK-eigen speichern: nur so wird es beim Start als
                # `claude --model <wert>` erzwungen (CLI-Flag = hoechste Prioritaet).
                # NICHT nach settings.json schreiben – dort ist 'model' der schwaechste
                # Hebel (User-Scope), verwirft das '[1m]'-Suffix und wird vom zuletzt
                # per /model gewaehlten, in ~/.claude.json gemerkten Modell ueberstimmt.
                self.settings["model"] = cset.label_to_value(cset.MODEL_CHOICES, model_var.get())
                self.store.save_settings()
                lvl, uc = cset.effort_spec(effort_var.get())
                cset.write_values(
                    mode=cset.label_to_value(cset.MODE_CHOICES, mode_var.get()),
                    effort=lvl, ultracode=uc,
                    language=_lang_d2v.get(lang_var.get(), "german"),
                )
                i18n.refresh()   # Deck-Sprache sofort uebernehmen (voll durchgaengig nach Neustart)
                status.configure(
                    text=i18n.L(
                        "Gespeichert ✓ – gilt für neu gestartete Agenten (Sprache der "
                        "Oberfläche: Panel neu starten).",
                        "Saved ✓ – applies to newly started agents (UI language: restart "
                        "the panel)."),
                    fg="#6ee7a8")
            except Exception as e:
                status.configure(
                    text=i18n.L(f"Fehler beim Speichern: {e}", f"Error while saving: {e}"),
                    fg="#ff6b6b")

        btns = tk.Frame(dlg, bg=BG)
        btns.grid(row=13, column=0, columnspan=2, sticky="e", padx=12, pady=(10, 12))
        ck.btn(btns, i18n.L("💾 Speichern", "💾 Save"), _save)
        ck.btn(btns, i18n.L("⟳ Panel neu starten", "⟳ Restart panel"), self.restart)
        ck.btn(btns, i18n.L("Schließen", "Close"), dlg.destroy)
        dlg.bind("<Escape>", lambda e: dlg.destroy())
        self._place_dialog(dlg)
        try:
            dlg.grab_set()
        except tk.TclError:
            pass
        self._set_modal(True)
        try:
            self.root.wait_window(dlg)
        finally:
            self._set_modal(False)
