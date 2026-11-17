<#
.SYNOPSIS
Installiert oder aktualisiert das Agent Deck für den angemeldeten Benutzer.

.DESCRIPTION
Läuft unter Windows PowerShell 5.1 und PowerShell 7. Woher die Programme kommen:

  aus einem entpackten Paket   install.ps1 liegt neben agent-deck.exe (Vorgabe)
  -Package <zip>               aus einem heruntergeladenen Paket
  -Download                    neuestes Release von GitHub holen
  -FromSource                  aus dem Quellcode bauen (pnpm, cargo, node); Vorgabe, wenn
                               das Skript in tools\ eines Checkouts liegt
  -SkipBuild                   wie -FromSource, aber das schon gebaute target\release nehmen

Schritte: Voraussetzungen prüfen, laufendes Deck und Wächter beenden, Programme nach
%LOCALAPPDATA%\claude-agent-deck\bin kopieren, beim ersten Mal ein paar Fragen
(Repos-Ordner, Jira-Kürzel, Reviewer, fremde statusLine, Autostart), Hooks in Claudes
settings.json (deck-setup install; trägt fehlende Vorgaben ein: Modus Bypass, Effort high,
Vorhandenes bleibt), Windows-Terminal-Profil (deck-setup wt-profile),
Startmenü-Verknüpfung (deck-setup shortcut), Autostart, Deck starten und prüfen.

Ein zweiter Lauf ist ein Update: er fragt nichts mehr und lässt deck_settings.json stehen.
Entfernen: %LOCALAPPDATA%\claude-agent-deck\bin\uninstall.ps1

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1 -NonInteractive -ReposRoot C:\Code -JiraPrefix ABC -PrReviewer "leo, anna" -StatusLine keep -Autostart
#>
[CmdletBinding()]
param(
    [switch]$FromSource,
    [switch]$SkipBuild,
    [string]$Package,
    [switch]$Download,
    [switch]$NonInteractive,
    [string]$ReposRoot,
    [string]$JiraPrefix,
    [string]$PrReviewer,
    [ValidateSet('keep', 'replace')][string]$StatusLine,
    [switch]$Autostart,
    [switch]$NoShortcut
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Fortschrittsbalken bremsen Downloads in 5.1 stark

$GitHubRepo = 'jvosskuehler/agent-deck'
$AppDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck'
$Bin = Join-Path $AppDir 'bin'
$SettingsFile = Join-Path $AppDir 'deck_settings.json'
$Setup = Join-Path $Bin 'deck-setup.exe'
$Exes = @('agent-deck.exe', 'deck-report.exe', 'deck-statusline.exe', 'deck-console.exe', 'deck-watchdog.exe', 'deck-setup.exe')
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$RunValue = 'AgentDeck'
$Winget = 'winget install --source winget -e --id'

# Fehler enden in Fail. Aus einer Datei gestartet beendet das den Prozess mit Exit 1; aus
# dem Speicher gestartet (Einzeiler) nur das Skript, das Fenster bleibt offen.
$script:Failed = $false
function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Ok($text) { Write-Host "   ok: $text" -ForegroundColor Green }
function Info($text) { Write-Host "   $text" }
function Warn($text) { Write-Host "   Hinweis: $text" -ForegroundColor Yellow }
function Fail($text) {
    Write-Host "   FEHLER: $text" -ForegroundColor Red
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

function Invoke-Setup([string[]]$SetupArgs) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Setup @SetupArgs 2>&1 } finally { $ErrorActionPreference = $old }
    $out | ForEach-Object { Write-Host "   $_" }
    # Was deck-setup an Claudes Vorgaben neu eingetragen hat, steht am Ende noch einmal.
    $script:NewDefaults += @($out | Where-Object { "$_" -match 'Neu eingetragen: (.*)$' } | ForEach-Object { $Matches[1] })
    if ($LASTEXITCODE -ne 0) { Fail "deck-setup $($SetupArgs -join ' ') ist gescheitert" }
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
    $a = Read-Host "   $Prompt$suffix"
    if ([string]::IsNullOrWhiteSpace($a)) { return $Default }
    return $a.Trim()
}

function Read-YesNo([string]$Prompt, [bool]$Default) {
    $hint = 'j/N'
    if ($Default) { $hint = 'J/n' }
    while ($true) {
        $a = Read-Host "   $Prompt ($hint)"
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
            'norepos' {
                # Ausdrücklich angegeben: das ist die Bestätigung.
                Warn "$p enthält keinen Unterordner mit .git. Übernommen, weil ausdrücklich angegeben."
                return (Resolve-Path -LiteralPath $p).Path
            }
            default { Fail "Repos-Ordner $p gibt es nicht." }
        }
    }
    $suggest = Get-ReposSuggestion
    if (-not $Ask) {
        if ($suggest -and (Test-ReposRoot $suggest) -eq 'ok') {
            Info "Repos-Ordner: $suggest (gefunden)"
            return $suggest
        }
        Fail 'Kein Repos-Ordner mit git-Repos gefunden. Bitte -ReposRoot <Ordner> angeben.'
    }
    Write-Host ''
    Write-Host '   Der Repos-Ordner ist der Ordner, in dem alle deine git-Repos liegen (je Repo ein'
    Write-Host '   Unterordner). Dort starten die Agenten, dort sucht das Deck nach worktrees.'
    while ($true) {
        $p = Read-Answer 'Repos-Ordner' $suggest
        if (-not $p) { continue }
        $p = [Environment]::ExpandEnvironmentVariables($p.Trim('"'))
        switch (Test-ReposRoot $p) {
            'ok' {
                $p = (Resolve-Path -LiteralPath $p).Path
                Ok "$p ($(Get-RepoCount $p) Repos)"
                return $p
            }
            'missing' { Warn "$p gibt es nicht." }
            'norepos' {
                Warn "$p enthält keinen Unterordner mit .git."
                if (Read-YesNo 'Trotzdem diesen Ordner nehmen?' $false) { return (Resolve-Path -LiteralPath $p).Path }
            }
        }
    }
}

function Resolve-JiraPrefix([bool]$Ask) {
    if ($script:JiraGiven) { $v = $JiraPrefix }
    elseif ($Ask) {
        Write-Host ''
        Write-Host '   Jira-Projektkürzel (z. B. ORPH): Ticketnummern wie ORPH-123 werden dann erkannt.'
        $v = Read-Answer 'Jira-Kürzel, leer = keins' ''
    } else { $v = '' }
    $v = "$v".Trim().ToUpper()
    if ($v -and $v -notmatch '^[A-Z][A-Z0-9_]*$') { Fail "Jira-Kürzel '$v' ist ungültig (nur Buchstaben, Ziffern, _)." }
    return $v
}

# GitHub-Logins, die die PR-Automatik als Reviewer einträgt (mehrere mit Komma). Die
# Automatik selbst bleibt aus, bis sie in den Einstellungen eingeschaltet wird.
function Resolve-PrReviewer([bool]$Ask) {
    if ($script:ReviewerGiven) { $v = $PrReviewer }
    elseif ($Ask) {
        Write-Host ''
        Write-Host '   Die PR-Automatik (zunächst aus) kann deine PRs Reviewern zuweisen.'
        $v = Read-Answer 'Wer soll deine PRs reviewen? GitHub-Logins, mehrere mit Komma, leer = Vorgabe des Repos' ''
    } else { $v = '' }
    $logins = @()
    foreach ($l in ("$v" -split '[,;\s]+')) {
        $l = $l.Trim().TrimStart('@')
        if (-not $l) { continue }
        if ($l -notmatch '^[A-Za-z0-9][A-Za-z0-9-]*(\[bot\])?$') { Fail "GitHub-Login '$l' ist ungültig." }
        if (-not ($logins | Where-Object { $_ -ieq $l })) { $logins += $l }
    }
    return ($logins -join ', ')
}

# ── Claudes settings.json ─────────────────────────────────────────────────

function Get-ClaudeSettingsPath {
    $dir = $env:CLAUDE_CONFIG_DIR
    if ([string]::IsNullOrWhiteSpace($dir)) { $dir = Join-Path $env:USERPROFILE '.claude' }
    return (Join-Path $dir 'settings.json')
}

# Kommando einer statusLine, die nicht dem Deck gehört; sonst $null.
function Get-ForeignStatusLine {
    $path = Get-ClaudeSettingsPath
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    try {
        $raw = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8)
        $s = $raw | ConvertFrom-Json
    } catch { return $null }
    if (-not $s.statusLine) { return $null }
    $cmd = "$($s.statusLine.command)"
    if ($cmd -match '(^|[/\\"\s])deck-statusline(\.exe)?($|["\s])') { return $null }
    if ([string]::IsNullOrWhiteSpace($cmd)) { return '(ohne Kommando)' }
    return $cmd
}

function Resolve-StatusLine([bool]$Ask) {
    if ($StatusLine) { return $StatusLine }
    $foreign = Get-ForeignStatusLine
    if (-not $foreign -or -not $Ask) { return 'keep' }
    Write-Host ''
    Write-Host '   In Claudes settings.json steht schon eine statusLine eines anderen Werkzeugs:'
    Write-Host "     $foreign"
    Write-Host '   Das Deck liest Modell, Effort und Kontext-% aus seiner eigenen statusLine; mit der'
    Write-Host '   fremden bleiben diese Angaben auf den Kacheln leer. Ersetzen sichert die alte,'
    Write-Host '   uninstall.ps1 legt sie zurück.'
    if (Read-YesNo 'statusLine durch die des Decks ersetzen?' $false) { return 'replace' }
    return 'keep'
}

# ── Autostart (HKCU\...\Run, Wert „AgentDeck“) ────────────────────────────

function Set-Autostart([bool]$On) {
    if ($On) {
        New-Item -Path $RunKey -Force | Out-Null
        Set-ItemProperty -Path $RunKey -Name $RunValue -Value ('"' + (Join-Path $Bin 'agent-deck.exe') + '"')
        Ok 'Autostart beim Anmelden (HKCU\...\Run\AgentDeck)'
    } else {
        Remove-ItemProperty -Path $RunKey -Name $RunValue -ErrorAction SilentlyContinue
        Ok 'kein Autostart'
    }
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
    Step 'Release bauen'
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

# Ordner mit den sechs Programmen in einem entpackten Paket (flach oder eine Ebene tiefer).
function Find-PackageRoot([string]$Dir) {
    if (Test-Path -LiteralPath (Join-Path $Dir 'agent-deck.exe')) { return $Dir }
    $sub = Get-ChildItem -LiteralPath $Dir -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'agent-deck.exe') } | Select-Object -First 1
    if ($sub) { return $sub.FullName }
    return $null
}

function Expand-Package([string]$Zip) {
    if (-not (Test-Path -LiteralPath $Zip)) { Fail "Paket $Zip gibt es nicht." }
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
    Step "Neuestes Release von github.com/$GitHubRepo holen"
    try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12 } catch { }
    $api = "https://api.github.com/repos/$GitHubRepo/releases/latest"
    try {
        $rel = Invoke-RestMethod -Uri $api -UseBasicParsing -Headers @{ 'User-Agent' = 'agent-deck-installer'; 'Accept' = 'application/vnd.github+json' }
    } catch {
        $code = $null
        try { $code = [int]$_.Exception.Response.StatusCode } catch { }
        if ($code -eq 404) { Fail "Es gibt noch kein veröffentlichtes Release (https://github.com/$GitHubRepo/releases). Bitte ein Paket von Hand besorgen und mit -Package <zip> installieren." }
        Fail "GitHub nicht erreichbar ($($_.Exception.Message)). Proxy oder Netz prüfen, oder das Paket von Hand laden und -Package <zip> nehmen."
    }
    $asset = $rel.assets | Where-Object { $_.name -match '^agent-deck-.+-win-x64\.zip$' } | Select-Object -First 1
    if (-not $asset) { Fail "Release $($rel.tag_name) hat kein Paket agent-deck-*-win-x64.zip." }
    $zip = Join-Path ([IO.Path]::GetTempPath()) $asset.name
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing -Headers @{ 'User-Agent' = 'agent-deck-installer' }
    Ok "$($asset.name) ($($rel.tag_name))"
    return $zip
}

# Gibt @{ Dir = Ordner mit den Programmen; Version; Scripts = Ordner mit uninstall.ps1 }.
function Resolve-Source {
    $here = $PSScriptRoot
    $repo = $null
    if ($here) {
        $parent = Split-Path $here -Parent
        if ((Test-Path -LiteralPath (Join-Path $parent 'Cargo.toml')) -and (Test-Path -LiteralPath (Join-Path $parent 'app\package.json'))) { $repo = $parent }
    }
    if ($FromSource -or $SkipBuild) {
        if (-not $repo) { Fail '-FromSource/-SkipBuild gehen nur aus tools\ eines Checkouts.' }
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
            return @{ Dir = (Join-Path $repo 'target\release'); Version = (Get-SourceVersion $repo); Scripts = (Join-Path $repo 'tools') }
        }
        'folder' { $dir = $here }
        'package' {
            if (-not (Test-Path -LiteralPath $Package -PathType Leaf)) { Fail "Paket $Package gibt es nicht." }
            $dir = Expand-Package (Resolve-Path -LiteralPath $Package).Path
        }
        'download' { $dir = Expand-Package (Get-ReleaseZip) }
    }
    $version = 'unbekannt'
    $vf = Join-Path $dir 'version.txt'
    if (Test-Path -LiteralPath $vf) { $version = (Get-Content -LiteralPath $vf -TotalCount 1).Trim() }
    return @{ Dir = $dir; Version = $version; Scripts = $dir }
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
    Step 'Voraussetzungen prüfen'
    $bad = 0
    $wt = Find-Command 'wt.exe'
    if (-not $wt) {
        # Ohne Alias im PATH (Server, frisch per winget installiert) steht es noch als Paket da.
        try { $pkg = Get-AppxPackage -Name 'Microsoft.WindowsTerminal*' -ErrorAction SilentlyContinue | Select-Object -First 1 } catch { $pkg = $null }
        if ($pkg) { $wt = $pkg.InstallLocation }
    }
    if ($wt) { Ok "Windows Terminal ($wt)" }
    else {
        Write-Host '   FEHLT: Windows Terminal. Die Agenten laufen darin.' -ForegroundColor Red
        Write-Host "          Installieren: $Winget Microsoft.WindowsTerminal (oder Microsoft Store)"
        $bad++
    }
    $wv = Test-WebView2
    if ($wv) { Ok "WebView2-Laufzeit $wv" }
    else {
        Write-Host '   FEHLT: Microsoft Edge WebView2-Laufzeit. Die Oberfläche des Decks braucht sie.' -ForegroundColor Red
        Write-Host "          Installieren: $Winget Microsoft.EdgeWebView2Runtime"
        $bad++
    }
    $claude = Find-Claude
    if ($claude) { Ok "Claude Code ($claude)" }
    else {
        Write-Host '   FEHLT: Claude Code (claude). Das Deck startet und beobachtet Claude-Code-Agenten.' -ForegroundColor Red
        Write-Host '          Installieren: irm https://claude.ai/install.ps1 | iex   (danach neues Fenster)'
        $bad++
    }
    if ($bad -gt 0) { Fail "$bad Voraussetzung(en) fehlen, siehe oben. Danach install.ps1 noch einmal starten." }

    # Kann, muss nicht.
    $pwsh = Find-Command 'pwsh.exe'
    if ($pwsh) { Ok "PowerShell 7 ($pwsh)" } else { Info "PowerShell 7 fehlt: die Agenten laufen in Windows PowerShell 5.1. Optional: $Winget Microsoft.PowerShell" }
    $gh = Find-Command 'gh'
    if ($gh) {
        Invoke-Native $gh @('auth', 'status') 2>&1 | Out-Null
        if ($LASTEXITCODE -eq 0) { Ok 'GitHub CLI (gh), angemeldet' }
        else { Warn 'GitHub CLI (gh) ist nicht angemeldet: PRs auf den Kacheln fehlen. Anmelden: gh auth login' }
    } else { Info "GitHub CLI (gh) fehlt: ohne sie keine PRs auf den Kacheln. Optional: $Winget GitHub.cli" }
    if (Find-Command 'acli') { Ok 'Atlassian CLI (acli)' } else { Info 'Atlassian CLI (acli) fehlt: optional, für Jira-Tickets.' }
    if (Find-Command 'ssh.exe') { Ok 'OpenSSH-Client' } else { Info 'OpenSSH-Client fehlt: optional (Windows-Feature „OpenSSH-Client“).' }
}

# ── Ablauf ────────────────────────────────────────────────────────────────

function Stop-Deck {
    Step 'Laufendes Deck beenden'
    # Der Wächter zuerst, sonst startet er die App gleich wieder.
    $any = $false
    foreach ($name in 'deck-watchdog', 'agent-deck') {
        foreach ($p in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
            Ok "$name beendet (PID $($p.Id))"
            $any = $true
        }
    }
    if ($any) { Start-Sleep -Milliseconds 700 }   # die Exe ist erst nach dem Prozessende überschreibbar
    else { Ok 'lief nicht' }
}

function Copy-Programs($Source) {
    Step "Programme nach $Bin kopieren"
    foreach ($exe in $Exes) {
        if (-not (Test-Path -LiteralPath (Join-Path $Source.Dir $exe))) { Fail "$exe fehlt in $($Source.Dir)" }
    }
    New-Item -ItemType Directory -Force -Path $Bin | Out-Null
    foreach ($exe in $Exes) {
        # Ein eben beendeter Prozess gibt seine Exe manchmal erst Augenblicke später frei.
        for ($i = 1; ; $i++) {
            try { Copy-Item -LiteralPath (Join-Path $Source.Dir $exe) -Destination (Join-Path $Bin $exe) -Force; break }
            catch {
                if ($i -ge 10) { Fail "$exe nicht kopiert: $($_.Exception.Message)" }
                Start-Sleep -Milliseconds 500
            }
        }
    }
    $un = Join-Path $Source.Scripts 'uninstall.ps1'
    if (Test-Path -LiteralPath $un) { Copy-Item -LiteralPath $un -Destination (Join-Path $Bin 'uninstall.ps1') -Force }
    Write-Utf8 (Join-Path $Bin 'version.txt') "$($Source.Version)`r`n"
    # Aus dem Internet geladene Dateien tragen eine Herkunftsmarke; ohne sie fragt
    # SmartScreen nicht bei jedem Start.
    Get-ChildItem -LiteralPath $Bin -File | Unblock-File -ErrorAction SilentlyContinue
    Ok "$($Exes -join ', ') (Version $($Source.Version))"
}

function Write-DeckSettings([string]$Repos, [string]$Jira, [string]$Reviewers) {
    $s = [ordered]@{ settings_version = 1; repos_root = $Repos }
    if ($Jira) { $s.jira_prefix = $Jira }
    if ($Reviewers) { $s.pr_reviewer = $Reviewers }
    # Alles mit Wirkung nach außen ist aus, bis es jemand in den Einstellungen anschaltet.
    # Auto-Compact ist an: es schreibt nur in den eigenen wartenden Agenten.
    foreach ($k in 'titan', 'store_update', 'speechmike_voice', 'auto_permission_mode', 'pr_automation') { $s[$k] = $false }
    $s.auto_compact = $true
    New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
    Write-Utf8 $SettingsFile ($s | ConvertTo-Json)
    Ok "Einstellungen: $SettingsFile"
}

function Start-Deck {
    Step 'Deck starten'
    Start-Process -FilePath (Join-Path $Bin 'agent-deck.exe') -WorkingDirectory $Bin
    $deadline = (Get-Date).AddSeconds(15)
    $app = $null
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $app = Get-Process -Name 'agent-deck' -ErrorAction SilentlyContinue
        if ($app) { break }
    }
    if (-not $app) { Fail "agent-deck.exe läuft nicht. Protokoll: $AppDir\agent-deck.log" }
    Ok "agent-deck läuft (PID $(@($app.Id) -join ', '))"
}

function Invoke-Main {
    Write-Host 'Agent Deck installieren' -ForegroundColor Cyan
    Test-Prerequisites
    $source = Resolve-Source
    $firstRun = -not (Test-Path -LiteralPath $SettingsFile)
    $ask = Test-Interactive

    $repos = $null; $jira = ''; $reviewers = ''
    $autostartOn = $null
    if ($firstRun) {
        Step 'Ersteinrichtung'
        $repos = Resolve-ReposRoot $ask
        $jira = Resolve-JiraPrefix $ask
        $reviewers = Resolve-PrReviewer $ask
        if ($script:AutostartGiven) { $autostartOn = [bool]$Autostart }
        elseif ($ask) { Write-Host ''; $autostartOn = Read-YesNo 'Deck beim Anmelden an Windows automatisch starten?' $true }
        else { $autostartOn = $false }
    } elseif ($script:AutostartGiven) {
        $autostartOn = [bool]$Autostart
    }
    $mode = Resolve-StatusLine ($firstRun -and $ask)

    Stop-Deck
    Copy-Programs $source

    # Die frühere VS-Code-Extension (bis 09.11.2026) nur anfassen, wenn sie da ist.
    if (Test-Path (Join-Path $env:USERPROFILE '.vscode\extensions\agent-deck-bridge*')) {
        Step 'Frühere VS-Code-Extension des Decks entfernen'
        Invoke-Setup @('remove-vscode-extension')
    }

    Step "Hooks in $(Get-ClaudeSettingsPath) einrichten und prüfen"
    Invoke-Setup @('install', '--bin', $Bin, '--statusline', $mode)
    Invoke-Setup @('check', '--bin', $Bin)
    # Erst nach den Hooks: scheitert ein erster Lauf vorher, fragt der nächste wieder.
    if ($firstRun -and -not (Test-Path -LiteralPath $SettingsFile)) { Write-DeckSettings $repos $jira $reviewers }

    Step 'Windows-Terminal-Profil „Agent Deck“'
    Invoke-Setup @('wt-profile')

    $link = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk'
    if ($NoShortcut) { Info 'Startmenü-Verknüpfung übersprungen (-NoShortcut)' }
    elseif ($firstRun -or (Test-Path -LiteralPath $link)) {
        Step 'Startmenü-Verknüpfung'
        Invoke-Setup @('shortcut', '--bin', $Bin)
    }

    if ($null -ne $autostartOn) { Step 'Autostart'; Set-Autostart $autostartOn }

    Start-Deck
    Write-Host "`nAgent Deck $($source.Version) installiert." -ForegroundColor Green
    if ($script:NewDefaults.Count -gt 0) {
        Write-Host '   In Claudes settings.json neu eingetragen (deine eigenen Vorgaben bleiben immer):' -ForegroundColor Yellow
        foreach ($d in $script:NewDefaults) { Write-Host "     $d" -ForegroundColor Yellow }
    }
    if ($firstRun) {
        Write-Host '   PR-Automatik, Titan, Store-Update und Auto-Modus sind aus;'
        Write-Host '   anschalten im Deck unter Einstellungen.'
    }
    Write-Host "   Entfernen: powershell -ExecutionPolicy Bypass -File `"$Bin\uninstall.ps1`""
}

# Ob ein Parameter angegeben wurde, sehen die Funktionen in ihrem eigenen
# $PSBoundParameters nicht: einmal hier oben festhalten.
$script:NewDefaults = @()
$script:JiraGiven = $PSBoundParameters.ContainsKey('JiraPrefix')
$script:ReviewerGiven = $PSBoundParameters.ContainsKey('PrReviewer')
$script:AutostartGiven = $PSBoundParameters.ContainsKey('Autostart')

# deck-setup schreibt UTF-8; ohne das zeigt 5.1 die Umlaute als Zeichensalat.
$oldEncoding = $null
try { $oldEncoding = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { }
try {
    Invoke-Main
} catch {
    if (-not $script:Failed) { Write-Host "   FEHLER: $($_.Exception.Message)" -ForegroundColor Red; Write-Host "   $($_.InvocationInfo.PositionMessage)" }
    $script:Failed = $true
} finally {
    if ($oldEncoding) { try { [Console]::OutputEncoding = $oldEncoding } catch { } }
}
if ($script:Failed) {
    if ($PSCommandPath) { exit 1 }
    return
}
