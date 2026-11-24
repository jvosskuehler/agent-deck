"""Vorher/Nachher-Video eines Ablaufs nebeneinander (ffmpeg hstack), auf dem Titan.

    python sidebyside.py <run_vorher> <run_nachher> <abschnitt> <out.mp4> [--before 0.5] [--after 3]

`run_*` sind Ordner unter runs/ (mit video.mkv, events.jsonl, metrics.json). Beide Videos
beginnen `--before` s vor der Marke des Abschnitts und laufen `--after` s danach; links
vorher, rechts nachher, je halbe Größe. Darunter eine Zeitleiste ist nicht nötig: beide
Hälften starten an derselben Marke, gleiche Stelle im Video = gleiche Zeit nach dem Druck.
"""
import argparse
import json
import subprocess
from pathlib import Path

import imageio_ffmpeg


def start_of(run: Path, segment: str) -> float:
    metrics = json.loads((run / "metrics.json").read_text(encoding="utf-8"))
    off = metrics["video"]["offset_ms"]
    for line in (run / "events.jsonl").read_text(encoding="utf-8").splitlines():
        e = json.loads(line)
        if e.get("ev") == "mark" and e.get("name") == segment:
            return (e["t"] + off) / 1000.0
    raise SystemExit(f"Marke {segment} fehlt in {run}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("before_run")
    ap.add_argument("after_run")
    ap.add_argument("segment")
    ap.add_argument("out")
    ap.add_argument("--before", type=float, default=0.5)
    ap.add_argument("--after", type=float, default=3.0)
    a = ap.parse_args()
    runs = [Path(a.before_run), Path(a.after_run)]
    starts = [max(0.0, start_of(r, a.segment) - a.before) for r in runs]
    dur = a.before + a.after
    ff = imageio_ffmpeg.get_ffmpeg_exe()
    cmd = [ff, "-hide_banner", "-loglevel", "error", "-y"]
    for r, s in zip(runs, starts):
        cmd += ["-ss", f"{s:.3f}", "-t", f"{dur:.3f}", "-i", str(r / "video.mkv")]
    # Halbe Größe je Seite, 4 Pixel weiße Fuge dazwischen.
    graph = ("[0:v]scale=960:-2,pad=964:ih:0:0:white[l];[1:v]scale=960:-2[r];[l][r]hstack=inputs=2,format=yuv420p[v]")
    cmd += ["-filter_complex", graph, "-map", "[v]", "-r", "60", "-c:v", "libx264", "-crf", "20", "-preset", "medium", a.out]
    subprocess.run(cmd, check=True)
    print(a.out)


if __name__ == "__main__":
    main()
