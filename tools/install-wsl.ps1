# Agenten in WSL: die WSL-Teile von install.ps1, in eigenen Funktionen.
#
# Wird von install.ps1 per Dot-Source geladen (liegt daneben, im Paket wie im Checkout)
# und benutzt dessen Helfer (Say, Ok, Hint, Fail, Read-Answer, Read-YesNo, Get-RepoCount,
# Invoke-Setup, Write-Utf8). Läuft unter Windows PowerShell 5.1: UTF-8 mit BOM, keine
# 7er-Syntax.
#
# Ablauf beim ersten Mal (Team-Ausgabe: nur WSL):
#   1. Distro wählen (Vorschlag: Standard-Distro), Home und Einhängepunkt erfragen
#      (deck-setup wsl-probe)
#   2. claude in der Distro? Sonst Installation anbieten (offizielles Skript)
#   3. Repos-Ordner in WSL (Linux-Pfad)
# Danach trägt install.ps1 Hooks und Infozeile mit `--wsl <Distro>` in der Distro ein.

$ClaudeInstallLinux = 'curl -fsSL https://claude.ai/install.sh | bash'

# deck-setup aufrufen und die Ausgabe als JSON lesen ($null bei Fehler).
function Invoke-SetupJson([string]$Exe, [string[]]$SetupArgs) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Exe @SetupArgs 2>$null } finally { $ErrorActionPreference = $old }
    if ($LASTEXITCODE -ne 0 -or -not $out) { return $null }
    try { return (($out -join "`n") | ConvertFrom-Json) } catch { return $null }
}

function Get-WslProbe([string]$Exe, [string]$Distro) {
    return Invoke-SetupJson $Exe @('wsl-probe', '--wsl', $Distro)
}

# Windows-Pfad eines Linux-Pfads in der Distro (wie deck_core::wsl::Wsl::to_windows).
function ConvertTo-WslUnc([string]$Distro, [string]$Mount, [string]$LinuxPath) {
    if (-not $LinuxPath.StartsWith('/')) { return $LinuxPath }
    if ($Mount -and $LinuxPath.StartsWith($Mount)) {
        $rest = $LinuxPath.Substring($Mount.Length)
        if ($rest -match '^([a-zA-Z])(/(.*))?$') { return ($Matches[1].ToUpper() + ':\' + ($Matches[3] -replace '/', '\')) }
    }
    return ('\\wsl.localhost\' + $Distro + ($LinuxPath -replace '/', '\'))
}

function Select-WslDistro([bool]$Ask, [string]$Exe) {
    $list = @(Invoke-SetupJson $Exe @('wsl-distros'))
    $names = @($list | Where-Object { $_ -and $_.name } | ForEach-Object { $_.name })
    if ($names.Count -eq 0) {
        Write-Host '  ✗ Keine WSL-Distro gefunden.' -ForegroundColor Red
        Say 'WSL mit Ubuntu einrichten (als Administrator, danach neu anmelden):  wsl --install -d Ubuntu'
        Fail 'Danach den Installer noch einmal starten.'
    }
    if ($WslDistro) {
        if ($names -notcontains $WslDistro) { Fail "Die WSL-Distro $WslDistro gibt es nicht. Vorhanden: $($names -join ', ')" }
        return $WslDistro
    }
    $default = ($list | Where-Object { $_.default } | Select-Object -First 1).name
    if (-not $default) { $default = $names[0] }
    if (-not $Ask -or $names.Count -eq 1) { return $default }
    while ($true) {
        $d = Read-Answer "In welcher WSL-Distro sollen die Agenten laufen ($($names -join ', '))?" $default
        if ($names -contains $d) { return $d }
        Hint "Die Distro $d gibt es nicht."
    }
}

# claude in der Distro; fehlt es, die Installation anbieten.
function Confirm-WslClaude([bool]$Ask, [string]$Exe, $Probe) {
    if ($Probe.claude) { Ok "Claude Code in WSL ($($Probe.distro))"; return $Probe }
    Write-Host "  ✗ Claude Code fehlt in WSL ($($Probe.distro))." -ForegroundColor Red
    $install = $false
    if ($Ask) { $install = Read-YesNo 'Jetzt in WSL installieren?' $true }
    if (-not $install) {
        Say "Installieren in WSL:  $ClaudeInstallLinux"
        Fail 'Danach den Installer noch einmal starten.'
    }
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & wsl.exe -d $Probe.distro -e bash -lc $ClaudeInstallLinux } finally { $ErrorActionPreference = $old }
    $again = Get-WslProbe $Exe $Probe.distro
    if (-not $again -or -not $again.claude) { Fail "Claude Code ließ sich in WSL nicht installieren. Von Hand: $ClaudeInstallLinux" }
    Ok "Claude Code in WSL installiert ($($again.distro))"
    return $again
}

function Resolve-WslReposRoot([bool]$Ask, $Probe) {
    $check = {
        param([string]$p)
        $unc = ConvertTo-WslUnc $Probe.distro $Probe.mount $p
        if (-not (Test-Path -LiteralPath $unc -PathType Container)) { return 'missing' }
        if ((Get-RepoCount $unc) -lt 1) { return 'norepos' }
        return 'ok'
    }
    if ($ReposRoot) {
        $p = $ReposRoot.Replace('~', $Probe.home).TrimEnd('/')
        if ((& $check $p) -eq 'missing') { Fail "Den Ordner $p gibt es in WSL nicht." }
        return $p
    }
    $suggest = $null
    foreach ($c in @("$($Probe.home)/repos", "$($Probe.home)/Repos", "$($Probe.home)/src", "$($Probe.home)/code")) {
        if ((& $check $c) -eq 'ok') { $suggest = $c; break }
    }
    if (-not $Ask) {
        if ($suggest) { return $suggest }
        Fail 'Kein Ordner mit Git-Repos in WSL gefunden. Bitte mit -ReposRoot <Linux-Ordner> angeben.'
    }
    if (-not $suggest) { $suggest = "$($Probe.home)/repos" }
    while ($true) {
        $p = Read-Answer 'In welchem WSL-Ordner liegen deine Git-Repos?' $suggest
        if (-not $p) { continue }
        $p = $p.Trim('"').Replace('~', $Probe.home).TrimEnd('/')
        if (-not $p.StartsWith('/')) { Hint 'Bitte einen Linux-Pfad angeben, etwa /home/name/repos.'; continue }
        switch (& $check $p) {
            'ok' { return $p }
            'missing' { Hint 'Den Ordner gibt es in WSL nicht.' }
            'norepos' { if (Read-YesNo 'In diesem Ordner liegt kein Git-Repo. Trotzdem nehmen?' $false) { return $p } }
        }
    }
}

# Erster Lauf: Distro, claude, Repos-Ordner. Rückgabe: @{ Distro; Home; Mount; Repos }.
function Initialize-WslAgents([bool]$Ask, [string]$Exe) {
    $distro = Select-WslDistro $Ask $Exe
    $probe = Get-WslProbe $Exe $distro
    if (-not $probe) { Fail "Die WSL-Distro $distro startet nicht. Einmal von Hand öffnen (wsl -d $distro) und den Installer noch einmal starten." }
    $probe = Confirm-WslClaude $Ask $Exe $probe
    $repos = Resolve-WslReposRoot $Ask $probe
    return @{ Distro = $probe.distro; Home = $probe.home; Mount = $probe.mount; Repos = $repos }
}

# Update: die Distro aus den Einstellungen; claude dort prüfen (nur ein Hinweis).
function Get-WslFromSettings([string]$SettingsFile) {
    try { $s = Get-Content -LiteralPath $SettingsFile -Raw -Encoding UTF8 | ConvertFrom-Json } catch { return $null }
    if ($s.agent_env -ne 'wsl' -or -not $s.wsl_distro) { return $null }
    return @{ Distro = $s.wsl_distro }
}

# Die WSL-Angaben in deck_settings.json eintragen (nach Write-DeckSettings).
function Set-WslDeckSettings([string]$SettingsFile, $Wsl) {
    $s = Get-Content -LiteralPath $SettingsFile -Raw -Encoding UTF8 | ConvertFrom-Json
    $add = [ordered]@{ agent_env = 'wsl'; wsl_distro = $Wsl.Distro; wsl_home = $Wsl.Home; wsl_mount_root = $Wsl.Mount }
    foreach ($k in $add.Keys) { $s | Add-Member -NotePropertyName $k -NotePropertyValue $add[$k] -Force }
    Write-Utf8 $SettingsFile ($s | ConvertTo-Json -Depth 6)
}

# Die Linux-Fassungen von deck-report und deck-statusline nach bin\linux; deck-setup
# install --wsl kopiert sie von dort in die Distro. Fehlen sie (Quellbau ohne Linux-Ziel),
# laufen die Hooks über die .exe, nur langsamer.
function Copy-LinuxHooks($Source) {
    $from = $null
    foreach ($c in @((Join-Path $Source.Dir 'linux'), (Join-Path (Split-Path $Source.Dir -Parent) 'x86_64-unknown-linux-musl\release'))) {
        if ((Test-Path -LiteralPath (Join-Path $c 'deck-report')) -and (Test-Path -LiteralPath (Join-Path $c 'deck-statusline'))) { $from = $c; break }
    }
    if (-not $from) { Write-Verbose 'Keine Linux-Fassungen der Hooks gefunden, sie laufen über die .exe.'; return }
    $to = Join-Path $Bin 'linux'
    New-Item -ItemType Directory -Force -Path $to | Out-Null
    foreach ($f in 'deck-report', 'deck-statusline') { Copy-Item -LiteralPath (Join-Path $from $f) -Destination $to -Force }
}

# Hooks, Infozeile und Theme in der Distro (statt in Claudes Windows-Einstellungen).
function Install-WslHooks([string]$Distro) {
    Invoke-Setup @('install', '--bin', $Bin, '--wsl', $Distro, '--statusline', $StatusLine)
    Invoke-Setup @('check', '--bin', $Bin, '--wsl', $Distro)
}
