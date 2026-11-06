"""Alle VS-Code-Fenster auf den Hauptmonitor holen und dort maximieren.

Der Knopf „VS-Code-Fenster einsammeln" in den Einstellungen. Gedacht für den
Arbeitsplatzwechsel: hängt der Laptop wieder an der Dockingstation, liegen die Fenster
oft alle auf dem kleinen Laptop-Schirm - und jedes einzeln hinüberzuziehen und neu zu
maximieren ist Handarbeit für nichts.

Hier steht nur die Bilanz: Fenster holen (platform), auswählen (domain), umziehen
(platform), zählen. Rückgabe wie bei `vscode_glow.set_glow` - (erledigt, gefunden,
Fehler) -, damit der Dialog beides in seine Statuszeile schreiben kann, ohne zu rechnen.

Zielmonitor ist der WINDOWS-HAUPTBILDSCHIRM (Systemeinstellungen: „Diese Anzeige als
Hauptanzeige verwenden"). Das ist eine Festlegung, keine Vermutung: der Hauptmonitor ist
der einzige, den das System selbst als „den einen" benennt - jede andere Wahl (der
größte, der, auf dem das Deck liegt) wäre geraten und beim nächsten
Arbeitsplatzwechsel anders.
"""
from deck import i18n
from deck.domain import arrange as ar
from deck.platform import arrange as wa
from deck.platform import monitor


def editor_windows() -> list[wa.Win]:
    """Die sichtbaren VS-Code-Hauptfenster (maximierbar, Prozess ist ein VS Code).

    Getrennt vom Umzug, damit man nachsehen kann, WAS eingesammelt würde, ohne etwas
    zu verschieben.
    """
    return [w for w in wa.visible_windows()
            if ar.is_editor_window(w.exe, w.title) and ar.is_maximizable(w.style)]


def gather_to_primary() -> tuple[int, int, str | None]:
    """Alle VS-Code-Fenster auf den Hauptmonitor holen und maximieren.

    Rückgabe (erledigt, gefunden, Fehler): erledigt = Anzahl umgezogener Fenster,
    gefunden = Anzahl gefundener VS-Code-Fenster, Fehler = None oder eine kurze
    Meldung. Wirft nicht - der Aufrufer ist ein Dialog-Knopf.
    """
    area = monitor.primary_work_area()
    if not area:
        return 0, 0, i18n.L("Hauptmonitor nicht ermittelbar.",
                            "Could not determine the primary monitor.")
    wins = editor_windows()
    if not wins:
        return 0, 0, i18n.L("Kein VS-Code-Fenster gefunden.", "No VS Code window found.")
    done = 0
    for w in wins:
        try:
            # Die gemerkte Normalgröße mitnehmen: das Fenster soll nach einem
            # späteren „Wiederherstellen" so gross sein wie vorher, nur eben auf dem
            # Zielmonitor. Ist sie nicht zu holen (Fenster gerade geschlossen), nimmt
            # normal_rect seine Untergrenze.
            size = wa.normal_size(w.hwnd) or (0, 0)
            if wa.move_maximized(w.hwnd, ar.normal_rect(size[0], size[1], area)):
                done += 1
        except OSError:
            pass        # ein einzelnes verschwundenes Fenster beendet den Lauf nicht
    err = None
    if done < len(wins):
        rest = len(wins) - done
        err = i18n.L(f"{rest} Fenster ließen sich nicht verschieben.",
                     f"{rest} window(s) could not be moved.")
    return done, len(wins), err
