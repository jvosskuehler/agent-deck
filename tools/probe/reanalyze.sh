#!/bin/bash
# Alle Läufe eines Stands auf dem Titan neu auswerten (nach einer Änderung an
# host/analyze*.py) und die metrics.json zurückholen.
#
#   tools/probe/reanalyze.sh <label>
set -euo pipefail
label=$1
SSH=/c/Windows/System32/OpenSSH/ssh.exe
SCP="/c/Windows/System32/OpenSSH/scp.exe -O"
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
$SCP -q "$here"/host/*.py gpu:deckprobe/
runs=$($SSH gpu "cd ~/deckprobe/runs && ls -d $label-* 2>/dev/null" | tr -d '\r')
# Nacheinander: jede Auswertung nimmt schon bis zu 24 Kerne, und eine laufende Aufnahme
# (vncrec.py) soll ihren Takt halten.
for r in $runs; do
    $SSH gpu "cd ~/deckprobe && nice ./venv/bin/python analyze.py runs/$r > /dev/null 2>&1 || echo 'Auswertung $r gescheitert' >&2"
done
for r in $runs; do
    name=${r#"$label-"}
    mkdir -p "$repo/docs/messung/$label/$name"
    $SCP -q gpu:deckprobe/runs/$r/metrics.json "$repo/docs/messung/$label/$name/metrics.json"
done
python "$here/host/table.py" "$repo/docs/messung/$label"
