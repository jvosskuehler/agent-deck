<#
.SYNOPSIS
Baut das Installationspaket dist\agent-deck-<version>-win-x64.zip.

.DESCRIPTION
Läuft unter Windows PowerShell 5.1 und PowerShell 7 (auch in GitHub Actions).

  1. baut die Release-Fassung (überspringen mit -SkipBuild: nimmt target\release)
  2. legt die sechs Programme, install.ps1, uninstall.ps1, LICENSE und version.txt flach
     in das Zip, dazu resources\papier (Schrift, Farben und Shader für den Papier-Look);
     Rechtsklick > Alle extrahieren ergibt genau einen Ordner
  3. legt install.ps1 zusätzlich einzeln nach dist\ (Release-Anhang für den Einzeiler)

Dazu deck-report und deck-statusline als statische Linux-Programme (linux\ im Zip) für
Agenten in WSL: gebaut mit dem lld, den Rust mitbringt (.cargo\config.toml), also ohne
WSL, gcc oder zig. Das Ziel holt das Skript selbst (rustup target add).

Die Version steht in app\src-tauri\tauri.conf.json.
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
$Repo = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $Repo 'dist' }
$Release = Join-Path $Repo 'target\release'
$LinuxTarget = 'x86_64-unknown-linux-musl'
$LinuxRelease = Join-Path $Repo "target\$LinuxTarget\release"
$LinuxHooks = @('deck-report', 'deck-statusline')
$Exes = @('agent-deck.exe', 'deck-report.exe', 'deck-statusline.exe', 'deck-console.exe', 'deck-setup.exe', 'deck-wsl.exe')

function Fail($text) { Write-Host "FEHLER: $text" -ForegroundColor Red; exit 1 }

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Exe @Arguments } finally { $ErrorActionPreference = $old }
    if ($LASTEXITCODE -ne 0) { Fail "$Exe $($Arguments -join ' ') ist gescheitert (Exit $LASTEXITCODE)" }
}

$conf = Get-Content -Raw -LiteralPath (Join-Path $Repo 'app\src-tauri\tauri.conf.json') -Encoding UTF8 | ConvertFrom-Json
$Version = $conf.version
if (-not $Version) { Fail 'Keine Version in app\src-tauri\tauri.conf.json' }
$Name = "agent-deck-$Version-win-x64"

if (-not $SkipBuild) {
    $cargoHome = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path -LiteralPath $cargoHome) { $env:PATH = $cargoHome + ';' + $env:PATH }
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Push-Location (Join-Path $Repo 'app')
    try {
        Invoke-Native 'pnpm' @('install', '--frozen-lockfile')
        Invoke-Native 'pnpm' @('helpers:release')
        Invoke-Native 'pnpm' @('tauri', 'build', '--no-bundle')
    } finally { Pop-Location }
    Push-Location $Repo
    try {
        Invoke-Native 'cargo' @('build', '--release', '-p', 'deck-services', '--bin', 'deck-setup', '--bin', 'deck-wsl')
        Invoke-Native 'rustup' @('target', 'add', $LinuxTarget)
        Invoke-Native 'cargo' @('build', '--release', '-p', 'deck-hooks', '--target', $LinuxTarget)
    } finally { Pop-Location }
}

foreach ($exe in $Exes) {
    if (-not (Test-Path -LiteralPath (Join-Path $Release $exe))) { Fail "$exe fehlt in $Release (ohne -SkipBuild bauen)" }
}
foreach ($h in $LinuxHooks) {
    if (-not (Test-Path -LiteralPath (Join-Path $LinuxRelease $h))) { Fail "$h (Linux) fehlt in $LinuxRelease (ohne -SkipBuild bauen)" }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stage = Join-Path $OutDir $Name
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
foreach ($exe in $Exes) { Copy-Item -LiteralPath (Join-Path $Release $exe) -Destination $stage }
New-Item -ItemType Directory -Force -Path (Join-Path $stage 'linux') | Out-Null
foreach ($h in $LinuxHooks) { Copy-Item -LiteralPath (Join-Path $LinuxRelease $h) -Destination (Join-Path $stage 'linux') }
foreach ($f in 'tools\install.ps1', 'tools\install-wsl.ps1', 'tools\uninstall.ps1', 'LICENSE') { Copy-Item -LiteralPath (Join-Path $Repo $f) -Destination $stage }
$papier = Join-Path $Repo 'resources\papier'
if (-not (Test-Path -LiteralPath (Join-Path $papier 'fonts\IBMPlexMono-Regular.ttf'))) { Fail "$papier ist unvollständig" }
New-Item -ItemType Directory -Force -Path (Join-Path $stage 'resources') | Out-Null
Copy-Item -LiteralPath $papier -Destination (Join-Path $stage 'resources\papier') -Recurse
[IO.File]::WriteAllText((Join-Path $stage 'version.txt'), "$Version`r`n", (New-Object Text.UTF8Encoding $false))

$zip = Join-Path $OutDir "$Name.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal
Copy-Item -LiteralPath (Join-Path $Repo 'tools\install.ps1') -Destination (Join-Path $OutDir 'install.ps1') -Force
Remove-Item -LiteralPath $stage -Recurse -Force

$size = [math]::Round((Get-Item -LiteralPath $zip).Length / 1MB, 1)
Write-Host "Paket: $zip ($size MB)" -ForegroundColor Green
# Für GitHub Actions: Pfad und Version als Ausgabe des Schritts.
if ($env:GITHUB_OUTPUT) {
    Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "zip=$zip"
    Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "version=$Version"
}
