#Requires -Version 7.0
<#
.SYNOPSIS
Rückweg vom neuen Agent Deck (Tauri) zum Python-Deck.

.DESCRIPTION
  1. beendet agent-deck.exe und deck-watchdog.exe
  2. stellt in ~/.claude/settings.json `hooks` und `statusLine` genau so wieder her, wie
     sie vor tools\umstellen.ps1 waren (alle anderen Einstellungen bleiben)
  3. startet das Python-Panel aus dem Haupt-Checkout

Braucht PowerShell 7 (pwsh): die Hook-Probe gibt eine Befehlszeile mit Anführungszeichen an Git Bash weiter, und erst pwsh 7 reicht sie unverändert durch.
#>
param([string]$PythonRepo = (Join-Path $env:USERPROFILE 'Repos\agent-deck'))

$ErrorActionPreference = 'Stop'
$Setup = Join-Path $env:LOCALAPPDATA 'claude-agent-deck\bin\deck-setup.exe'

Write-Host '== neues Deck beenden' -ForegroundColor Cyan
Get-Process -Name 'agent-deck', 'deck-watchdog' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

Write-Host '== hooks und statusLine zurückstellen' -ForegroundColor Cyan
$Snapshot = Join-Path $env:LOCALAPPDATA 'claude-agent-deck\settings.before-deck.json'
if (-not (Test-Path $Snapshot)) {
    Write-Host '   settings.json war nicht umgestellt (umstellen.ps1 -Hooks lief nie), nichts zu tun.' -ForegroundColor Green
} elseif (Test-Path $Setup) {
    & $Setup rollback
    if ($LASTEXITCODE -ne 0) { Write-Host '   FEHLER beim Zurückstellen, settings.json von Hand prüfen (Sicherungen liegen daneben als .bak-*)' -ForegroundColor Red; exit 1 }
} else {
    Write-Host "   $Setup fehlt; settings.json war nie umgestellt oder ist von Hand zu prüfen." -ForegroundColor Yellow
}

Write-Host '== Python-Panel starten' -ForegroundColor Cyan
$pyw = (Get-Command pythonw.exe -ErrorAction SilentlyContinue).Source
if (-not $pyw) { $pyw = Join-Path (Split-Path (Get-Command python.exe).Source) 'pythonw.exe' }
Start-Process -FilePath $pyw -ArgumentList ('"' + (Join-Path $PythonRepo 'agent_deck.py') + '"') -WorkingDirectory $PythonRepo
Write-Host '   Python-Deck gestartet. Zurück auf dem alten Stand.' -ForegroundColor Green
