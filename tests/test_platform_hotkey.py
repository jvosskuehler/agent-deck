"""Globale Tastenkombination: das Lesen der Kombination (platform/hotkey.py).

Das Registrieren selbst ist Win32 und wird nicht hier, sondern am Panel angeschaut -
ein Test, der eine echte globale Taste belegt, nahme sie dem Nutzer weg."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.platform import hotkey as hk


def test_standardkombination_wird_gelesen():
    assert hk.parse("ctrl+alt+space") == (hk.MOD_CONTROL | hk.MOD_ALT, 0x20)
    assert hk.parse("Strg + Alt + Leertaste") == (hk.MOD_CONTROL | hk.MOD_ALT, 0x20)


def test_buchstaben_ziffern_und_funktionstasten():
    assert hk.parse("win+shift+j") == (hk.MOD_WIN | hk.MOD_SHIFT, ord("J"))
    assert hk.parse("ctrl+alt+7") == (hk.MOD_CONTROL | hk.MOD_ALT, ord("7"))
    assert hk.parse("alt+f12") == (hk.MOD_ALT, 0x7B)


def test_ohne_modifier_oder_mit_unbekanntem_teil_gibt_es_keine_taste():
    assert hk.parse("space") is None
    assert hk.parse("") is None and hk.parse(None) is None
    assert hk.parse("hyper+space") is None
    assert hk.parse("ctrl+kaputt") is None
    assert hk.parse("ctrl+f25") is None


def test_die_standard_sprungtaste_ist_lesbar():
    from deck.domain import config as cfg
    assert hk.parse(cfg.JUMP_HOTKEY) == (hk.MOD_CONTROL, 0xE2)
    assert hk.parse("strg+kleiner") == (hk.MOD_CONTROL, 0xE2)
