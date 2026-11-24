# Ein Deck-Paket in der VM installieren, in der Desktop-Sitzung (session.ps1). Mit
# -Fresh beginnt alles von vorn: Agenten und Daten des Decks weg, Einstellungen wie im
# Messstand (Repos-Ordner C:\Repos, nichts mit Wirkung nach außen, Glow wie beim Autor).
param([Parameter(Mandatory = $true)][string]$Zip, [switch]$Fresh)
$ErrorActionPreference = 'Continue'
$data = Join-Path $env:LOCALAPPDATA 'claude-agent-deck'
Get-Process agent-deck, deck-watchdog -ErrorAction SilentlyContinue | Stop-Process -Force
if ($Fresh) {
    Get-Process WindowsTerminal, OpenConsole -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep 1
    Remove-Item $data -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $data | Out-Null
    $glow = '{"light":{"kind":"weichlicht","animation":"still","color":"#FF4B00","outer":"#FF3300","intensity":1.05,"width":0.7,"speed":1.0},"dark":{"kind":"weichlicht","animation":"still","color":"#796200","outer":"#B76400","intensity":0.75,"width":0.95,"speed":1.0}}'
    $settings = '{"settings_version":1,"repos_root":"C:\\Repos","dock_edge":"right","appearance":"light","glow":' + $glow +
        ',"auto_permission_mode":false,"auto_compact":false}'
    [IO.File]::WriteAllText((Join-Path $data 'deck_settings.json'), $settings)
}
$pkg = Join-Path C:\probe ('pkg-' + [IO.Path]::GetFileNameWithoutExtension($Zip))
Remove-Item $pkg -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive $Zip $pkg -Force
# Eigener Prozess mit Ausgabe in Dateien: in der versteckten Aufgabe hat pwsh keine
# Konsole, an die ein direkt aufgerufenes Konsolenprogramm sich hängen könnte.
$log = Join-Path C:\probe 'install-log.txt'
# Nicht -Wait: das wartet auf alle Nachkommen, also auch auf das gestartete Deck.
$p = Start-Process powershell -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError "$log.err" `
    -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $pkg 'install.ps1')
$p.WaitForExit()
Get-Content $log, "$log.err" -ErrorAction SilentlyContinue | Select-Object -Last 12
Start-Sleep 3
# Der Installer startet das Deck ohne DevTools-Port: das macht scenario.ps1 selbst.
Get-Process agent-deck, deck-watchdog -ErrorAction SilentlyContinue | Stop-Process -Force
"installiert: $(Get-Content (Join-Path $data 'bin\version.txt') -ErrorAction SilentlyContinue)"
