"""Kennzahlen aus dem Video eines Ablaufs (vncrec.py), siehe analyze.py.

Ausrichtung: das Sync-Feld oben links (Probe.cs) wechselt bei jeder Marke die Farbe;
das erste Bild in der Farbe der Marke k gehört zur Zeit der Marke k im Protokoll.

Fensterlage je Bild: die Attrappe (fake-claude.ps1) zeichnet je Fenster zwei Marken in
der Farbe ihres Slots, oben links und unten rechts. Linke obere Ecke der ersten und
rechte untere Ecke der zweiten ergeben ein Rechteck, das sich genau dann bewegt, wenn das
Fenster (samt Inhalt) sichtbar umzieht oder neu zeichnet.

Glow: der Fokus-Rahmen färbt einen Saum 3 bis 12 Pixel außerhalb des Fensters mit
Fokus orange. Ein Bild zählt als „Glow daneben“, wenn innerhalb dieses Fensters (mehr
als 14 Pixel vom Rand) ein orangefarbener Streifen liegt, wie ihn ein Glow am alten Platz
hinterlässt.
"""
import re
from multiprocessing import Pool, cpu_count

import numpy as np
import imageio_ffmpeg

# Sync-Farben (RGB) in der Reihenfolge von Probe.SyncColors (dort BGR).
SYNC = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0), (0, 255, 255), (255, 0, 255), (255, 255, 255), (128, 0, 0)]
# Markenfarben je Slot-Nummer wie in fake-claude.ps1.
MARK = [(30, 60, 230), (200, 30, 30), (20, 160, 160), (120, 40, 200), (110, 90, 20), (0, 0, 0), (60, 60, 140), (150, 150, 150)]
DESKTOP = (255, 0, 255)


def near(img, rgb, tol):
    """Maske der Pixel nahe `rgb`. `img` als int16 übergeben spart je Farbe eine Umwandlung."""
    if img.dtype != np.int16:
        img = img.astype(np.int16)
    return np.all(np.abs(img - np.array(rgb, np.int16)) <= tol, axis=-1)


def frames(path):
    gen = imageio_ffmpeg.read_frames(str(path), pix_fmt="rgb24")
    meta = next(gen)
    w, h = meta["size"]
    for raw in gen:
        yield np.frombuffer(raw, np.uint8).reshape(h, w, 3).copy()


def sync_index(img):
    px = img[3, 3].astype(int)
    best = min(range(len(SYNC)), key=lambda k: sum(abs(px[i] - SYNC[k][i]) for i in range(3)))
    return best if sum(abs(px[i] - SYNC[best][i]) for i in range(3)) < 40 else None


def window_box(img, rgb):
    m = near(img, rgb, 14)
    ys, xs = np.nonzero(m)
    if len(xs) < 60:
        return None
    return (int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max()))


def _frame_info(args):
    img, colors = args
    sync = sync_index(img)
    img = img.astype(np.int16)
    boxes = {s: window_box(img, c) for s, c in colors.items()}
    holes = int(near(img[:-60], DESKTOP, 10).sum())
    return sync, boxes, holes


def analyze(path, events, ring):
    marks = [e for e in events if e["ev"] == "mark"]
    slots = sorted({e["title"] for e in events if e.get("cls") == "CASCADIA_HOSTING_WINDOW_CLASS"
                    and re.fullmatch(r"[A-Z]\d+", e.get("title", ""))})
    colors = {}
    for s in slots:
        digits = "".join(c for c in s if c.isdigit())
        if digits:
            colors[s] = MARK[(int(digits) - 1) % len(MARK)]
    # Je Bild unabhängig: auf alle Kerne des Titan verteilt (ein Bild 1080p kostet ~0,1 s).
    with Pool(min(24, cpu_count())) as pool:
        per_frame = list(pool.imap(_frame_info, ((img, colors) for img in frames(path)), chunksize=8))
    sync = [f[0] for f in per_frame]
    boxes = [f[1] for f in per_frame]
    holes = [f[2] for f in per_frame]
    fps = 60.0
    # Bildnummer der Marken: erstes Bild in ihrer Farbe nach dem vorigen Wechsel.
    mark_frame = {}
    k = 0
    for i, m in enumerate(marks):
        color = m.get("color")
        while k < len(sync) and sync[k] != color:
            k += 1
        if k < len(sync):
            mark_frame[i] = k
    if not mark_frame:
        return {"error": "keine Sync-Marke im Video gefunden"}
    # Zeitversatz Protokoll → Video aus allen gefundenen Marken (Median).
    offs = [mark_frame[i] / fps * 1000 - marks[i]["t"] for i in mark_frame]
    off = float(np.median(offs))
    segs = []
    names = [(i, m) for i, m in enumerate(marks) if m["name"] not in ("start", "end")]
    for n, (i, m) in enumerate(names):
        f0 = int(round((m["t"] + off) / 1000 * fps))
        nxt = next((mm["t"] for j, mm in enumerate(marks) if j > i), None)
        f1 = int(round((nxt + off) / 1000 * fps)) if nxt else len(boxes)
        f0, f1 = max(0, f0), min(len(boxes), f1)
        if f1 <= f0:
            continue
        base_holes = holes[f1 - 1]
        per = {}
        last_change = None
        for s in colors:
            seq = [boxes[f][s] for f in range(f0, f1)]
            pos = []
            for f, bx in zip(range(f0, f1), seq):
                if bx is None:
                    continue
                if not pos or any(abs(a - b) > 2 for a, b in zip(bx, pos[-1])):
                    pos.append(bx)
                    if len(pos) > 1:
                        last_change = max(last_change or 0, f - f0)
            final = pos[-1] if pos else None
            inter = [p for p in pos[1:-1] if any(abs(a - b) > 2 for a, b in zip(p, final))]
            per[s] = {"positions": len(pos), "intermediate": len(inter)}
        hole_frames = sum(1 for f in range(f0, f1) if holes[f] - base_holes > 2000)
        segs.append({
            "segment": m["name"],
            "frames": f1 - f0,
            "settle_ms": round(last_change / fps * 1000) if last_change is not None else 0,
            "windows": per,
            "visible_moves": sum(max(0, p["positions"] - 1) for p in per.values()),
            "intermediate": sum(p["intermediate"] for p in per.values()),
            "hole_frames": hole_frames,
        })
    return {"offset_ms": round(off, 1), "marks_found": len(mark_frame), "segments": segs}
