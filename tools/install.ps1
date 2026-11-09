#Requires -Version 7.0
<#
.SYNOPSIS
Installiert oder aktualisiert das Agent Deck auf diesem Rechner.

.DESCRIPTION
  1. baut die Release-Fassung (überspringen mit -SkipBuild): App, Hilfsprogramme, deck-setup
  2. beendet deck-watchdog und agent-deck (der Wächter zuerst, sonst startet er die App neu)
  3. kopiert die Programme nach %LOCALAPPDATA%\claude-agent-deck\bin
  4. richtet die Hooks in ~/.claude/settings.json auf diese Programme (deck-setup install)
     und prüft sie (deck-setup check)
  5. startet agent-deck.exe und prüft, dass es läuft und den Broker-Port 8765 hält

Ein zweiter Lauf ist ein Update: deck-setup ändert dann nichts mehr an settings.json.
Entfernen der Hooks: %LOCALAPPDATA%\claude-agent-deck\bin\deck-setup.exe uninstall
#>
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$Repo = Split-Path $PSScriptRoot -Parent
$Bin = Join-Path $env:LOCALAPPDATA 'claude-agent-deck\bin'
$Release = Join-Path $Repo 'target\release'
$Setup = Join-Path $Bin 'deck-setup.exe'
$Exes = @('agent-deck.exe', 'deck-report.exe', 'deck-statusline.exe', 'deck-console.exe', 'deck-watchdog.exe', 'deck-setup.exe')
$BrokerPort = 8765

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Ok($text) { Write-Host "   ok: $text" -ForegroundColor Green }
function Fail($text) { Write-Host "   FEHLER: $text" -ForegroundColor Red; exit 1 }

function Invoke-Setup([string[]]$SetupArgs) {
    $out = & $Setup @SetupArgs 2>&1
    $out | ForEach-Object { Write-Host "   $_" }
    if ($LASTEXITCODE -ne 0) { Fail "deck-setup $($SetupArgs -join ' ') ist gescheitert" }
}

# ── 1. Bauen ──────────────────────────────────────────────────────────────
if (-not $SkipBuild) {
    Step 'Release bauen'
    $env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
    # Release muss in target\release landen, nicht in einem Agenten-Zielordner.
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Push-Location (Join-Path $Repo 'app')
    try {
        pnpm install --silent
        if ($LASTEXITCODE -ne 0) { Fail 'pnpm install ist gescheitert' }
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

foreach ($exe in $Exes) {
    if (-not (Test-Path (Join-Path $Release $exe))) { Fail "$exe fehlt in $Release (ohne -SkipBuild bauen)" }
}

# ── 2. Laufendes Deck beenden ─────────────────────────────────────────────
Step 'Laufendes Deck beenden'
foreach ($name in 'deck-watchdog', 'agent-deck') {
    $procs = Get-Process -Name $name -ErrorAction SilentlyContinue
    foreach ($p in $procs) { Stop-Process -Id $p.Id -Force; Ok "$name beendet (PID $($p.Id))" }
}
# Die Exe ist erst nach dem Prozessende überschreibbar.
Start-Sleep -Milliseconds 500

# ── 3. Kopieren ───────────────────────────────────────────────────────────
Step "Programme nach $Bin kopieren"
New-Item -ItemType Directory -Force -Path $Bin | Out-Null
foreach ($exe in $Exes) { Copy-Item (Join-Path $Release $exe) (Join-Path $Bin $exe) -Force }
Ok ($Exes -join ', ')

# ── 4. Hooks ──────────────────────────────────────────────────────────────
Step 'Hooks in ~/.claude/settings.json einrichten und prüfen'
Invoke-Setup @('install', '--bin', $Bin)
Invoke-Setup @('check', '--bin', $Bin)

# ── 5. Starten und prüfen ─────────────────────────────────────────────────
Step 'Deck starten'
Start-Process -FilePath (Join-Path $Bin 'agent-deck.exe') -WorkingDirectory $Bin

Step 'Prüfen'
# Der Port kann kurz nach dem Beenden der alten Instanz noch belegt sein: bis zu 15 s warten.
$deadline = (Get-Date).AddSeconds(15)
$app = $null; $owner = $null
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 500
    $app = Get-Process -Name 'agent-deck' -ErrorAction SilentlyContinue
    $listen = Get-NetTCPConnection -LocalPort $BrokerPort -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
    $owner = if ($listen) { (Get-Process -Id $listen.OwningProcess -ErrorAction SilentlyContinue).ProcessName } else { $null }
    if ($app -and $owner -eq 'agent-deck') { break }
}
if (-not $app) { Fail 'agent-deck.exe läuft nicht. Protokoll: %LOCALAPPDATA%\claude-agent-deck\agent-deck.log' }
Ok "agent-deck läuft (PID $($app.Id -join ', '))"
if (-not $owner) { Fail "Broker-Port $BrokerPort lauscht nicht" }
if ($owner -ne 'agent-deck') { Fail "Port $BrokerPort gehört $owner, nicht agent-deck" }
Ok "Broker-Port $BrokerPort gehört dem Deck; VS-Code-Fenster verbinden sich von selbst."

Write-Host "`nAgent Deck installiert." -ForegroundColor Green
