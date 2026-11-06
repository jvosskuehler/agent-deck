"""Laufende Prozesse zählen - für die eine Frage: ist VS Code WIRKLICH weg?

Gebraucht vom Neustart (`ops/vscode_restart`). Ein geschlossenes Fenster beweist
nichts: VS Code räumt danach noch Renderer, Extension-Hosts und den pty-Host ab, und
genau in dieser Lücke würde ein neu gestartetes `Code.exe` sich beim alten, sterbenden
Prozess anmelden statt eine frische Instanz zu werden. Darum wird nicht gewartet,
sondern nachgesehen.

Das NACHSEHEN läuft über einen Toolhelp32-Snapshot (ctypes), nicht über `tasklist`:
ein Subprozess je Blick kostet einen Prozessstart und liefert lokalisierte Ausgabe -
beides schlechte Zutaten für eine Poll-Schleife. Dieselbe Technik nutzt schon
`claude/hooks/resolve.py`, dort für die PPID-Kette; hier interessieren die Exe-NAMEN.

Die NOTBREMSE am Ende (`kill_by_name`) ruft dagegen absichtlich `taskkill` - siehe
dort: es geht um den Prozessbaum, und den kennt Windows besser als eine
selbstgebaute Nachkommensuche.
"""
import re
import subprocess
import sys

TH32CS_SNAPPROCESS = 0x00000002
_CREATE_NO_WINDOW = 0x08000000

# Ein Exe-Name, nichts anderes: der Wert geht als Argument in eine Kommandozeile, und
# ein "/F" oder "*" darin waere ein anderer Befehl als der gemeinte. Die Namen kommen
# heute aus der gefundenen Installation (ops/vscode_restart.code_exe), also aus dem
# Dateisystem - aber ein Pfad ist kein Beweis.
_EXE_NAME = re.compile(r"^[A-Za-z0-9 ._\-]+\.exe$")


def exe_names() -> list[str] | None:
    """Die Exe-Namen aller laufenden Prozesse (Basename, wie ihn Windows führt).

    None heißt „konnte nicht nachsehen" (kein Windows, Snapshot verweigert) - und ist
    bewusst NICHT die leere Liste: „ich weiß es nicht" darf nie als „nichts läuft mehr"
    durchgehen, sonst startet der Neustart in einen noch lebenden VS Code hinein.
    """
    if sys.platform != "win32":
        return None
    try:
        import ctypes
        from ctypes import wintypes

        class PROCESSENTRY32(ctypes.Structure):
            _fields_ = [
                ("dwSize", wintypes.DWORD),
                ("cntUsage", wintypes.DWORD),
                ("th32ProcessID", wintypes.DWORD),
                ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
                ("th32ModuleID", wintypes.DWORD),
                ("cntThreads", wintypes.DWORD),
                ("th32ParentProcessID", wintypes.DWORD),
                ("pcPriClassBase", ctypes.c_long),
                ("dwFlags", wintypes.DWORD),
                ("szExeFile", ctypes.c_char * 260),
            ]

        k = ctypes.windll.kernel32
        snap = k.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
        if snap == -1:
            return None
        out = []
        e = PROCESSENTRY32()
        e.dwSize = ctypes.sizeof(PROCESSENTRY32)
        ok = k.Process32First(snap, ctypes.byref(e))
        while ok:
            # szExeFile ist ANSI (Process32First, nicht ...W): notfalls ersetzen statt
            # den ganzen Snapshot an einem exotischen Namen scheitern zu lassen.
            out.append(e.szExeFile.decode("mbcs", "replace"))
            ok = k.Process32Next(snap, ctypes.byref(e))
        k.CloseHandle(snap)
        return out
    except Exception:
        return None


def count_exe(name: str) -> int | None:
    """Wie viele Prozesse dieses Exe-Namens laufen (ohne Groß-/Kleinschreibung).

    None = konnte nicht nachsehen (siehe exe_names). Leerer Name -> 0.
    """
    if not name:
        return 0
    names = exe_names()
    if names is None:
        return None
    low = name.lower()
    return sum(1 for n in names if n.lower() == low)


def kill_by_name(name: str) -> bool:
    """Alle Prozesse dieses Exe-Namens SAMT KINDERN hart beenden. Die Notbremse.

    True = das Werkzeug hat seine Arbeit getan (auch dann, wenn es gar nichts zu
    beenden fand). False = Name unplausibel, Werkzeug fehlt, Zeitueberschreitung.

    Gerufen wird `taskkill /F /T /IM <name>` und nicht TerminateProcess per ctypes,
    und zwar wegen des `/T`: unter einem VS-Code-Prozess haengt ein ganzer Baum
    (Renderer, Extension-Hosts, der pty-Host und darunter die Claude-Sessions). Ein
    einzeln abgeschossener Hauptprozess laesst diese Kinder als Waisen zurueck - sie
    halten dann weiter Dateien und Ports, und das Ergebnis waere genau das Gegenteil
    von „alles beendet". Den Baum kennt Windows selbst am besten.

    Dass das Beenden der Claude-Sessions hier GEWOLLT ist, unterscheidet diese Funktion
    von den Sperren in extension/killable.js: dort darf Claude niemals getroffen werden
    (der Nutzer sucht einen Port-Blocker), hier wird die Session gleich danach per
    `claude --resume` wieder aufgenommen.
    """
    if not _EXE_NAME.match(name or ""):
        return False
    try:
        r = subprocess.run(["taskkill", "/F", "/T", "/IM", name],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=_CREATE_NO_WINDOW, timeout=30)
        # 0 = beendet, 128 = "kein solcher Prozess" (aus unserer Sicht dasselbe Ziel).
        return r.returncode in (0, 128)
    except Exception:
        return False


def process_start(pid: int) -> int | None:
    """Startzeit eines LEBENDEN Prozesses als FILETIME (100-ns-Schritte seit 1601),
    sonst None (beendet, unbekannt, kein Zugriff).

    Die Startzeit macht aus einer PID einen Beweis: Windows vergibt PIDs neu, und eine
    alte .wtpid-Datei (ops/wt_host) zeigte sonst irgendwann auf einen fremden Prozess,
    der zufaellig dieselbe Nummer bekommen hat.
    """
    if sys.platform != "win32" or not pid:
        return None
    try:
        import ctypes
        from ctypes import wintypes

        k = ctypes.windll.kernel32
        k.OpenProcess.restype = wintypes.HANDLE
        h = k.OpenProcess(0x1000, False, int(pid))      # PROCESS_QUERY_LIMITED_INFORMATION
        if not h:
            return None
        try:
            code = wintypes.DWORD()
            if not k.GetExitCodeProcess(h, ctypes.byref(code)) or code.value != 259:
                return None                               # 259 = STILL_ACTIVE
            c, e, kt, ut = (wintypes.FILETIME() for _ in range(4))
            if not k.GetProcessTimes(h, ctypes.byref(c), ctypes.byref(e),
                                     ctypes.byref(kt), ctypes.byref(ut)):
                return None
            return (c.dwHighDateTime << 32) | c.dwLowDateTime
        finally:
            k.CloseHandle(h)
    except Exception:
        return None


def kill_tree(pid: int) -> bool:
    """Einen Prozess SAMT KINDERN beenden (`taskkill /F /T /PID`). Gedacht fuer das Pane
    eines Agenten in Windows Terminal: dessen Shell, darunter claude und was claude
    gerade laufen laesst. Begruendung fuer taskkill statt TerminateProcess wie bei
    kill_by_name - der Baum."""
    try:
        pid = int(pid)
    except (TypeError, ValueError):
        return False
    if pid <= 4:                                          # 0 = Leerlauf, 4 = System
        return False
    try:
        r = subprocess.run(["taskkill", "/F", "/T", "/PID", str(pid)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=_CREATE_NO_WINDOW, timeout=30)
        return r.returncode in (0, 128)
    except Exception:
        return False
