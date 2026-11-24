#!/bin/bash
# Einen Ablauf messen: Ausgangslage in der VM, Aufnahme auf dem Titan starten, Ablauf in
# der VM, Protokoll zum Titan, auswerten, Ergebnis nach docs/messung/<label>/<ablauf>/.
#
#   tools/probe/run.sh <label> <ablauf> [last]      z. B. run.sh base new 6
#
# Voraussetzungen und Einrichtung: tools/probe/README.md. Läuft in Git Bash auf dem
# Laptop; ssh/scp immer aus Windows-OpenSSH (Git-Bash-ssh sperrt den Titan aus).
set -euo pipefail
label=$1; name=$2; load=${3:-0}
VM=${DECK_PROBE_VM:-deck-probe}
PORT=${DECK_PROBE_VNC:-5903}
SECONDS_REC=${DECK_PROBE_SECONDS:-90}
SSH=/c/Windows/System32/OpenSSH/ssh.exe
SCP="/c/Windows/System32/OpenSSH/scp.exe -O"
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
tag="$label-$name"; [ "$load" != 0 ] && tag="$tag-load$load"
out="$repo/docs/messung/$label/$name"; [ "$load" != 0 ] && out="$out-load$load"
mkdir -p "$out"
PW='& "C:\Program Files\PowerShell\7\pwsh.exe" -NoProfile -File C:\probe\session.ps1 -File C:\probe\scenario.ps1'

$SCP -q "$here"/guest/*.ps1 "$here"/guest/Probe.cs "$VM":C:/probe/
$SSH "$VM" 'Copy-Item C:\probe\fake-claude.ps1 C:\probe\bin\claude.ps1 -Force'
$SCP -q "$here"/host/*.py gpu:deckprobe/
echo "== $tag: Ausgangslage $(date +%T)"
setup=$($SSH "$VM" "$PW -Params '$name -Phase setup ' -Wait 240" | tr -d '\r')
echo "$setup"
# Ohne saubere Ausgangslage wäre die Messung wertlos.
if echo "$setup" | grep -q "FEHLER\|Zeit abgelaufen"; then exit 1; fi
# Die Aufnahme muss sich ganz lösen (eigene Sitzung, alle drei Kanäle umgeleitet), sonst
# wartet ssh, bis sie fertig ist.
$SSH -n gpu "mkdir -p ~/deckprobe/runs/$tag; cd ~/deckprobe; rm -f runs/$tag/stop runs/$tag/video.mkv.json; setsid nohup ./venv/bin/python vncrec.py --port $PORT --seconds $SECONDS_REC --stop runs/$tag/stop --out runs/$tag/video.mkv > runs/$tag/rec.log 2>&1 < /dev/null &"
sleep 1
echo "== $tag: Ablauf $(date +%T)"
$SSH "$VM" "$PW -Params '$name -Phase run -Out C:\\probe\\out\\$tag -Load $load ' -Wait 120" | tr -d '\r'
echo "== $tag: Ablauf fertig $(date +%T)"
# Aufnahme beenden und warten, bis ffmpeg die Datei geschrieben hat.
$SSH gpu "cd ~/deckprobe && touch runs/$tag/stop && for i in \$(seq 1 60); do [ -f runs/$tag/video.mkv.json ] && break; sleep 1; done"
$SCP -q "$VM":C:/probe/out/$tag/events.jsonl "$VM":C:/probe/out/$tag/info.json "$out/"
$SCP -q "$out/events.jsonl" "$out/info.json" gpu:deckprobe/runs/$tag/
$SSH gpu "cd ~/deckprobe && cat runs/$tag/rec.log >&2; ./venv/bin/python analyze.py runs/$tag > /dev/null && cat runs/$tag/metrics.json" > "$out/metrics.json"
echo "== $tag: $out/metrics.json"
