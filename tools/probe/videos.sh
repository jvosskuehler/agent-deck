#!/bin/bash
# Vorher/Nachher-Videos (nebeneinander, links vorher) je Ablauf nach docs/messung/videos/.
#
#   tools/probe/videos.sh <vorher> <nachher>       z. B. videos.sh base v4
set -euo pipefail
a=$1; b=$2
SSH=/c/Windows/System32/OpenSSH/ssh.exe
SCP="/c/Windows/System32/OpenSSH/scp.exe -O"
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../../docs/messung/videos"
mkdir -p "$out"
$SCP -q "$here"/host/sidebyside.py gpu:deckprobe/
# Ablauf (Ordner) und Abschnitt (Marke); Last-Läufe mit -load6.
clips="new:new new-load6:new close:close close-load6:close focus:focus-A3 focus-load6:focus-A3
gather:fetch gather:hide gather-load6:fetch reorder:reorder reorder-load6:reorder resize:resize resize:resize-back"
for c in $clips; do
    run=${c%%:*}; seg=${c#*:}; seg=${seg//-A/ A}
    file="$run-${seg// /-}.mp4"
    $SSH gpu "cd ~/deckprobe && ./venv/bin/python sidebyside.py runs/$a-$run runs/$b-$run '$seg' /tmp/$file --after 3.5" > /dev/null \
        && $SCP -q "gpu:/tmp/$file" "$out/$file" && echo "$out/$file" \
        || echo "FEHLER: $file"
done
