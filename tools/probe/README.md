# Messstand: Fensterabläufe des Decks messen

Misst in einer Windows-VM auf dem Titan, wie das Deck Agentenfenster öffnet, schließt,
umordnet, holt und versteckt und wie Ringe, Schatten und Glow dabei mitkommen. Nie auf
dem eigenen Rechner: die Abläufe nehmen Fokus und Maus.

## Teile

| Datei | Wo | Was |
|---|---|---|
| `guest/Probe.cs` | VM | Ereignisprotokoll (WinEvent-Hook: Lage, Zeigen, Verstecken, Vordergrund der Terminals und der Hilfsfenster des Decks, QPC-Zeit), Sync-Feld oben links (12 × 12 Pixel), Tasten, Auflösung, Skalierung |
| `guest/fake-claude.ps1` | VM | Attrappe statt Claude (als `claude.ps1` im PATH): Vollbild-Oberfläche, zeichnet nach jeder Größenänderung neu, meldet einen Zustand (Ring in Farbe), zwei Marken in Slot-Farbe für die Videoauswertung |
| `guest/scenario.ps1` | VM | ein Ablauf: `new`, `close`, `focus`, `gather`, `reorder`, `resize` (Skalierung 125 % und zurück), `idle` (Kosten im Leerlauf); Phase `setup` stellt die Ausgangslage her, `run` misst (mit `-Load n`: n Rechenschleifen dazu) |
| `guest/deck.ps1` | VM | Deck mit DevTools-Port starten (`AGENT_DECK_DEVTOOLS_PORT`, `ui_windows.rs`), Befehle der Oberfläche aufrufen (`invoke` wie ein Klick), Kosten lesen |
| `guest/session.ps1` | VM | Skript in der Desktop-Sitzung ausführen (SSH läuft in Sitzung 0 ohne Bildschirm) |
| `guest/prepare.ps1`, `guest/install-deck.ps1` | VM | einmal: Desktop einfarbig Magenta, Attrappe, Repos-Ordner; je Stand: Paket installieren |
| `host/vncrec.py` | Titan | Bildschirm der VM von außen über QEMU-VNC aufnehmen (60-Bilder-Raster, echte Auflösung ~30 ms) |
| `host/analyze.py`, `host/analyze_video.py` | Titan | Kennzahlen aus Protokoll und Video |
| `host/timeline.py` | überall | Ereignisprotokoll lesbar, Zeiten ab der Marke (zur Ursachensuche) |
| `host/table.py`, `host/sidebyside.py` | Laptop, Titan | Tabelle eines oder zweier Stände; Vorher/Nachher-Video nebeneinander |
| `run.sh` | Laptop (Git Bash) | ein Ablauf von vorn bis hinten, Ergebnis nach `docs/messung/<label>/<ablauf>/` |
| `all.sh`, `reanalyze.sh`, `videos.sh` | Laptop | alle Abläufe eines Stands (ohne und mit Last); Neuauswertung auf dem Titan; Videos vorher/nachher |

## Einrichten (einmal)

1. VM: `titan vm new deck-probe --guest ~/VMs/deck-probe/guest --memory 8 --cpus 6`; in
   der libvirt-Beschreibung `<model type='vga' vram='32768'><resolution x='1920' y='1080'/></model>`
   (sonst 1280×800, der Basic Display Adapter kann die Auflösung nicht selbst ändern).
2. In der VM nach `C:\probe`: `ffmpeg` ist nicht nötig, aber PowerShell 7
   (`C:\Program Files\PowerShell\7`), die Dateien aus `guest/`; dann
   `session.ps1 -File C:\probe\prepare.ps1`.
3. Titan: `~/deckprobe/venv` mit `numpy` und `imageio-ffmpeg` (`uv venv`, `uv pip install`).
   VNC-Port der VM: `virsh -c qemu:///system vncdisplay deck-probe` (:3 = 5903).
4. Paket bauen (`tools\package.ps1 -OutDir …`), nach `C:\probe\` kopieren,
   `session.ps1 -File C:\probe\install-deck.ps1 -Params "C:\probe\<paket>.zip -Fresh"`.

## Messen

```bash
tools/probe/run.sh base new          # Ausgangsstand, neuer Agent
tools/probe/run.sh base gather 6     # dasselbe unter Last (6 Rechenschleifen)
tools/probe/all.sh v5 6              # alles für einen Stand, danach die Tabelle
python tools/probe/host/table.py docs/messung/base docs/messung/v5   # vorher → nachher
tools/probe/videos.sh base v5        # Videos nach docs/messung/videos/
```

Zwei Stände vergleichen: Paket je Stand bauen, nacheinander installieren
(`install-deck.ps1`, ohne `-Fresh` bleiben die Agenten offen) und messen.

`metrics.json` je Ablauf: `events` (aus dem Protokoll, exakt) und `video` (vom
Bildschirm). Wichtigste Zahlen je Abschnitt: `visible_moves` (sichtbare Lagewechsel
aller Fenster; ideal = Zahl der Fenster, die umziehen), `intermediate` (Zwischenlagen,
Ziel 0), `reversals` (Hin und Her, Ziel 0), `ring_missing_ms`/`ring_stray_ms` (Ring
fehlt bzw. steht ohne Fenster, sichtbar), `veil_ms` (Dauer unter dem Vorhang),
`hole_frames` (Bilder mit Desktop zwischen den Fenstern).

Gemessen wird außerdem `info.json`: CPU-Zeit und Arbeitsspeicher von `agent-deck.exe`
(`deck_exe_*`), samt WebView2 (`deck_*`) und der Agentenprozesse vor und nach dem Ablauf.

Grenzen: die VM hat keine GPU (DWM und Fokus-Rahmen laufen auf WARP), das Bildschirmfoto
des Vorhangs (`BitBlt`) ist dort langsamer als auf echter Hardware. Die Attrappe startet
in Millisekunden; die Zeit bis tippbereit mit echtem Claude braucht eine Anmeldung.
