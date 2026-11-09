#Requires -Version 7.0
<#
.SYNOPSIS
Stellt vom Python-Deck auf das neue Agent Deck (Tauri) um. Rückweg: tools\zurueck.ps1

.DESCRIPTION
Drei Schritte, jeder für sich lauffähig:

  umstellen.ps1            Panel tauschen. Baut die Release-Fassung (überspringen mit
                           -SkipBuild), kopiert agent-deck.exe und die Hilfsprogramme nach
                           %LOCALAPPDATA%\claude-agent-deck\bin, beendet das Python-Panel und
                           startet das neue. ~/.claude/settings.json bleibt unberührt: die
                           Python-Hooks schreiben dasselbe Dateiformat, das neue Deck liest es.

  umstellen.ps1 -Hooks     Neue Hooks ZUSÄTZLICH eintragen (Python-Einträge bleiben) und die
                           Befehlszeile beweisen, so wie Claude Code sie ausführt (Git Bash,
                           in einen Wegwerf-Ordner), statt sich auf den Exit-Code zu verlassen.
                           Ändert ~/.claude/settings.json: diesen Schritt startet Jorrit selbst.

  umstellen.ps1 -Finish    Python-Einträge heraus, statusLine auf deck-statusline.

Braucht PowerShell 7 (pwsh): die Hook-Probe gibt eine Befehlszeile mit Anführungszeichen an
Git Bash weiter, und erst pwsh 7 reicht sie unverändert durch.
#>
param([switch]$SkipBuild, [switch]$Hooks, [switch]$Finish)

$ErrorActionPreference = 'Stop'
$Repo = Split-Path $PSScriptRoot -Parent
$Bin = Join-Path $env:LOCALAPPDATA 'claude-agent-deck\bin'
$Release = Join-Path $Repo 'target\release'
$Setup = Join-Path $Bin 'deck-setup.exe'
$Exes = @('agent-deck.exe', 'deck-report.exe', 'deck-statusline.exe', 'deck-console.exe', 'deck-watchdog.exe', 'deck-setup.exe')

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Ok($text) { Write-Host "   ok: $text" -ForegroundColor Green }
function Fail($text) { Write-Host "   FEHLER: $text" -ForegroundColor Red; exit 1 }

function Invoke-Setup([string[]]$SetupArgs) {
    if (-not (Test-Path $Setup)) { Fail "$Setup fehlt, erst umstellen.ps1 ohne Schalter ausführen" }
    $out = & $Setup @SetupArgs 2>&1
    $out | ForEach-Object { Write-Host "   $_" }
    if ($LASTEXITCODE -ne 0) { Fail "deck-setup $($SetupArgs -join ' ') ist gescheitert" }
}

# ── Schritt 3: Python-Einträge heraus ─────────────────────────────────────
if ($Finish) {
    Step 'Python-Einträge herausnehmen, statusLine umstellen'
    Invoke-Setup @('remove-legacy', '--bin', $Bin)
    Invoke-Setup @('check', '--bin', $Bin)
    Ok 'Umstellung abgeschlossen. Rückweg weiterhin: tools\zurueck.ps1'
    exit 0
}

# ── Schritt 2: neue Hooks dazu und beweisen ───────────────────────────────
if ($Hooks) {
    Step 'Hooks zusätzlich eintragen (Python-Einträge bleiben)'
    Invoke-Setup @('install', '--bin', $Bin)

    Step 'Hook-Befehl beweisen, so wie Claude Code ihn ausführt'
    $settings = Get-Content (Join-Path $env:USERPROFILE '.claude\settings.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $cmd = $settings.hooks.PreToolUse | ForEach-Object { $_.hooks } | Where-Object { $_.command -like '*deck-report*' } | Select-Object -First 1 -ExpandProperty command
    if (-not $cmd) { Fail 'kein deck-report-Eintrag unter PreToolUse' }
    $bash = Join-Path $env:ProgramFiles 'Git\bin\bash.exe'
    if (-not (Test-Path $bash)) { Fail "Git Bash nicht gefunden: $bash" }
    $probe = Join-Path ([IO.Path]::GetTempPath()) ('deck-probe-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $probe | Out-Null
    $oldLad = $env:LOCALAPPDATA; $oldSlot = $env:AGENT_SLOT
    try {
        $env:LOCALAPPDATA = $probe
        $env:AGENT_SLOT = 'ZZ9'
        '{"hook_event_name":"PreToolUse","session_id":"probe","cwd":"C:\\","tool_name":"Bash"}' | & $bash -c $cmd
        $state = Join-Path $probe 'claude-agent-deck\state\ZZ9.json'
        if (-not (Test-Path $state)) { Fail "deck-report hat nichts geschrieben (erwartet: $state). Befehl: $cmd" }
        Ok "deck-report schreibt über Git Bash: $cmd"
    } finally {
        $env:LOCALAPPDATA = $oldLad; $env:AGENT_SLOT = $oldSlot
        Remove-Item $probe -Recurse -Force -ErrorAction SilentlyContinue
    }
    Invoke-Setup @('check', '--bin', $Bin)
    Write-Host "`nHooks eingetragen. Wenn alles läuft: tools\umstellen.ps1 -Finish" -ForegroundColor Green
    exit 0
}

# ── Schritt 1: Panel tauschen ─────────────────────────────────────────────
if (-not $SkipBuild) {
    Step 'Release bauen'
    $env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
    # Release muss in target\release landen, nicht in einem Agenten-Zielordner.
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Push-Location (Join-Path $Repo 'app')
    try {
        pnpm install --silent
        pnpm helpers:release
        if ($LASTEXITCODE -ne 0) { Fail 'Hilfsprogramme nicht gebaut' }
        pnpm tauri build --no-bundle
        if ($LASTEXITCODE -ne 0) { Fail 'App nicht gebaut' }
    } finally { Pop-Location }
    Push-Location $Repo
    try {
        cargo build --release -p deck-services --bin deck-setup
        if ($LASTEXITCODE -ne 0) { Fail 'deck-setup nicht gebaut' }
    } finally { Pop-Location }
}

Step "Programme nach $Bin kopieren"
foreach ($exe in $Exes) {
    if (-not (Test-Path (Join-Path $Release $exe))) { Fail "$exe fehlt in $Release" }
}
Get-Process -Name 'agent-deck', 'deck-watchdog' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
New-Item -ItemType Directory -Force -Path $Bin | Out-Null
foreach ($exe in $Exes) { Copy-Item (Join-Path $Release $exe) (Join-Path $Bin $exe) -Force }
Ok ($Exes -join ', ')

Step 'Python-Panel beenden, neues Deck starten'
$py = Get-CimInstance Win32_Process -Filter "Name='pythonw.exe' OR Name='python.exe'" | Where-Object { $_.CommandLine -like '*agent_deck.py*' -or $_.CommandLine -like '*\watchdog.py*' }
foreach ($p in $py) { Stop-Process -Id $p.ProcessId -Force; Ok "Python beendet (PID $($p.ProcessId))" }
Start-Sleep -Seconds 1
Start-Process -FilePath (Join-Path $Bin 'agent-deck.exe') -WorkingDirectory $Bin
Start-Sleep -Seconds 4

Step 'Prüfen'
$app = Get-Process -Name 'agent-deck' -ErrorAction SilentlyContinue
if (-not $app) { Fail 'agent-deck.exe läuft nicht. Rückweg: tools\zurueck.ps1' }
Ok "agent-deck läuft (PID $($app.Id -join ', '))"
$listen = Get-NetTCPConnection -LocalPort 8765 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $listen) { Fail 'Broker-Port 8765 lauscht nicht' }
$owner = (Get-Process -Id $listen.OwningProcess).ProcessName
if ($owner -ne 'agent-deck') { Fail "Port 8765 gehört $owner, nicht agent-deck" }
Ok 'Broker-Port 8765 gehört dem neuen Deck; VS-Code-Fenster verbinden sich von selbst neu.'

Write-Host "`nPanel getauscht. Nächster Schritt: tools\umstellen.ps1 -Hooks   Rückweg: tools\zurueck.ps1" -ForegroundColor Green
