"""Ereignisprotokoll lesbar ausgeben, Zeiten relativ zur jeweils letzten Marke.

    python timeline.py events.jsonl [--all]

Ohne --all fallen aufeinanderfolgende Lagemeldungen desselben Fensters mit gleicher Lage
weg (Windows meldet viele doppelt).
"""
import json
import sys

SHORT = {"CASCADIA_HOSTING_WINDOW_CLASS": "WT", "AgentDeckVeil": "VORHANG", "AgentDeckGroupShadow": "RING/SCHATTEN",
         "AgentDeckFocusFrameGpu": "GLOW"}


def main():
    path = sys.argv[1]
    every = "--all" in sys.argv
    base = 0.0
    last = {}
    for line in open(path, encoding="utf-8"):
        e = json.loads(line)
        if e["ev"] == "mark":
            base = e["t"]
            print(f"\n== {e['name']} (t={e['t']:.0f})")
            continue
        if e["ev"] in ("sync", "note"):
            continue
        cls = SHORT.get(e.get("cls", ""), e.get("cls", ""))
        key = (e.get("h"), e["ev"])
        sig = (e.get("frame"), e.get("vis"), e.get("min"))
        if not every and e["ev"] == "loc" and last.get(key) == sig:
            continue
        last[key] = sig
        title = e.get("title", "")
        shown = e.get("client") or e.get("frame")
        print(f"{e['t'] - base:9.1f}  {e['ev']:9} {cls:14} {title[:14]:14} h={e.get('h')} frame={shown} vis={e.get('vis')} min={e.get('min')}")


if __name__ == "__main__":
    main()
