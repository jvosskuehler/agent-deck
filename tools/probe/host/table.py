"""Kennzahlen eines oder zweier Stände als Markdown-Tabelle.

    python table.py docs/messung/base [docs/messung/neu]

Je Ablauf und Abschnitt eine Zeile; bei zwei Ständen stehen die Werte als „vorher → nachher“.
"""
import json
import sys
from pathlib import Path

COLS = [
    ("Dauer ms", lambda e, v: e.get("duration_ms")),
    ("Fokus da ms", lambda e, v: (e.get("focus") or [None, None])[1]),
    ("Vorhang ab ms", lambda e, v: e.get("veil_on_ms")),
    ("Vorhang ms", lambda e, v: e.get("veil_ms")),
    ("sichtb. Lagewechsel", lambda e, v: e.get("visible_moves")),
    ("Zwischenlagen", lambda e, v: e.get("intermediate")),
    ("Hin und Her", lambda e, v: e.get("reversals")),
    ("Ring fehlt ms", lambda e, v: e.get("ring_missing_ms")),
    ("Ring ohne Fenster ms", lambda e, v: e.get("ring_stray_ms")),
    ("Ring/Schatten blinzelt", lambda e, v: e.get("ring_blinks")),
    ("Video: Lagewechsel", lambda e, v: v.get("visible_moves") if v else None),
    ("Video: Lückenbilder", lambda e, v: v.get("hole_frames") if v else None),
    ("Video: ruhig nach ms", lambda e, v: v.get("settle_ms") if v else None),
]


def rows(folder):
    out = {}
    for m in sorted(Path(folder).glob("*/metrics.json")):
        try:
            data = json.loads(m.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            continue
        video = {s["segment"]: s for s in (data.get("video") or {}).get("segments", [])}
        for e in data["events"]:
            key = (m.parent.name, e["segment"])
            out[key] = [f(e, video.get(e["segment"])) for _, f in COLS]
        info = data.get("info", {})
        if "ready_ms" in info:
            out[(m.parent.name, "tippbereit ms")] = [info["ready_ms"]] + [None] * (len(COLS) - 1)
    return out


def fmt(x):
    if x is None:
        return "–"
    if isinstance(x, float):
        return f"{x:.0f}"
    return str(x)


def main():
    a = rows(sys.argv[1])
    b = rows(sys.argv[2]) if len(sys.argv) > 2 else None
    print("| Ablauf | Abschnitt | " + " | ".join(c for c, _ in COLS) + " |")
    print("|---|---|" + "---|" * len(COLS))
    for key in sorted(set(a) | set(b or {})):
        va = a.get(key, [None] * len(COLS))
        if b is None:
            cells = [fmt(x) for x in va]
        else:
            vb = b.get(key, [None] * len(COLS))
            cells = [f"{fmt(x)} → {fmt(y)}" for x, y in zip(va, vb)]
        print(f"| {key[0]} | {key[1]} | " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
