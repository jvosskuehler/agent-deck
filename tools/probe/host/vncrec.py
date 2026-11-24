"""Bildschirm der Test-VM von außen aufnehmen, über den VNC-Server von QEMU auf dem Titan.

Warum von außen: die VM hat nur den „Microsoft Basic Display Adapter“ (keine GPU).
ffmpeg ddagrab findet dort kein D3D11-Gerät, gdigrab schafft 4 Bilder je Sekunde und
belastet den Gast. QEMU liest dagegen den Bildspeicher der VM direkt, also genau das,
was die VM zeigt, ohne den Gast anzufassen.

Ablauf: minimaler RFB-Client (Raw-Kodierung, 32 bit), jede Aktualisierung landet im
Bildpuffer; ein zweiter Faden schreibt im festen 60-Hz-Raster den jeweils letzten Stand
an ffmpeg (verlustfrei). Bild k zeigt also den Stand zur Zeit start + k/60 s auf der Uhr
des Titan. QEMU frischt seinen Bildspeicher etwa alle 30 ms auf: die echte Auflösung in
der Zeit ist darum rund 33 ms, das Video hat 60 Bilder je Sekunde zum leichten Abgleich.

    python vncrec.py --port 5903 --seconds 60 --out a.mkv [--stop a.stop]

Neben dem Video: a.mkv.json mit Startzeit und den Zeiten der Aktualisierungen.
"""
import argparse
import json
import socket
import struct
import subprocess
import threading
import time
from pathlib import Path

import imageio_ffmpeg
import numpy as np


def recv_exact(s, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = s.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("VNC-Verbindung zu")
        buf += chunk
    return bytes(buf)


def connect(port):
    s = socket.create_connection(("127.0.0.1", port))
    s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    recv_exact(s, 12)
    s.sendall(b"RFB 003.008\n")
    n = recv_exact(s, 1)[0]
    types = recv_exact(s, n)
    if 1 not in types:
        raise RuntimeError(f"VNC verlangt Anmeldung: {list(types)}")
    s.sendall(b"\x01")
    if struct.unpack(">I", recv_exact(s, 4))[0] != 0:
        raise RuntimeError("VNC lehnt ab")
    s.sendall(b"\x01")  # geteilt
    w, h = struct.unpack(">HH", recv_exact(s, 4))
    recv_exact(s, 16)
    name_len = struct.unpack(">I", recv_exact(s, 4))[0]
    recv_exact(s, name_len)
    # 32 bit, little endian, true color, R<<16 G<<8 B: im Speicher B G R X.
    s.sendall(struct.pack(">BxxxBBBBHHHBBBxxx", 0, 32, 24, 0, 1, 255, 255, 255, 16, 8, 0))
    s.sendall(struct.pack(">BxH", 2, 2) + struct.pack(">ii", 0, -223))
    return s, w, h


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--seconds", type=float, required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--fps", type=int, default=60)
    ap.add_argument("--stop", help="Datei: sobald es sie gibt, endet die Aufnahme (vor --seconds)")
    a = ap.parse_args()

    s, w, h = connect(a.port)
    # Puffer in der Größe beim Start, aber groß genug für eine höhere Auflösung danach.
    fb = np.zeros((max(h, 1440), max(w, 2560), 4), np.uint8)
    size = [w, h]
    lock = threading.Lock()
    updates = []
    stop = threading.Event()

    def reader():
        incremental = 0
        try:
            while not stop.is_set():
                s.sendall(struct.pack(">BBHHHH", 3, incremental, 0, 0, size[0], size[1]))
                incremental = 1
                kind = recv_exact(s, 1)[0]
                if kind != 0:
                    if kind == 2:
                        continue
                    if kind == 3:
                        recv_exact(s, 3)
                        recv_exact(s, struct.unpack(">I", recv_exact(s, 4))[0])
                        continue
                    raise RuntimeError(f"unbekannte Nachricht {kind}")
                recv_exact(s, 1)
                (count,) = struct.unpack(">H", recv_exact(s, 2))
                rects = []
                for _ in range(count):
                    x, y, rw, rh, enc = struct.unpack(">HHHHi", recv_exact(s, 12))
                    if enc == 0:
                        data = recv_exact(s, rw * rh * 4)
                        rects.append((x, y, rw, rh, data))
                    elif enc == -223:
                        rects.append((0, 0, rw, rh, None))
                        incremental = 0
                    else:
                        raise RuntimeError(f"unerwartete Kodierung {enc}")
                with lock:
                    for x, y, rw, rh, data in rects:
                        if data is None:
                            # Neue Auflösung: der Bildspeicher wächst oder schrumpft, das
                            # Video behält seine Größe (oben links, Rest schwarz).
                            fb[:] = 0
                            size[0], size[1] = rw, rh
                            continue
                        fb[y:y + rh, x:x + rw] = np.frombuffer(data, np.uint8).reshape(rh, rw, 4)
                updates.append(time.time())
        except Exception as e:  # noqa: BLE001 - Ende der Aufnahme
            if not stop.is_set():
                print("Leser:", e)

    ff = subprocess.Popen(
        [imageio_ffmpeg.get_ffmpeg_exe(), "-hide_banner", "-loglevel", "error", "-y",
         "-f", "rawvideo", "-pix_fmt", "bgr0", "-s", f"{w}x{h}", "-r", str(a.fps), "-i", "-",
         "-c:v", "libx264rgb", "-preset", "ultrafast", "-crf", "0", a.out],
        stdin=subprocess.PIPE,
    )
    t = threading.Thread(target=reader, daemon=True)
    t.start()
    time.sleep(0.3)
    start = time.time()
    frames = int(a.seconds * a.fps)
    stop_file = Path(a.stop) if a.stop else None
    for k in range(frames):
        if stop_file is not None and k % a.fps == 0 and stop_file.exists():
            frames = k
            break
        due = start + k / a.fps
        delay = due - time.time()
        if delay > 0:
            time.sleep(delay)
        with lock:
            frame = fb[:h, :w].tobytes()
        ff.stdin.write(frame)
    stop.set()
    ff.stdin.close()
    ff.wait()
    s.close()
    with open(a.out + ".json", "w") as f:
        json.dump({"start": start, "fps": a.fps, "frames": frames, "size": [w, h],
                   "updates": [u - start for u in updates]}, f)
    gaps = np.diff(updates) if len(updates) > 1 else np.array([0.0])
    print(f"{frames} Bilder, {len(updates)} Aktualisierungen, Abstand median {np.median(gaps) * 1000:.0f} ms")


if __name__ == "__main__":
    main()
