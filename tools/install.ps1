<#
.SYNOPSIS
Installiert oder aktualisiert das Agent Deck für den angemeldeten Benutzer.

.DESCRIPTION
Läuft unter Windows PowerShell 5.1 und PowerShell 7, ohne Administratorrechte.
Woher die Programme kommen:

  aus einem entpackten Paket   install.ps1 liegt neben agent-deck.exe (Vorgabe)
  -Package <zip>               aus einem heruntergeladenen Paket
  -Download                    neuestes Release von GitHub holen
  -FromSource                  aus dem Quellcode bauen (pnpm, cargo, node); Vorgabe, wenn
                               das Skript in tools\ eines Checkouts liegt
  -SkipBuild                   wie -FromSource, aber das schon gebaute target\release nehmen

Beim ersten Mal fragt der Installer nach der WSL-Distro, in der die Agenten laufen,
dem Repos-Ordner darin und dem Autostart. Ein zweiter Lauf ist ein Update und fragt
nichts. (-AgentEnv windows: Agenten mit PowerShell und Windows-claude, Repos-Ordner
unter Windows; ein Update behält, was eingestellt ist.)
Was er am Rechner ändert, steht in README.md; Einzelheiten in docs\BETRIEB.md.
-Verbose zeigt jeden Schritt.

Entfernen: Windows-Einstellungen > Apps > Agent Deck.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1 -NonInteractive -ReposRoot C:\Code -Autostart
#>
[CmdletBinding()]
param(
    [switch]$FromSource,
    [switch]$SkipBuild,
    [string]$Package,
    [switch]$Download,
    [switch]$NonInteractive,
    [string]$ReposRoot,
    # Nur für Fachleute: keep lässt eine fremde Infozeile allein stehen (dann fehlen
    # Modell, Effort und Kontext auf den Kacheln). Vorgabe: die des Decks, die alte dahinter.
    [ValidateSet('keep', 'replace')][string]$StatusLine = 'replace',
    [switch]$Autostart,
    [switch]$NoShortcut,
    # Wo die Agenten laufen. Ohne Angabe: neue Installation in WSL, ein Update wie bisher.
    [ValidateSet('wsl', 'windows')][string]$AgentEnv,
    [string]$WslDistro
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Fortschrittsbalken bremsen Downloads in 5.1 stark

$GitHubRepo = 'jvosskuehler/agent-deck'
$AppDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck'
$Bin = Join-Path $AppDir 'bin'
$SettingsFile = Join-Path $AppDir 'deck_settings.json'
$Setup = Join-Path $Bin 'deck-setup.exe'
$Exes = @('agent-deck.exe', 'deck-report.exe', 'deck-statusline.exe', 'deck-console.exe', 'deck-setup.exe', 'deck-wsl.exe')
# Programme früherer Fassungen, die ein Update aus bin\ entfernt.
$ObsoleteExes = @('deck-watchdog.exe')
$Winget = 'winget install --source winget -e --id'

# Fehler enden in Fail. Aus einer Datei gestartet beendet das den Prozess mit Exit 1; aus
# dem Speicher gestartet (Einzeiler) nur das Skript, das Fenster bleibt offen.
$script:Failed = $false
function Say($text) { Write-Host "  $text" }
function Ok($text) { Write-Host "  ✓ $text" -ForegroundColor Green }
function Hint($text) { Write-Host "  Hinweis: $text" -ForegroundColor Yellow }
function Fail($text) {
    Write-Host "  FEHLER: $text" -ForegroundColor Red
    $script:Failed = $true
    throw 'agent-deck-install-failed'
}

# ── Hilfen ────────────────────────────────────────────────────────────────

# Ein natives Programm aufrufen, ohne dass 5.1 seine stderr-Zeilen als Fehler wirft.
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Exe @Arguments } finally { $ErrorActionPreference = $old }
}

# deck-setup aufrufen. Gezeigt werden nur Hinweise und Fehler; alles andere mit -Verbose.
# Merker (`## <merker>`) landen in $script:Tags, daraus entsteht die Zusammenfassung.
function Invoke-Setup([string[]]$SetupArgs) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Setup @SetupArgs 2>&1 } finally { $ErrorActionPreference = $old }
    $code = $LASTEXITCODE
    foreach ($line in @($out | ForEach-Object { "$_" })) {
        if ($line -match '^## fails=') { continue }
        if ($line -match '^## (.+)$') { $script:Tags += $Matches[1]; continue }
        if ($line -match '^warn: (.*)$') { Hint $Matches[1] }
        elseif ($line -match '^fail: (.*)$') { Write-Host "  FEHLER: $($Matches[1])" -ForegroundColor Red }
        else { Write-Verbose "deck-setup $($SetupArgs[0]): $line" }
    }
    if ($code -ne 0) { Fail "Einrichtung gescheitert (deck-setup $($SetupArgs[0])). Mit -Verbose noch einmal starten, um mehr zu sehen." }
}

function Find-Command([string]$Name) {
    $c = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($c) { return $c.Source }
    return $null
}

function Test-Interactive {
    if ($NonInteractive) { return $false }
    if (-not [Environment]::UserInteractive) { return $false }
    try { if ([Console]::IsInputRedirected) { return $false } } catch { }
    return $true
}

function Read-Answer([string]$Prompt, [string]$Default) {
    $suffix = ''
    if ($Default) { $suffix = " [$Default]" }
    $a = Read-Host "$Prompt$suffix"
    if ([string]::IsNullOrWhiteSpace($a)) { return $Default }
    return $a.Trim()
}

function Read-YesNo([string]$Prompt, [bool]$Default) {
    $hint = 'j/N'
    if ($Default) { $hint = 'J/n' }
    while ($true) {
        $a = Read-Host "$Prompt ($hint)"
        if ([string]::IsNullOrWhiteSpace($a)) { return $Default }
        switch -regex ($a.Trim()) {
            '^(j|ja|y|yes)$' { return $true }
            '^(n|nein|no)$' { return $false }
        }
    }
}

# UTF-8 ohne BOM: serde_json liest kein BOM.
function Write-Utf8([string]$Path, [string]$Text) {
    [IO.File]::WriteAllText($Path, $Text, (New-Object Text.UTF8Encoding $false))
}

# ── Repos-Ordner ──────────────────────────────────────────────────────────

# Unterordner mit .git (Ordner oder Datei: ein worktree hat eine .git-Datei).
function Get-RepoCount([string]$Dir) {
    $n = 0
    try {
        foreach ($c in Get-ChildItem -LiteralPath $Dir -Directory -ErrorAction SilentlyContinue) {
            if (Test-Path -LiteralPath (Join-Path $c.FullName '.git')) { $n++ }
        }
    } catch { }
    return $n
}

function Get-ReposSuggestion {
    $home_ = $env:USERPROFILE
    foreach ($p in @((Join-Path $home_ 'Repos'), (Join-Path (Join-Path $home_ 'source') 'repos'))) {
        if (Test-Path -LiteralPath $p -PathType Container) { return $p }
    }
    # Sonst der Ordner (bis zwei Ebenen unter dem Profil) mit den meisten Repos darin.
    $best = $null; $bestN = 0
    $skip = @('AppData', 'Application Data', 'Local Settings', 'node_modules')
    $level1 = Get-ChildItem -LiteralPath $home_ -Directory -ErrorAction SilentlyContinue |
        Where-Object { $skip -notcontains $_.Name -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -and -not $_.Name.StartsWith('.') }
    $candidates = @()
    foreach ($d in $level1) {
        $candidates += $d.FullName
        $candidates += Get-ChildItem -LiteralPath $d.FullName -Directory -ErrorAction SilentlyContinue |
            Where-Object { -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -and -not $_.Name.StartsWith('.') -and $skip -notcontains $_.Name } |
            ForEach-Object { $_.FullName }
    }
    foreach ($c in $candidates) {
        $n = Get-RepoCount $c
        if ($n -gt $bestN) { $best = $c; $bestN = $n }
    }
    return $best
}

# 'ok', 'missing' oder 'norepos'.
function Test-ReposRoot([string]$Dir) {
    if ([string]::IsNullOrWhiteSpace($Dir) -or -not (Test-Path -LiteralPath $Dir -PathType Container)) { return 'missing' }
    if ((Get-RepoCount $Dir) -lt 1) { return 'norepos' }
    return 'ok'
}

function Resolve-ReposRoot([bool]$Ask) {
    if ($ReposRoot) {
        $p = [Environment]::ExpandEnvironmentVariables($ReposRoot)
        switch (Test-ReposRoot $p) {
            'ok' { return (Resolve-Path -LiteralPath $p).Path }
            # Ausdrücklich angegeben: das ist die Bestätigung.
            'norepos' { return (Resolve-Path -LiteralPath $p).Path }
            default { Fail "Den Ordner $p gibt es nicht." }
        }
    }
    $suggest = Get-ReposSuggestion
    if (-not $Ask) {
        if ($suggest -and (Test-ReposRoot $suggest) -eq 'ok') {
            Write-Verbose "Repos-Ordner: $suggest (gefunden)"
            return $suggest
        }
        Fail 'Kein Ordner mit Git-Repos gefunden. Bitte mit -ReposRoot <Ordner> angeben.'
    }
    while ($true) {
        $p = Read-Answer 'In welchem Ordner liegen deine Git-Repos?' $suggest
        if (-not $p) { continue }
        $p = [Environment]::ExpandEnvironmentVariables($p.Trim('"'))
        switch (Test-ReposRoot $p) {
            'ok' { return (Resolve-Path -LiteralPath $p).Path }
            'missing' { Hint 'Den Ordner gibt es nicht.' }
            'norepos' {
                if (Read-YesNo 'In diesem Ordner liegt kein Git-Repo. Trotzdem nehmen?' $false) { return (Resolve-Path -LiteralPath $p).Path }
            }
        }
    }
}

# Beim Windows-Start mitstarten? Angegeben (-Autostart) gilt, sonst fragt der Installer
# (Vorgabe ja); ohne Rückfrage bleibt es aus.
function Resolve-Autostart([bool]$Ask) {
    if ($script:AutostartGiven) { return [bool]$Autostart }
    if ($Ask) { return (Read-YesNo 'Deck beim Windows-Start mitstarten?' $true) }
    return $false
}

# Die Fragen beim ersten Lauf, je eine Funktion. Wer eine Frage dazubaut, hängt sie hier
# an und gibt ihre Antwort mit zurück. Bei Agenten in WSL ($Wsl aus install-wsl.ps1) ist
# der Repos-Ordner schon beantwortet, als Linux-Pfad.
function Get-FirstRunAnswers([bool]$Ask, $Wsl) {
    if ($Ask) { Write-Host '' }
    if ($Wsl) { $repos = $Wsl.Repos } else { $repos = Resolve-ReposRoot $Ask }
    $answers = @{
        Repos = $repos
        Autostart = (Resolve-Autostart $Ask)
    }
    if ($Ask) { Write-Host '' }
    return $answers
}

# ── Quelle der Programme ──────────────────────────────────────────────────

function Get-SourceVersion([string]$Repo) {
    try { return ((Get-Content -Raw -LiteralPath (Join-Path $Repo 'app\src-tauri\tauri.conf.json') -Encoding UTF8) | ConvertFrom-Json).version } catch { return 'dev' }
}

function Test-BuildTools {
    $cargoHome = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path -LiteralPath $cargoHome) { $env:PATH = $cargoHome + ';' + $env:PATH }
    $missing = @()
    foreach ($t in 'node', 'pnpm', 'cargo') { if (-not (Find-Command $t)) { $missing += $t } }
    if ($missing.Count -gt 0) {
        Fail ("Zum Bauen fehlt: $($missing -join ', '). Rust: https://rustup.rs, Node: $Winget OpenJS.NodeJS.LTS, " +
            'pnpm: npm install -g pnpm. Ohne Bauen: ein fertiges Paket mit -Download oder -Package <zip> installieren.')
    }
}

function Build-FromSource([string]$Repo) {
    Say 'Baue aus dem Quellcode …'
    Test-BuildTools
    # Release muss in target\release landen, nicht in einem Agenten-Zielordner.
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Push-Location (Join-Path $Repo 'app')
    try {
        Invoke-Native 'pnpm' @('install', '--silent')
        if ($LASTEXITCODE -ne 0) { Fail 'pnpm install ist gescheitert' }
        Invoke-Native 'pnpm' @('helpers:release')
        if ($LASTEXITCODE -ne 0) { Fail 'Hilfsprogramme nicht gebaut' }
        Invoke-Native 'pnpm' @('tauri', 'build', '--no-bundle')
        if ($LASTEXITCODE -ne 0) { Fail 'App nicht gebaut' }
    } finally { Pop-Location }
    Push-Location $Repo
    try {
        Invoke-Native 'cargo' @('build', '--release', '-p', 'deck-services', '--bin', 'deck-setup')
        if ($LASTEXITCODE -ne 0) { Fail 'deck-setup nicht gebaut' }
    } finally { Pop-Location }
}

# Ordner mit den Programmen in einem entpackten Paket (flach oder eine Ebene tiefer).
function Find-PackageRoot([string]$Dir) {
    if (Test-Path -LiteralPath (Join-Path $Dir 'agent-deck.exe')) { return $Dir }
    $sub = Get-ChildItem -LiteralPath $Dir -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'agent-deck.exe') } | Select-Object -First 1
    if ($sub) { return $sub.FullName }
    return $null
}

function Expand-Package([string]$Zip) {
    if (-not (Test-Path -LiteralPath $Zip)) { Fail "Das Paket $Zip gibt es nicht." }
    $dest = Join-Path ([IO.Path]::GetTempPath()) ('agent-deck-' + [Guid]::NewGuid().ToString('N').Substring(0, 8))
    # .NET statt Expand-Archive: in einer 5.1, die das Archive-Modul aus PowerShell 7 lädt
    # (PSModulePath geerbt), scheitert Expand-Archive an einer fehlenden Überladung.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $dest)
    $root = Find-PackageRoot $dest
    if (-not $root) { Fail "$Zip enthält kein agent-deck.exe." }
    return $root
}

function Get-ReleaseZip {
    Say 'Lade das neueste Paket von GitHub …'
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
    $api = "https://api.github.com/repos/$GitHubRepo/releases/latest"
    try {
        $rel = Invoke-RestMethod -Uri $api -UseBasicParsing -Headers @{ 'User-Agent' = 'agent-deck-installer'; 'Accept' = 'application/vnd.github+json' }
    } catch {
        $code = $null
        try { $code = [int]$_.Exception.Response.StatusCode } catch { }
        if ($code -eq 404) { Fail "Es gibt noch kein veröffentlichtes Paket (https://github.com/$GitHubRepo/releases). Bitte ein Paket besorgen und mit -Package <zip> installieren." }
        Fail "GitHub ist nicht erreichbar ($($_.Exception.Message)). Netz oder Proxy prüfen, oder das Paket von Hand laden und -Package <zip> nehmen."
    }
    $asset = $rel.assets | Where-Object { $_.name -match '^agent-deck-.+-win-x64\.zip$' } | Select-Object -First 1
    if (-not $asset) { Fail "Das Release $($rel.tag_name) enthält kein Paket agent-deck-*-win-x64.zip." }
    $zip = Join-Path ([IO.Path]::GetTempPath()) $asset.name
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing -Headers @{ 'User-Agent' = 'agent-deck-installer' }
    Write-Verbose "$($asset.name) ($($rel.tag_name))"
    return $zip
}

# Gibt @{ Dir = Ordner mit den Programmen; Version; Scripts = Ordner mit uninstall.ps1;
# Papier = Ordner mit Schrift und Farben }.
function Resolve-Source {
    $here = $PSScriptRoot
    $repo = $null
    if ($here) {
        $parent = Split-Path $here -Parent
        if ((Test-Path -LiteralPath (Join-Path $parent 'Cargo.toml')) -and (Test-Path -LiteralPath (Join-Path $parent 'app\package.json'))) { $repo = $parent }
    }
    if ($FromSource -or $SkipBuild) {
        if (-not $repo) { Fail '-FromSource und -SkipBuild gehen nur aus tools\ eines Checkouts.' }
    }
    $mode = $null
    if ($Package) { $mode = 'package' }
    elseif ($Download) { $mode = 'download' }
    elseif ($FromSource -or $SkipBuild) { $mode = 'source' }
    elseif ($here -and (Test-Path -LiteralPath (Join-Path $here 'agent-deck.exe'))) { $mode = 'folder' }
    elseif ($repo) { $mode = 'source' }
    else { $mode = 'download' }

    switch ($mode) {
        'source' {
            if (-not $SkipBuild) { Build-FromSource $repo }
            return @{ Dir = (Join-Path $repo 'target\release'); Version = (Get-SourceVersion $repo); Scripts = (Join-Path $repo 'tools'); Papier = (Join-Path $repo 'resources\papier') }
        }
        'folder' { $dir = $here }
        'package' {
            if (-not (Test-Path -LiteralPath $Package -PathType Leaf)) { Fail "Das Paket $Package gibt es nicht." }
            $dir = Expand-Package (Resolve-Path -LiteralPath $Package).Path
        }
        'download' { $dir = Expand-Package (Get-ReleaseZip) }
    }
    $version = 'unbekannt'
    $vf = Join-Path $dir 'version.txt'
    if (Test-Path -LiteralPath $vf) { $version = (Get-Content -LiteralPath $vf -TotalCount 1).Trim() }
    return @{ Dir = $dir; Version = $version; Scripts = $dir; Papier = (Join-Path $dir 'resources\papier') }
}

# ── Voraussetzungen ───────────────────────────────────────────────────────

function Test-WebView2 {
    $guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    foreach ($k in @("HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$guid", "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$guid", "HKCU:\Software\Microsoft\EdgeUpdate\Clients\$guid")) {
        $pv = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
        if ($pv -and $pv -ne '0.0.0.0') { return $pv }
    }
    return $null
}

function Find-Claude {
    $c = Find-Command 'claude'
    if ($c) { return $c }
    $local = Join-Path $env:USERPROFILE '.local\bin\claude.exe'
    if (Test-Path -LiteralPath $local) { return $local }
    return $null
}

function Test-Prerequisites {
    $bad = 0
    $wt = Find-Command 'wt.exe'
    if (-not $wt) {
        # Ohne Alias im PATH (Server, frisch per winget installiert) steht es noch als Paket da.
        try { $pkg = Get-AppxPackage -Name 'Microsoft.WindowsTerminal*' -ErrorAction SilentlyContinue | Select-Object -First 1 } catch { $pkg = $null }
        if ($pkg) { $wt = $pkg.InstallLocation }
    }
    if ($wt) { Ok 'Windows Terminal' }
    else { Write-Host "  ✗ Windows Terminal fehlt. Installieren: $Winget Microsoft.WindowsTerminal" -ForegroundColor Red; $bad++ }
    if (Test-WebView2) { Ok 'WebView2' }
    else { Write-Host "  ✗ WebView2 fehlt. Installieren: $Winget Microsoft.EdgeWebView2Runtime" -ForegroundColor Red; $bad++ }
    # Agenten in WSL brauchen claude in der Distro, nicht unter Windows (install-wsl.ps1).
    if ($script:AgentEnvChoice -eq 'wsl') { }
    elseif (Find-Claude) { Ok 'Claude Code' }
    else { Write-Host '  ✗ Claude Code fehlt. Installieren: irm https://claude.ai/install.ps1 | iex (danach ein neues Fenster öffnen)' -ForegroundColor Red; $bad++ }
    if ($bad -gt 0) { Fail 'Bitte das Fehlende installieren und den Installer noch einmal starten.' }
}

# ── Ablauf ────────────────────────────────────────────────────────────────

function Stop-Deck {
    # deck-watchdog gab es bis 24.11.2026; ein alter, noch laufender startete die App sonst neu.
    $any = $false
    foreach ($name in 'deck-watchdog', 'agent-deck') {
        foreach ($p in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
            Write-Verbose "$name beendet (PID $($p.Id))"
            $any = $true
        }
    }
    if ($any) { Start-Sleep -Milliseconds 700 }   # die Exe ist erst nach dem Prozessende überschreibbar
}

function Copy-Programs($Source) {
    foreach ($exe in $Exes) {
        if (-not (Test-Path -LiteralPath (Join-Path $Source.Dir $exe))) { Fail "$exe fehlt in $($Source.Dir)" }
    }
    New-Item -ItemType Directory -Force -Path $Bin | Out-Null
    # Reste früherer Updates (siehe unten), soweit nicht mehr in Benutzung.
    Get-ChildItem -LiteralPath $Bin -Filter '*.exe.old*' -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    foreach ($exe in $Exes) {
        $dst = Join-Path $Bin $exe
        # Ein eben beendeter Prozess gibt seine Exe manchmal erst Augenblicke später frei.
        for ($i = 1; ; $i++) {
            try { Copy-Item -LiteralPath (Join-Path $Source.Dir $exe) -Destination $dst -Force; break }
            catch {
                # Läuft die Exe weiter (deck-wsl.exe steht im Fenster jedes WSL-Agenten), darf
                # sie umbenannt werden; der Agent läuft mit der alten weiter, neue nehmen die neue.
                if ($i -ge 4 -and (Test-Path -LiteralPath $dst)) {
                    try { Move-Item -LiteralPath $dst -Destination "$dst.old-$([DateTime]::Now.Ticks)" -Force -ErrorAction Stop; continue } catch { }
                }
                if ($i -ge 10) { Fail "$exe ließ sich nicht kopieren: $($_.Exception.Message)" }
                Start-Sleep -Milliseconds 500
            }
        }
    }
    foreach ($old in $ObsoleteExes) {
        $path = Join-Path $Bin $old
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
            Write-Verbose "$old entfernt (gibt es nicht mehr)"
        }
    }
    $un = Join-Path $Source.Scripts 'uninstall.ps1'
    if (Test-Path -LiteralPath $un) { Copy-Item -LiteralPath $un -Destination (Join-Path $Bin 'uninstall.ps1') -Force }
    Write-Utf8 (Join-Path $Bin 'version.txt') "$($Source.Version)`r`n"
    # Aus dem Internet geladene Dateien tragen eine Herkunftsmarke; ohne sie fragt
    # SmartScreen nicht bei jedem Start.
    Get-ChildItem -LiteralPath $Bin -File | Unblock-File -ErrorAction SilentlyContinue
    Ok "Programme (Version $($Source.Version))"
}

function Write-DeckSettings([string]$Repos) {
    $s = [ordered]@{ settings_version = 1; repos_root = $Repos }
    # Alles mit Wirkung nach außen ist aus, bis es jemand in den Einstellungen anschaltet.
    # Auto-Compact ist an: es schreibt nur in den eigenen wartenden Agenten.
    $s.auto_permission_mode = $false
    $s.auto_compact = $true
    New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
    Write-Utf8 $SettingsFile ($s | ConvertTo-Json -Depth 4)
    Write-Verbose "Einstellungen: $SettingsFile"
}

function Start-Deck {
    Start-Process -FilePath (Join-Path $Bin 'agent-deck.exe') -WorkingDirectory $Bin
    $deadline = (Get-Date).AddSeconds(15)
    $app = $null
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $app = Get-Process -Name 'agent-deck' -ErrorAction SilentlyContinue
        if ($app) { break }
    }
    if (-not $app) { Fail "Das Deck ist nicht gestartet. Protokoll: $AppDir\agent-deck.log" }
    Ok 'Deck gestartet'
}

function Write-Summary([bool]$FirstRun, [string]$Version, [bool]$InStartMenu) {
    Write-Host ''
    if ($FirstRun -and $InStartMenu) { Write-Host 'Fertig. Das Deck läuft und steht im Startmenü.' -ForegroundColor Green }
    elseif ($FirstRun) { Write-Host 'Fertig. Das Deck läuft.' -ForegroundColor Green }
    else { Write-Host "Fertig. Agent Deck $Version ist aktualisiert und läuft." -ForegroundColor Green }
    if ($script:Tags -contains 'statusline') {
        $t = 'Die Infozeile unter dem Claude-Chat zeigt jetzt Modell, Effort und Kontext.'
        if ($script:Tags -contains 'statusline-chained') { $t += ' Deine bisherige Zeile steht dahinter.' }
        Write-Host $t
    }
    $new = @()
    foreach ($tag in $script:Tags) {
        if ($tag -match '^default permissions\.defaultMode=') { $new += 'Agenten fragen nicht nach (Modus Bypass)' }
        elseif ($tag -match '^default effortLevel=(.+)$') { $new += "Effort $($Matches[1])" }
    }
    if ($new.Count -gt 0) { Write-Host "Neu in den Claude-Einstellungen: $($new -join ', '). Änderbar im Deck unter Einstellungen." }
    Write-Host 'Deinstallieren: Windows-Einstellungen › Apps › Agent Deck'
}

# Wo die Agenten laufen: ausdrücklich angegeben, sonst wie eingestellt (ein Update ohne
# Eintrag läuft wie bisher unter Windows), sonst bei einer neuen Installation WSL.
function Get-AgentEnvChoice {
    if ($AgentEnv) { return $AgentEnv }
    if (Test-Path -LiteralPath $SettingsFile) {
        try { if ((Get-Content -LiteralPath $SettingsFile -Raw -Encoding UTF8 | ConvertFrom-Json).agent_env -eq 'wsl') { return 'wsl' } } catch { }
        return 'windows'
    }
    return 'wsl'
}

function Invoke-Main {
    Write-Host 'Agent Deck wird installiert.' -ForegroundColor Cyan
    Write-Host ''
    $script:AgentEnvChoice = Get-AgentEnvChoice
    Test-Prerequisites
    $source = Resolve-Source
    $firstRun = -not (Test-Path -LiteralPath $SettingsFile)
    $ask = Test-Interactive
    $wsl = $null
    if ($script:AgentEnvChoice -eq 'wsl') {
        . (Join-Path $source.Scripts 'install-wsl.ps1')
        if (-not $firstRun) { $wsl = Get-WslFromSettings $SettingsFile }
        if (-not $wsl) { $wsl = Initialize-WslAgents $ask (Join-Path $source.Dir 'deck-setup.exe') }
    }

    $answers = $null
    $autostartOn = $null
    if ($firstRun) {
        $answers = Get-FirstRunAnswers $ask $wsl
        $autostartOn = $answers.Autostart
    } elseif ($script:AutostartGiven) {
        $autostartOn = [bool]$Autostart
    }

    Stop-Deck
    Copy-Programs $source
    if ($wsl) { Copy-LinuxHooks $source }

    if ($wsl) { Install-WslHooks $wsl.Distro }
    else {
        Invoke-Setup @('install', '--bin', $Bin, '--statusline', $StatusLine)
        Invoke-Setup @('check', '--bin', $Bin)
    }
    # Erst nach den Hooks: scheitert ein erster Lauf vorher, fragt der nächste wieder.
    if ($firstRun -and -not (Test-Path -LiteralPath $SettingsFile)) {
        Write-DeckSettings $answers.Repos
        if ($wsl) { Set-WslDeckSettings $SettingsFile $wsl }
    }
    Ok 'Claude-Einstellungen'

    # Erst Schrift und Shader, dann das Terminal-Profil, das beide nennt.
    if ($wsl) { Invoke-Setup @('look', '--from', $source.Papier, '--wsl', $wsl.Distro) }
    else { Invoke-Setup @('look', '--from', $source.Papier) }
    Invoke-Setup @('wt-profile')
    if (Test-Path -LiteralPath $source.Papier) { Ok 'Schrift und Farben' }

    $link = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk'
    $inStartMenu = $false
    if (-not $NoShortcut -and ($firstRun -or (Test-Path -LiteralPath $link))) {
        Invoke-Setup @('shortcut', '--bin', $Bin)
        $inStartMenu = $true
    }
    if ($null -ne $autostartOn) {
        if ($autostartOn) { Invoke-Setup @('autostart', '--bin', $Bin) } else { Invoke-Setup @('autostart', '--remove') }
    }
    Invoke-Setup @('apps-entry', '--bin', $Bin, '--version', $source.Version)
    if ($inStartMenu) { Ok 'Startmenü' }

    Start-Deck
    Write-Summary $firstRun $source.Version $inStartMenu
}

# Ob ein Parameter angegeben wurde, sehen die Funktionen in ihrem eigenen
# $PSBoundParameters nicht: einmal hier oben festhalten.
$script:Tags = @()
$script:AutostartGiven = $PSBoundParameters.ContainsKey('Autostart')

# deck-setup schreibt UTF-8; ohne das zeigt 5.1 die Umlaute als Zeichensalat.
$oldEncoding = $null
try { $oldEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { }
try {
    Invoke-Main
} catch {
    if (-not $script:Failed) { Write-Host "  FEHLER: $($_.Exception.Message)" -ForegroundColor Red; Write-Host "  $($_.InvocationInfo.PositionMessage)" }
    $script:Failed = $true
} finally {
    if ($oldEncoding) { try { [Console]::OutputEncoding = $oldEncoding } catch { } }
}
if ($script:Failed) {
    if ($PSCommandPath) { exit 1 }
    return
}
