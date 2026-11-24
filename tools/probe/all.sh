#!/bin/bash
# Alle Abläufe für einen Stand messen, ohne und mit Last, danach die Tabelle.
#
#   tools/probe/all.sh <label> [last]      z. B. all.sh base 6
#
# Last = Zahl der Rechenschleifen (so viele wie vCPUs: alle Kerne voll).
set -uo pipefail
label=$1; load=${2:-6}
here=$(cd "$(dirname "$0")" && pwd)
for name in new close focus gather reorder resize; do
    "$here/run.sh" "$label" "$name" 0 || echo "FEHLER: $label $name"
    "$here/run.sh" "$label" "$name" "$load" || echo "FEHLER: $label $name unter Last"
done
python "$here/host/table.py" "$here/../../docs/messung/$label"
