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

Beim ersten Mal fragt der Installer nach dem Repos-Ordner, dem Jira-Kürzel, den
Reviewern und dem Autostart. Ein zweiter Lauf ist ein Update und fragt nichts.
Was er am Rechner ändert, steht in README.md; Einzelheiten in docs\BETRIEB.md.
-Verbose zeigt jeden Schritt.

Entfernen: Windows-Einstellungen > Apps > Agent Deck.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1 -NonInteractive -ReposRoot C:\Code -JiraPrefix ABC -PrReviewer "Leo, Anna Schmidt" -Autostart
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
    # Nur für Fachleute: keep lässt eine fremde Infozeile allein stehen (dann fehlen
    # Modell, Effort und Kontext auf den Kacheln). Vorgabe: die des Decks, die alte dahinter.
    [ValidateSet('keep', 'replace')][string]$StatusLine = 'replace',
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

function Resolve-JiraPrefix([bool]$Ask) {
    if ($script:JiraGiven) {
        $v = "$JiraPrefix".Trim().ToUpper()
        if ($v -and $v -notmatch '^[A-Z][A-Z0-9_]*$') { Fail "Jira-Kürzel '$v' geht nicht: nur Buchstaben, Ziffern und _." }
        return $v
    }
    if (-not $Ask) { return '' }
    while ($true) {
        $v = "$(Read-Answer 'Jira-Projektkürzel für Ticketnummern (z. B. ORPH, leer = keins)' '')".Trim().ToUpper()
        if (-not $v -or $v -match '^[A-Z][A-Z0-9_]*$') { return $v }
        Hint 'Nur Buchstaben, Ziffern und _, zum Beispiel ORPH.'
    }
}

# ── Reviewer der PR-Automatik ─────────────────────────────────────────────

# Getippt werden Namen, Mailadressen oder Logins; `deck-setup resolve-reviewers` gleicht
# sie mit den Mitgliedern der GitHub-Organisation ab (über gh, nur lesend) und druckt
# JSON. $null, wenn das nicht ging (Programm fehlt, Ausgabe unlesbar).
function Invoke-ReviewerLookup([string]$Exe, [string]$Text, [string]$Org) {
    if (-not (Test-Path -LiteralPath $Exe)) { return $null }
    # 5.1 reicht Anführungszeichen in Argumenten nicht sauber weiter.
    $a = @('resolve-reviewers', '--names', ($Text -replace '"', ''))
    if ($Org) { $a += @('--org', $Org) }
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Exe @a 2>$null } catch { $out = $null } finally { $ErrorActionPreference = $old }
    if ($LASTEXITCODE -ne 0 -or -not $out) { return $null }
    try { return ((@($out) -join "`n") | ConvertFrom-Json) } catch { return $null }
}

function Test-GitHubLogin([string]$Text) {
    return ($Text -match '^[A-Za-z0-9][A-Za-z0-9-]{0,38}(\[bot\])?$')
}

# Nummerierte Wahl; -1 = übersprungen.
function Read-Choice([string]$Prompt, [string[]]$Items) {
    for ($i = 0; $i -lt $Items.Count; $i++) { Say "$($i + 1)  $($Items[$i])" }
    while ($true) {
        $a = Read-Host "$Prompt (1-$($Items.Count), leer = überspringen)"
        if ([string]::IsNullOrWhiteSpace($a)) { return -1 }
        $n = 0
        if ([int]::TryParse($a.Trim(), [ref]$n) -and $n -ge 1 -and $n -le $Items.Count) { return ($n - 1) }
    }
}

function Format-Person($Login, $Name) {
    if ($Name) { return "$Name ($Login)" }
    return "$Login"
}

# Wer die eigenen PRs reviewt. Ein Eintrag, der nicht passt oder kein Login ist, bricht
# nie ab: interaktiv wird nachgefragt, sonst gewarnt und übersprungen. Mehrere Treffer:
# interaktiv eine Wahl, sonst übersprungen. Ohne gh (oder ohne Organisation) bleibt ein
# Eintrag, der als Login taugt, ungeprüft stehen. Die Automatik selbst bleibt aus, bis
# sie in den Einstellungen eingeschaltet wird.
# Gibt @{ Logins = 'a, b'; Org = '...'; Names = @{ login = Name } } zurück.
function Resolve-PrReviewer([bool]$Ask, [string]$Exe) {
    $logins = @()
    $names = [ordered]@{}
    $orgOut = ''
    if ($script:ReviewerGiven) { $text = $PrReviewer }
    elseif ($Ask) { $text = Read-Answer 'Wer soll deine PRs reviewen? Namen, mehrere mit Komma (leer = keiner)' '' }
    else { $text = '' }
    $org = ''
    $warned = $false
    while ("$text".Trim()) {
        $r = Invoke-ReviewerLookup $Exe $text $org
        if ($r -and $r.note -eq 'chooseOrg' -and $Ask) {
            $orgs = @($r.orgs)
            Say 'Dein GitHub-Konto gehört zu mehreren Organisationen.'
            $i = Read-Choice 'In welcher sollen die Namen gesucht werden?' $orgs
            if ($i -ge 0) { $org = $orgs[$i]; continue }
        }
        $looked = [bool]($r -and $r.note -eq 'none')
        if (-not $looked -and -not $warned) {
            $why = 'GitHub CLI fehlt oder ist nicht angemeldet'
            if ($r -and $r.note -eq 'chooseOrg') { $why = 'keine Organisation gewählt' }
            elseif ($r -and $r.note -eq 'noOrg') { $why = 'keine GitHub-Organisation gefunden' }
            elseif ($r -and $r.note -eq 'membersFailed') { $why = "Mitglieder von $($r.org) nicht lesbar" }
            Hint "Namen nicht geprüft ($why), übernommen wie getippt."
            $warned = $true
        }
        if ($r) { $results = @($r.results) }
        else {
            $results = @()
            foreach ($t in ("$text" -split '[,;]+')) {
                $t = $t.Trim().TrimStart('@')
                if (-not $t) { continue }
                $l = ''
                if (Test-GitHubLogin $t) { $l = $t }
                $results += [pscustomobject]@{ input = $t; status = 'none'; login = $l; name = ''; candidates = @() }
            }
        }
        if ($r -and $r.org -and -not $orgOut) { $orgOut = "$($r.org)" }
        $where = $orgOut
        if (-not $where) { $where = 'deiner Organisation' }
        $retry = @()
        foreach ($x in $results) {
            $pick = ''
            $pickName = ''
            switch ("$($x.status)") {
                'unique' {
                    $pick = "$($x.login)"; $pickName = "$($x.name)"
                    Ok "Reviewer: $(Format-Person $pick $pickName)"
                }
                'ambiguous' {
                    $cands = @($x.candidates)
                    $labels = @($cands | ForEach-Object { Format-Person $_.login $_.name })
                    if ($Ask) {
                        Say "'$($x.input)' passt auf mehrere:"
                        $i = Read-Choice 'Wen meinst du?' $labels
                        if ($i -ge 0) {
                            $pick = "$($cands[$i].login)"; $pickName = "$($cands[$i].name)"
                            Ok "Reviewer: $(Format-Person $pick $pickName)"
                        }
                    } else {
                        Hint "'$($x.input)' passt auf mehrere ($($labels -join ', ')), übersprungen."
                    }
                }
                default {
                    if ($x.login) {
                        $pick = "$($x.login)"
                        if ($looked) { Hint "'$($x.input)' nicht in $where gefunden, als GitHub-Login übernommen." }
                    } else {
                        Hint "'$($x.input)' nicht gefunden, übersprungen."
                        $retry += "$($x.input)"
                    }
                }
            }
            if ($pick -and -not ($logins | Where-Object { $_ -ieq $pick })) {
                $logins += $pick
                if ($pickName) { $names[$pick] = $pickName }
            }
        }
        $text = ''
        if ($Ask -and $retry.Count -gt 0) {
            $text = Read-Answer "Wen meintest du mit $($retry -join ', ')? Name oder GitHub-Login (leer = überspringen)" ''
        }
    }
    return @{ Logins = ($logins -join ', '); Org = $orgOut; Names = $names }
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

# Ordner mit den sechs Programmen in einem entpackten Paket (flach oder eine Ebene tiefer).
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

# Angemeldet bei GitHub? Über cmd mit umgeleiteter Ausgabe: so sieht PowerShell 5.1 kein
# stderr von gh und schreibt keinen roten NativeCommandError (auch nicht ins Transcript).
function Test-GhLogin {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & cmd.exe /d /c 'gh auth status >nul 2>&1' } finally { $ErrorActionPreference = $old }
    return ($LASTEXITCODE -eq 0)
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
    if (Find-Claude) { Ok 'Claude Code' }
    else { Write-Host '  ✗ Claude Code fehlt. Installieren: irm https://claude.ai/install.ps1 | iex (danach ein neues Fenster öffnen)' -ForegroundColor Red; $bad++ }
    if ($bad -gt 0) { Fail 'Bitte das Fehlende installieren und den Installer noch einmal starten.' }

    # Kann, muss nicht.
    if (-not (Find-Command 'gh')) { Hint "GitHub CLI fehlt, dann zeigt das Deck keine PRs. Installieren: $Winget GitHub.cli" }
    elseif (Test-GhLogin) { Ok 'GitHub CLI' }
    else { Hint 'GitHub CLI ist nicht angemeldet, dann zeigt das Deck keine PRs. Anmelden: gh auth login' }
}

# ── Ablauf ────────────────────────────────────────────────────────────────

function Stop-Deck {
    # Der Wächter zuerst, sonst startet er die App gleich wieder.
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
    foreach ($exe in $Exes) {
        # Ein eben beendeter Prozess gibt seine Exe manchmal erst Augenblicke später frei.
        for ($i = 1; ; $i++) {
            try { Copy-Item -LiteralPath (Join-Path $Source.Dir $exe) -Destination (Join-Path $Bin $exe) -Force; break }
            catch {
                if ($i -ge 10) { Fail "$exe ließ sich nicht kopieren: $($_.Exception.Message)" }
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
    Ok "Programme (Version $($Source.Version))"
}

function Write-DeckSettings([string]$Repos, [string]$Jira, $Reviewers) {
    $s = [ordered]@{ settings_version = 1; repos_root = $Repos }
    if ($Jira) { $s.jira_prefix = $Jira }
    if ($Reviewers -and $Reviewers.Logins) { $s.pr_reviewer = $Reviewers.Logins }
    if ($Reviewers -and $Reviewers.Names.Count -gt 0) { $s.pr_reviewer_names = $Reviewers.Names }
    if ($Reviewers -and $Reviewers.Org) { $s.github_org = $Reviewers.Org }
    # Alles mit Wirkung nach außen ist aus, bis es jemand in den Einstellungen anschaltet.
    # Auto-Compact ist an: es schreibt nur in den eigenen wartenden Agenten.
    foreach ($k in 'titan', 'store_update', 'speechmike_voice', 'auto_permission_mode', 'pr_automation') { $s[$k] = $false }
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

function Invoke-Main {
    Write-Host 'Agent Deck wird installiert.' -ForegroundColor Cyan
    Write-Host ''
    Test-Prerequisites
    $source = Resolve-Source
    $firstRun = -not (Test-Path -LiteralPath $SettingsFile)
    $ask = Test-Interactive

    $repos = $null; $jira = ''; $reviewers = $null
    $autostartOn = $null
    if ($firstRun) {
        if ($ask) { Write-Host '' }
        $repos = Resolve-ReposRoot $ask
        $jira = Resolve-JiraPrefix $ask
        # deck-setup aus der Quelle: nach bin kopiert wird erst nach den Fragen.
        $reviewers = Resolve-PrReviewer $ask (Join-Path $source.Dir 'deck-setup.exe')
        if ($script:AutostartGiven) { $autostartOn = [bool]$Autostart }
        elseif ($ask) { $autostartOn = Read-YesNo 'Deck beim Windows-Start mitstarten?' $true }
        else { $autostartOn = $false }
        if ($ask) { Write-Host '' }
    } elseif ($script:AutostartGiven) {
        $autostartOn = [bool]$Autostart
    }

    Stop-Deck
    Copy-Programs $source

    # Die frühere VS-Code-Extension (bis 09.11.2026) nur anfassen, wenn sie da ist.
    if (Test-Path (Join-Path $env:USERPROFILE '.vscode\extensions\agent-deck-bridge*')) {
        Invoke-Setup @('remove-vscode-extension')
    }

    Invoke-Setup @('install', '--bin', $Bin, '--statusline', $StatusLine)
    Invoke-Setup @('check', '--bin', $Bin)
    # Erst nach den Hooks: scheitert ein erster Lauf vorher, fragt der nächste wieder.
    if ($firstRun -and -not (Test-Path -LiteralPath $SettingsFile)) { Write-DeckSettings $repos $jira $reviewers }
    Ok 'Claude-Einstellungen'

    # Erst Schrift und Shader, dann das Terminal-Profil, das beide nennt.
    Invoke-Setup @('look', '--from', $source.Papier)
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
$script:JiraGiven = $PSBoundParameters.ContainsKey('JiraPrefix')
$script:ReviewerGiven = $PSBoundParameters.ContainsKey('PrReviewer')
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
