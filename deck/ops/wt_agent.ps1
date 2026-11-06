# Startet einen Claude-Agenten in einem Windows-Terminal-Pane, das das Agent Deck
# geoeffnet hat (ops/wt_host.py). Laeuft mit -NoExit: endet claude, bleibt die Shell im
# Pane stehen - wie ein VS-Code-Terminal nach `exit` aus claude.
#
# -Spec ist Base64-JSON aus domain/wt.launch_spec: { slot, state, args }.
param([Parameter(Mandatory = $true)][string]$Spec)

$s = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Spec)) | ConvertFrom-Json

# 1) Die PID-Datei ZUERST: an ihr erkennt das Deck, dass der Slot lebt. Startzeit dazu,
#    damit eine spaeter neu vergebene PID nicht als dieser Slot durchgeht.
$me = Get-Process -Id $PID
$rec = @{ pid = $PID; created = $me.StartTime.ToFileTimeUtc(); slot = $s.slot } | ConvertTo-Json -Compress
$path = Join-Path $s.state ($s.slot + '.wtpid')
[IO.Directory]::CreateDirectory($s.state) | Out-Null
[IO.File]::WriteAllText($path + '.tmp', $rec)
Move-Item -LiteralPath ($path + '.tmp') -Destination $path -Force

# 2) Den Slot fuer die Hooks (report.py/statusline.py lesen AGENT_SLOT). wt.exe kann einem
#    neuen Pane keine Umgebungsvariable mitgeben, darum steht das hier.
$env:AGENT_SLOT = $s.slot

# 3) claude mit den Argumenten aus dem Deck (Modell, beim Neustart --resume).
$argv = @()
if ($s.args) { $argv = @($s.args) }
& claude @argv
