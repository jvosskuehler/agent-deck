<#
.SYNOPSIS
Entfernt das Agent Deck für den angemeldeten Benutzer.

.DESCRIPTION
Läuft unter Windows PowerShell 5.1 und PowerShell 7. Gestartet wird es meist über
Windows-Einstellungen > Apps > Agent Deck.

Nimmt alles wieder heraus, was der Installer hinzugefügt hat (deck-setup uninstall, laut
installed.json): Einträge und Vorgaben in den Claude-Einstellungen (Vorgaben nur, wenn
sie noch so sind, wie der Installer sie geschrieben hat), die frühere Infozeile kommt
zurück; Startmenü, Terminal-Profil, Schrift, Farben, Autostart und den Eintrag unter
„Apps“. Danach die Programme und, nach Rückfrage, die Daten des Decks.

  -KeepData     Einstellungen und Protokoll des Decks behalten, ohne Rückfrage
  -RemoveData   auch sie löschen, ohne Rückfrage
  -Quiet        nichts fragen (ohne -RemoveData bleiben die Daten), knappe Ausgabe

Liegt das Skript im Programmordner, kopiert es sich zuerst nach %TEMP% und läuft von
dort weiter, damit der Ordner gelöscht werden kann.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\claude-agent-deck\bin\uninstall.ps1"
#>
[CmdletBinding()]
param(
    [switch]$KeepData,
    [switch]$RemoveData,
    [switch]$Quiet,
    # Intern: die Kopie in %TEMP% läuft; am Ende auf Enter warten (eigenes Fenster).
    [switch]$Relaunched,
    [switch]$PauseAtEnd
)

$ErrorActionPreference = 'Stop'
$AppDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck'
$Bin = Join-Path $AppDir 'bin'
$Setup = Join-Path $Bin 'deck-setup.exe'
$Problems = 0
$script:Tags = @()

function Say($text) { if (-not $Quiet) { Write-Host "  $text" } }
function Ok($text) { if (-not $Quiet) { Write-Host "  ✓ $text" -ForegroundColor Green } }
function Hint($text) { Write-Host "  Hinweis: $text" -ForegroundColor Yellow }
function Problem($text) { Write-Host "  FEHLER: $text" -ForegroundColor Red; $script:Problems++ }

# Ist dieses Konsolenfenster nur für uns da (gestartet aus „Apps“), nicht aus einer Shell?
# Dann schließt es sich am Ende sofort, und das Ergebnis wäre nicht lesbar.
function Test-OwnConsole {
    try {
        if (-not ('AgentDeck.Console' -as [type])) {
            Add-Type -Namespace AgentDeck -Name Console -MemberDefinition '[DllImport("kernel32.dll")] public static extern uint GetConsoleProcessList(uint[] list, uint count);'
        }
        $list = New-Object 'uint32[]' 16
        return ([AgentDeck.Console]::GetConsoleProcessList($list, 16) -le 1)
    } catch { return $false }
}

# Aus bin heraus gestartet: Kopie nach %TEMP% und dort weiter, damit bin gelöscht werden kann.
if (-not $Relaunched -and $PSCommandPath) {
    $here = [IO.Path]::GetFullPath($PSCommandPath)
    $own = (-not $Quiet) -and (Test-OwnConsole)
    if ($here.StartsWith([IO.Path]::GetFullPath($Bin).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        $tmpDir = [IO.Path]::GetTempPath()
        $copy = Join-Path $tmpDir ('agent-deck-uninstall-' + [Guid]::NewGuid().ToString('N').Substring(0, 8) + '.ps1')
        Copy-Item -LiteralPath $here -Destination $copy -Force
        $pass = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $copy, '-Relaunched')
        if ($KeepData) { $pass += '-KeepData' }
        if ($RemoveData) { $pass += '-RemoveData' }
        if ($Quiet) { $pass += '-Quiet' }
        if ($own) { $pass += '-PauseAtEnd' }
        Set-Location -LiteralPath $tmpDir
        [Environment]::CurrentDirectory = $tmpDir
        $exe = (Get-Process -Id $PID).Path
        & $exe @pass
        exit $LASTEXITCODE
    }
    if ($own) { $PauseAtEnd = $true }
}

# deck-setup aufrufen; Hinweise und Fehler zeigen, Merker sammeln, den Rest mit -Verbose.
function Invoke-Setup([string[]]$SetupArgs) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Setup @SetupArgs 2>&1 } finally { $ErrorActionPreference = $old }
    $code = $LASTEXITCODE
    foreach ($line in @($out | ForEach-Object { "$_" })) {
        if ($line -match '^## fails=') { continue }
        if ($line -match '^## (.+)$') { $script:Tags += $Matches[1]; continue }
        if ($line -match '^warn: (.*)$') { Hint $Matches[1] }
        elseif ($line -match '^fail: (.*)$') { Problem $Matches[1] }
        else { Write-Verbose "deck-setup: $line" }
    }
    return ($code -eq 0)
}

# Ohne deck-setup (Programmordner beschädigt): wenigstens, was eindeutig dem Deck gehört.
function Remove-Fallback {
    Hint 'Das Aufräumprogramm fehlt. Die Claude-Einstellungen bitte von Hand prüfen.'
    Remove-Item -LiteralPath (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk') -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\Fragments\AgentDeck') -Recurse -Force -ErrorAction SilentlyContinue
    Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'AgentDeck' -ErrorAction SilentlyContinue
    Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentDeck' -Recurse -Force -ErrorAction SilentlyContinue
}

function Remove-Folder([string]$Path) {
    for ($i = 1; $i -le 5; $i++) {
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
        if (-not (Test-Path -LiteralPath $Path)) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

$oldEncoding = $null
try { $oldEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { }
try {
    if (-not $Quiet) { Write-Host 'Agent Deck wird entfernt.' -ForegroundColor Cyan; Write-Host '' }

    # deck-watchdog gab es bis 24.11.2026; ein alter, noch laufender startete die App sonst neu.
    foreach ($name in 'deck-watchdog', 'agent-deck') {
        foreach ($p in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Milliseconds 700
    Ok 'Deck beendet'

    if (Test-Path -LiteralPath $Setup) {
        if (Invoke-Setup @('uninstall')) {
            if ($script:Tags -contains 'statusline-restored') { Ok 'Claude-Einstellungen zurückgesetzt, deine frühere Infozeile ist wieder da' }
            else { Ok 'Claude-Einstellungen zurückgesetzt' }
            Ok 'Startmenü, Terminal-Profil, Schrift, Farben und Autostart entfernt'
        }
    } else { Remove-Fallback }

    $remove = $false
    if ($RemoveData) { $remove = $true }
    elseif (-not $KeepData -and -not $Quiet -and (Test-Path -LiteralPath $AppDir)) {
        Write-Host ''
        $a = Read-Host 'Auch deine Deck-Einstellungen und das Protokoll löschen? (j/N)'
        $remove = $a -match '^\s*(j|ja|y|yes)\s*$'
    }
    # Kein Arbeitsordner darin, sonst hielte er ihn fest.
    Set-Location -LiteralPath ([IO.Path]::GetTempPath())
    if ($remove) {
        if (Remove-Folder $AppDir) { Ok 'Programme und Daten gelöscht' }
        else { Hint "$AppDir ließ sich nicht ganz löschen (Datei in Benutzung?)." }
    } elseif (Test-Path -LiteralPath $Bin) {
        if (Remove-Folder $Bin) { Ok 'Programme gelöscht, deine Deck-Einstellungen bleiben' }
        else { Hint "$Bin ließ sich nicht ganz löschen (Datei in Benutzung?)." }
    }
} catch {
    Problem $_.Exception.Message
} finally {
    if ($oldEncoding) { try { [Console]::OutputEncoding = $oldEncoding } catch { } }
}

if (-not $Quiet -or $Problems -gt 0) { Write-Host '' }
if ($Problems -gt 0) { Write-Host 'Agent Deck ist entfernt, aber nicht ganz: siehe oben.' -ForegroundColor Yellow }
elseif (-not $Quiet) { Write-Host 'Fertig. Agent Deck ist entfernt.' -ForegroundColor Green }
if ($PauseAtEnd -and -not $Quiet) { [void](Read-Host 'Enter drücken zum Schließen') }
# Die Kopie in %TEMP% räumt sich selbst weg; PowerShell hat sie schon gelesen.
if ($Relaunched -and $PSCommandPath) { Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue }
if ($Problems -gt 0) { exit 1 }
exit 0
