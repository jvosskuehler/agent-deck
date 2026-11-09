"""Globale Tastenkombination - wirkt, egal welches Programm gerade vorn ist.

`RegisterHotKey` bindet eine Kombination an einen THREAD; dessen Nachrichtenschleife
bekommt dann WM_HOTKEY. Darum laeuft hier ein eigener kleiner Thread mit
`GetMessageW`-Schleife - Tk hat keine globalen Tasten, und seine eigene Schleife
bekommt Thread-Nachrichten ohne Fenster nicht zu sehen. Der Rueckruf kommt auf diesem
Thread an; wer Tk anfassen will, reicht ihn weiter (ui/uithread._post).

Ein Nebeneffekt, den man braucht: Windows erlaubt dem Prozess, der die Taste bekommt,
ein anderes Fenster nach vorn zu holen (sonst blinkt nur die Taskleiste).

Reine stdlib (ctypes).
"""
import ctypes
import threading
import time
from collections.abc import Callable
from ctypes import wintypes

MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, MOD_NOREPEAT = 0x1, 0x2, 0x4, 0x8, 0x4000
WM_HOTKEY, WM_QUIT = 0x0312, 0x0012
_MODS = {"ctrl": MOD_CONTROL, "strg": MOD_CONTROL, "alt": MOD_ALT, "shift": MOD_SHIFT,
         "win": MOD_WIN}
_KEYS = {"space": 0x20, "leertaste": 0x20, "enter": 0x0D, "tab": 0x09, "esc": 0x1B,
         "pause": 0x13, "insert": 0x2D, "home": 0x24, "end": 0x23,
         # Die Taste links neben Y auf der deutschen Tastatur (<>|), VK_OEM_102.
         "<": 0xE2, "kleiner": 0xE2}


VK_F13, VK_F24 = 0x7C, 0x87


def parse(combo: str) -> tuple[int, int] | None:
    """'ctrl+alt+space' -> (Modifier-Bits, Virtual-Key). Buchstaben, Ziffern, F1-F24 und
    die Namen aus _KEYS. Mindestens ein Modifier ist Pflicht - eine nackte globale Taste
    nimmt sie jedem anderen Programm weg. Ausnahme F13-F24: die hat keine normale
    Tastatur, sie kommen nur von Makro-Tastaturen (das Makropad sendet F13-F21), und
    kein Programm rechnet mit ihnen. Unbrauchbar -> None."""
    parts = [p.strip().lower() for p in str(combo or "").split("+") if p.strip()]
    if not parts:
        return None
    if len(parts) == 1:
        got = parse("ctrl+" + parts[0])
        return (0, got[1]) if got and VK_F13 <= got[1] <= VK_F24 else None
    mods = 0
    for p in parts[:-1]:
        if p not in _MODS:
            return None
        mods |= _MODS[p]
    key = parts[-1]
    if key in _KEYS:
        vk = _KEYS[key]
    elif len(key) == 1 and key.isalnum():
        vk = ord(key.upper())
    elif key.startswith("f") and key[1:].isdigit() and 1 <= int(key[1:]) <= 24:
        vk = 0x70 + int(key[1:]) - 1
    else:
        return None
    return mods, vk


class GlobalHotkey:
    """Eine globale Tastenkombination auf einem eigenen Thread.

    Ist die Kombination noch belegt - typisch beim Panel-Neustart, wenn die alte
    Instanz sie noch kurz haelt -, wird einige Sekunden lang erneut versucht."""

    def __init__(self, combo: str, callback: Callable[[], None],
                 retries: int = 10, retry_s: float = 1.0) -> None:
        self.combo = combo
        self.callback = callback
        self.retries, self.retry_s = retries, retry_s
        self.ok = False
        self._tid = 0

    def start(self) -> bool:
        got = parse(self.combo)
        if not got:
            return False
        threading.Thread(target=self._run, args=got, name="hotkey", daemon=True).start()
        return True

    def _run(self, mods: int, vk: int) -> None:
        u = ctypes.WinDLL("user32", use_last_error=True)
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        self._tid = k.GetCurrentThreadId()
        for _ in range(max(1, self.retries)):
            if u.RegisterHotKey(None, 1, mods | MOD_NOREPEAT, vk):
                self.ok = True
                break
            time.sleep(self.retry_s)
        if not self.ok:
            return
        msg = wintypes.MSG()
        try:
            while u.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
                if msg.message == WM_HOTKEY:
                    try:
                        self.callback()
                    except Exception:
                        pass            # ein Fehler im Rueckruf darf die Taste nicht toeten
        finally:
            u.UnregisterHotKey(None, 1)

    def stop(self) -> None:
        if self._tid:
            ctypes.WinDLL("user32").PostThreadMessageW(self._tid, WM_QUIT, 0, 0)
