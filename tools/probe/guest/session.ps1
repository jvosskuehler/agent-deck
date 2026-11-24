# Führt einen Befehl in der angemeldeten Desktop-Sitzung aus (geplante Aufgabe mit /IT)
# und wartet darauf. Über SSH läuft man in Sitzung 0 ohne Bildschirm: Fenster, Hotkeys
# und Bildschirmaufnahme gehen nur in der Sitzung des angemeldeten Benutzers.
#
#   session.ps1 -File C:\probe\x.ps1 [-Params '-Name a'] [-Wait 120]
#
# Ausgabe des Skripts landet in <File>.out und wird am Ende ausgegeben.
param([Parameter(Mandatory = $true)][string]$File, [string]$Params = '', [int]$Wait = 120)

$id = Get-Random
$done = "C:\probe\run-$id.done"
$out = "$File.out"
Remove-Item $out -ErrorAction SilentlyContinue
$wrap = "C:\probe\run-$id.ps1"
Set-Content $wrap @"
`$ProgressPreference = 'SilentlyContinue'
try { & '$File' $Params *>&1 | Out-File -Encoding utf8 '$out' } catch { "FEHLER: `$_" | Out-File -Append -Encoding utf8 '$out' } finally { Set-Content '$done' 1 }
"@ -Encoding UTF8
# schtasks verliert Anführungszeichen im /TR-Text: der Pfad mit Leerzeichen steckt darum
# in einer kleinen cmd-Datei.
$cmd = "C:\probe\run-$id.cmd"
Set-Content $cmd "@`"C:\Program Files\PowerShell\7\pwsh.exe`" -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File $wrap" -Encoding ASCII
$task = "deckprobe-$id"
schtasks /Create /F /TN $task /SC ONCE /ST 23:59 /IT /RL HIGHEST /RU Administrator /TR $cmd | Out-Null
schtasks /Run /TN $task | Out-Null
$t = 0
while (-not (Test-Path $done) -and $t -lt $Wait) { Start-Sleep -Milliseconds 500; $t += 0.5 }
schtasks /Delete /F /TN $task | Out-Null
Remove-Item $wrap, $done, $cmd -ErrorAction SilentlyContinue
if (Test-Path $out) { Get-Content $out }
if ($t -ge $Wait) { 'session.ps1: Zeit abgelaufen' }
