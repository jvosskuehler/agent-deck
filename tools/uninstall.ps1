<#
.SYNOPSIS
Entfernt das Agent Deck für den angemeldeten Benutzer.

.DESCRIPTION
Läuft unter Windows PowerShell 5.1 und PowerShell 7.

  1. beendet Wächter und Deck
  2. nimmt Hooks und statusLine des Decks aus Claudes settings.json und legt eine früher
     ersetzte statusLine zurück (deck-setup uninstall)
  3. entfernt Startmenü-Verknüpfung, Windows-Terminal-Profil (Fragment) und Autostart
  4. löscht %LOCALAPPDATA%\claude-agent-deck (Einstellungen, Protokoll, Programme) nach
     Rückfrage; -KeepData lässt den Ordner bis auf die Programme stehen, -RemoveData
     löscht ohne Rückfrage

.EXAMPLE
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\claude-agent-deck\bin\uninstall.ps1"
#>
[CmdletBinding()]
param(
    [switch]$KeepData,
    [switch]$RemoveData
)

$ErrorActionPreference = 'Stop'
$AppDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck'
$Bin = Join-Path $AppDir 'bin'
$Setup = Join-Path $Bin 'deck-setup.exe'
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$Problems = 0

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Ok($text) { Write-Host "   ok: $text" -ForegroundColor Green }
function Warn($text) { Write-Host "   Hinweis: $text" -ForegroundColor Yellow; $script:Problems++ }

function Invoke-Setup([string[]]$SetupArgs) {
    if (-not (Test-Path -LiteralPath $Setup)) { Warn "$Setup fehlt, Schritt übersprungen: deck-setup $($SetupArgs -join ' ')"; return }
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Setup @SetupArgs 2>&1 } finally { $ErrorActionPreference = $old }
    $out | ForEach-Object { Write-Host "   $_" }
    if ($LASTEXITCODE -ne 0) { Warn "deck-setup $($SetupArgs -join ' ') ist gescheitert" }
}

$oldEncoding = $null
try { $oldEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { }
try {
    Step 'Deck und Wächter beenden'
    foreach ($name in 'deck-watchdog', 'agent-deck') {
        foreach ($p in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
            Ok "$name beendet (PID $($p.Id))"
        }
    }
    Start-Sleep -Milliseconds 700

    Step 'Hooks und statusLine aus Claudes settings.json nehmen'
    Invoke-Setup @('uninstall')

    Step 'Startmenü-Verknüpfung entfernen'
    Invoke-Setup @('shortcut', '--remove')

    Step 'Windows-Terminal-Profil entfernen'
    Invoke-Setup @('wt-profile', '--remove')

    Step 'Autostart entfernen'
    if (Get-ItemProperty -Path $RunKey -Name 'AgentDeck' -ErrorAction SilentlyContinue) {
        Remove-ItemProperty -Path $RunKey -Name 'AgentDeck'
        Ok 'HKCU\...\Run\AgentDeck entfernt'
    } else { Ok 'kein Autostart eingetragen' }

    Step "Daten in $AppDir"
    $remove = $false
    if ($RemoveData) { $remove = $true }
    elseif (-not $KeepData -and (Test-Path -LiteralPath $AppDir)) {
        Write-Host '   Dort liegen Einstellungen, Protokoll, Zustände der Agenten und die Programme.'
        $a = Read-Host '   Ordner ganz löschen? (j/N)'
        $remove = $a -match '^\s*(j|ja|y|yes)\s*$'
    }
    if ($remove) {
        # Die Programme sind beendet; dieses Skript ist schon gelesen und darf mit weg.
        # Ein Arbeitsordner darin hielte ihn fest.
        Set-Location -LiteralPath ([IO.Path]::GetTempPath())
        Remove-Item -LiteralPath $AppDir -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $AppDir) { Warn "$AppDir ließ sich nicht ganz löschen (Datei in Benutzung?)" }
        else { Ok "$AppDir gelöscht" }
    } else {
        # Ohne Daten-Löschen gehen nur die Programme; Einstellungen bleiben für eine
        # spätere Installation.
        Get-ChildItem -LiteralPath $Bin -File -Filter '*.exe' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
        Ok 'Programme entfernt, Einstellungen und Protokoll bleiben'
    }
} catch {
    Write-Host "   FEHLER: $($_.Exception.Message)" -ForegroundColor Red
    $Problems++
} finally {
    if ($oldEncoding) { try { [Console]::OutputEncoding = $oldEncoding } catch { } }
}

if ($Problems -gt 0) {
    Write-Host "`nAgent Deck entfernt, mit $Problems Hinweis(en) oben." -ForegroundColor Yellow
    if ($PSCommandPath) { exit 1 }
} else {
    Write-Host "`nAgent Deck entfernt." -ForegroundColor Green
}
