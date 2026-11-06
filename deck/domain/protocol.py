"""Wire-Vokabular des Broker-Protokolls – die EINE Quelle der Wahrheit (Python-Seite).

Zwischen Panel und VS-Code-Extension laufen newline-getrennte JSON-Zeilen. Die
Kommando- und Typ-Strings unten sind der Vertrag: hier ein Wert geaendert, muss
er in extension/extension.js MITGEZOGEN werden. Es gibt bewusst keinen Build-
Step (reines JS/Python), also kann die Extension diese Datei nicht importieren –
sie spiegelt die Strings von Hand. Diese Datei buendelt wenigstens die Python-
Seite (broker.py + agent_deck.py), damit ein Tippfehler ein NameError wird statt
eines still verpuffenden Kommandos.
"""

# ── Kommandos: Panel -> Extension (JSON-Feld "cmd") ──────────────────────
CMD_ASSIGN       = "assign"        # der Extension ihren Fenster-Buchstaben zuweisen
CMD_UNASSIGN     = "unassign"      # Buchstaben wieder vergessen (loest Phantomkachel)
CMD_FOCUS_PANE   = "focusPane"     # ein bestimmtes Terminal (Slot) fokussieren
CMD_SEND         = "send"          # Text/Slash-Kommando an den Agent schicken
#   Feld "submit": true -> Extension schreibt den Text und schickt ihn per SEPARATEM
#   Enter ab (langer Prompt = Paste -> ein mitgeschicktes \r wuerde verschluckt).
CMD_KEY          = "key"           # einzelne Taste (enter/esc/shift-tab, ggf. repeat)
CMD_CREATE_AGENT = "createAgent"   # ein weiteres Claude-Terminal oeffnen
#   Felder: "model" (CLI-Flag --model), "args" (fertige Flag-Zeile, z.B.
#   `--resume "<id>" --model "opus[1m]"` – gebaut in domain/restart_plan.flags, hat
#   Vorrang vor "model"), "cwd" (Arbeitsverzeichnis des neuen Terminals – beim Resume
#   PFLICHT, Claude findet eine Session ueber ihr Projektverzeichnis) und "slot"
#   (Wunschname; belegt oder fremdes Fenster -> die Extension vergibt selbst).
CMD_RELOAD       = "reload"        # "Developer: Reload Window" ausloesen
CMD_CLOSE_AGENT  = "closeAgent"    # ein einzelnes Terminal/Agent schliessen
CMD_CLOSE_WINDOW = "closeWindow"   # das ganze VS-Code-Fenster schliessen
CMD_SAVE_ALL     = "saveAll"       # alle Editoren speichern (vor dem Schliessen: ein
#   ungespeicherter Editor macht aus closeWindow einen modalen Dialog, der einen
#   automatischen Ablauf still anhaelt, bis jemand hinsieht)

# ── Nachrichten-Typen: Extension -> Panel (JSON-Feld "type") ─────────────
TYPE_HELLO     = "hello"           # Erstmeldung: Workspace-Name (+ evtl. window/slots)
TYPE_TERMINALS = "terminals"       # aktualisierte Terminal-/Slot-Liste des Fensters
#   Beide tragen zusaetzlich "folder": den ORDNERPFAD des Fensters (bzw. die
#   .code-workspace-Datei), null bei einem Fenster ohne Ordner. Der Name allein genuegt
#   nicht, um ein Fenster wieder aufzumachen – siehe ops/vscode_restart. Bei einem
#   REMOTE-Fenster steht dort statt eines Pfades die Adresse der Gegenstelle
#   ("vscode-remote://ssh-remote+host/pfad") – restart_plan.open_args unterscheidet beides.
#   Und "remote": null bei einem lokalen Fenster, sonst der Remote-Name ("ssh-remote",
#   "wsl", "dev-container"). Das Panel MUSS das wissen: die Hooks der Agenten drueben
#   schreiben in den Zustandsordner IHRES Rechners, hier kommt also nie eine Meldung an.
#   Ohne den Marker sähe eine Remote-Kachel wie eine ruhige lokale aus (idle) – eine
#   Behauptung statt einer Beobachtung. Mit ihm zeigt sie "unbekannt".
TYPE_SEEN      = "seen"            # Pane in VS Code direkt fokussiert -> Slot als gelesen (done->idle)
