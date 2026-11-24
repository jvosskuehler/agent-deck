"""Kennzahlen eines Ablaufs aus dem Ereignisprotokoll (events.jsonl) und, wenn da, aus
dem Video (vncrec.py). Ausgabe: metrics.json neben dem Protokoll.

    python analyze.py <ordner>          # erwartet events.jsonl, info.json, optional video.mkv

Je Abschnitt (von einer Marke zur nächsten) zählt das Protokoll:
- sichtbare Lagen je Agentenfenster: jede Lageänderung außerhalb des Vorhangs. Ziel ist
  genau eine (von alt nach neu), unter dem Vorhang ist alles unsichtbar.
- Zwischenlagen (sichtbar, weder Anfang noch Ende) und Rückwärtsbewegungen (eine Kante
  läuft zurück).
- Ringfehler: Zeit, in der ein offenes Agentenfenster ohne passenden Ring dasteht oder
  ein Ring ohne Fenster, nur außerhalb des Vorhangs.
- Vorhang: wann er kommt, wie lange er steht.
- Dauer: Marke bis zur letzten Bewegung eines Agentenfensters, Rings oder Vorhangs.

Das Video zählt dasselbe noch einmal so, wie es auf dem Bildschirm ankam (Marken der
Attrappe, fake-claude.ps1), dazu Bilder mit Lücken (Desktop-Magenta zwischen den
Fenstern) und Bilder, in denen der Glow nicht am Fenster mit Fokus liegt.
"""
import json
import sys
from pathlib import Path

WT = "CASCADIA_HOSTING_WINDOW_CLASS"
VEIL = "AgentDeckVeil"
LAYER = "AgentDeckGroupShadow"
SLACK = 2  # Pixel, die noch als gleiche Lage gelten


def load(path):
    out = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def same(a, b, slack=SLACK):
    return a is not None and b is not None and all(abs(x - y) <= slack for x, y in zip(a, b))


def segments(events):
    marks = [e for e in events if e["ev"] == "mark"]
    end = events[-1]["t"] if events else 0
    segs = []
    for i, m in enumerate(marks):
        if m["name"] in ("start", "end"):
            continue
        nxt = next((x["t"] for x in marks[i + 1:]), end)
        segs.append((m["name"], m["t"], nxt))
    return segs


class Desk:
    """Zustand aller beobachteten Fenster, Ereignis für Ereignis nachgespielt."""

    def __init__(self):
        self.win = {}  # h -> dict(cls, title, frame, rect, vis, min)

    def apply(self, e):
        h = e.get("h")
        if h is None:
            return
        if e["ev"] == "destroy":
            self.win.pop(h, None)
            return
        if e["ev"] == "fg":
            return
        w = self.win.setdefault(h, {})
        # Sichtbare Fläche eines Terminals: die Client-Fläche (siehe Probe.cs).
        frame = e.get("client") if e.get("cls") == WT and e.get("client") else e.get("frame")
        w.update(cls=e["cls"], title=e.get("title", ""), frame=frame, rect=e.get("rect"),
                 vis=e.get("vis", 0), min=e.get("min", 0))

    def veiled(self):
        return any(w["cls"] == VEIL and w["vis"] for w in self.win.values())

    def agents(self):
        return {w["title"]: w["frame"] for w in self.win.values()
                if w["cls"] == WT and w["vis"] and not w["min"] and w.get("frame") and w["title"]}

    def layers(self):
        return [w["rect"] for w in self.win.values() if w["cls"] == LAYER and w["vis"] and w.get("rect")]


def ring_trouble(desk, ring):
    """(Fenster ohne Ring, Ringe ohne Fenster). Ein Ring liegt `ring` Pixel um den Rahmen."""
    agents = desk.agents()
    layers = desk.layers()
    want = {s: [f[0] - ring, f[1] - ring, f[2] + ring, f[3] + ring] for s, f in agents.items()}
    missing = [s for s, r in want.items() if not any(same(r, l, 2) for l in layers)]
    # Ringe sind so groß wie ein Fenster plus Ring rundum; alles andere sind Gruppenschatten.
    sizes = [(r[2] - r[0], r[3] - r[1]) for r in want.values()]
    ringlike = [l for l in layers if any(abs((l[2] - l[0]) - w) <= 3 and abs((l[3] - l[1]) - h) <= 3 for w, h in sizes)]
    stray = [l for l in ringlike if not any(same(l, r, 2) for r in want.values())]
    return missing, stray


def reversals(seq):
    n = 0
    for k in range(4):
        last = 0
        for a, b in zip(seq, seq[1:]):
            d = b[k] - a[k]
            if abs(d) <= SLACK:
                continue
            s = 1 if d > 0 else -1
            if last and s != last:
                n += 1
            last = s
    return n


def analyze_events(events, ring):
    desk = Desk()
    segs = segments(events)
    result = []
    idx = 0
    for name, t0, t1 in segs:
        while idx < len(events) and events[idx]["t"] < t0:
            desk.apply(events[idx])
            idx += 1
        start = dict(desk.agents())
        seen = {s: [f] for s, f in start.items()}  # sichtbare Lagen je Slot
        hidden_moves = {}
        veil_on = veil_off = None
        last_move = None
        fg = None
        trouble_ms = {"missing": 0.0, "stray": 0.0}
        last_sig = {h: (w.get("frame"), w.get("vis"), w.get("min"), False) for h, w in desk.win.items()}
        hidden_layers = {}  # h -> Zeit, zu der ein Ring/Schatten sichtbar weggenommen wurde
        blinks = []
        tprev = t0
        state = ring_trouble(desk, ring) if not desk.veiled() else ([], [])
        while idx < len(events) and events[idx]["t"] < t1:
            e = events[idx]
            idx += 1
            dt = e["t"] - tprev
            if not desk.veiled():
                if state[0]:
                    trouble_ms["missing"] += dt
                if state[1]:
                    trouble_ms["stray"] += dt
            tprev = e["t"]
            was_veiled = desk.veiled()
            desk.apply(e)
            if e["ev"] == "fg" and e.get("cls") == WT:
                # Der letzte Wechsel zählt; den Titel trägt ein frisches Fenster erst später.
                fg = (e["h"], e["t"] - t0)
            if e.get("cls") == VEIL:
                if desk.veiled() and veil_on is None:
                    veil_on = e["t"] - t0
                if was_veiled and not desk.veiled():
                    veil_off = e["t"] - t0
                    # Was unter dem Vorhang entstand, sieht man jetzt: jede Lage zählt.
                    for s_, f_ in desk.agents().items():
                        if not same(f_, seen.setdefault(s_, [None])[-1]):
                            seen[s_].append(f_)
            # Ring oder Schatten weg und gleich wieder da (ohne Vorhang): ein Blinzeln.
            if e.get("cls") == LAYER and not desk.veiled():
                if e["ev"] == "hide":
                    hidden_layers[e["h"]] = e["t"]
                elif e["ev"] == "show" and e["h"] in hidden_layers:
                    blinks.append(round(e["t"] - hidden_layers.pop(e["h"]), 1))
            # Dauer: bis zur letzten Meldung, die wirklich etwas änderte (Lage, sichtbar,
            # minimiert); ein Ring, der nur neu gestapelt wird, meldet dieselbe Lage.
            if e.get("cls") in (WT, LAYER, VEIL) and e["ev"] in ("loc", "show", "hide", "destroy", "minstart", "minend"):
                sig = (e.get("client") or e.get("frame"), e.get("vis"), e.get("min"), e["ev"] == "destroy")
                if last_sig.get(e.get("h")) != sig:
                    last_move = e["t"] - t0
                last_sig[e.get("h")] = sig
            if e.get("cls") == WT and e.get("title"):
                s = e["title"]
                f = desk.agents().get(s)
                if desk.veiled():
                    hidden_moves[s] = hidden_moves.get(s, 0) + 1
                elif f is not None and not same(f, seen.setdefault(s, [None])[-1]):
                    seen[s].append(f)
            state = ring_trouble(desk, ring) if not desk.veiled() else ([], [])
        end = desk.agents()
        for s_, f_ in end.items():
            if not [x for x in seen.get(s_, []) if x is not None]:
                seen[s_] = [f_]
        if fg is not None:
            fg = (desk.win.get(fg[0], {}).get("title", "?"), round(fg[1], 1))
        per = {}
        for s, lst in seen.items():
            lst = [x for x in lst if x is not None]
            final = end.get(s)
            inter = [x for x in lst[1:] if not same(x, final)]
            per[s] = {"visible_positions": len(lst), "intermediate": len(inter), "reversals": reversals(lst),
                      "hidden_moves": hidden_moves.get(s, 0)}
        result.append({
            "segment": name,
            "duration_ms": round(last_move, 1) if last_move is not None else 0,
            "veil_on_ms": veil_on, "veil_ms": (veil_off - veil_on) if veil_on is not None and veil_off is not None else None,
            "focus": fg,
            "windows": per,
            "ring_missing_ms": round(trouble_ms["missing"], 1),
            "ring_stray_ms": round(trouble_ms["stray"], 1),
            "ring_blinks": len(blinks),
            "ring_blink_ms": round(sum(blinks), 1),
            "visible_moves": sum(max(0, p["visible_positions"] - 1) for p in per.values()),
            "intermediate": sum(p["intermediate"] for p in per.values()),
            "reversals": sum(p["reversals"] for p in per.values()),
        })
    return result


def main():
    folder = Path(sys.argv[1])
    events = load(folder / "events.jsonl")
    info = json.loads((folder / "info.json").read_text(encoding="utf-8-sig")) if (folder / "info.json").exists() else {}
    ring = int(sys.argv[2]) if len(sys.argv) > 2 else 1
    metrics = {"info": info, "events": analyze_events(events, ring)}
    video = folder / "video.mkv"
    if video.exists():
        import analyze_video
        metrics["video"] = analyze_video.analyze(video, events, ring)
    elif (folder / "metrics.json").exists():
        # Ohne Video (etwa auf dem Laptop nachgerechnet): die Videowerte von vorher behalten.
        try:
            old = json.loads((folder / "metrics.json").read_text(encoding="utf-8"))
            if old.get("video"):
                metrics["video"] = old["video"]
        except (json.JSONDecodeError, UnicodeDecodeError):
            pass
    (folder / "metrics.json").write_text(json.dumps(metrics, indent=1, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(metrics, indent=1, ensure_ascii=False)[:4000])


if __name__ == "__main__":
    main()
