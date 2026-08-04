<#
.SYNOPSIS
Richtet Agent Deck auf diesem Rechner ein - in einem Lauf und wiederholbar.

.DESCRIPTION
Alles, was in einer Anleitung als "hier deinen Pfad einsetzen" stand, macht dieses
Skript: Voraussetzungen pruefen, Pillow holen, die VS-Code-Extension kopieren, die
sechs Hooks und die statusLine in ~/.claude/settings.json mergen - und danach
BEWEISEN, dass ein Hook wirklich schreibt. Der Beweis ist der Punkt: die Falle vom
2026-07-29 (ein 'cmd /c' vor dem Hook) endete mit Exit 0 und sah darum gesund aus.
Erkennbar war sie nur daran, dass in state\ keine Datei mehr frisch wurde - genau
das prueft Schritt 5.

Der Lauf ist idempotent: fremde Hooks anderer Werkzeuge bleiben stehen, eigene
werden ersetzt statt verdoppelt (siehe deck/claude/hook_setup.py). Ein zweiter
Aufruf ist also ein Nulldurchgang und kein Risiko.

Bewusst ASCII-only - wie install_watchdog.ps1. Eine .ps1 mit Umlauten liest die
Windows-PowerShell 5.1 ohne BOM als ANSI, und dann steht Muell auf dem Schirm.

.PARAMETER Check
Nichts aendern, nur pruefen und berichten (der Doctor). Exit 1 bei einem Befund.

.PARAMETER Remove
Unsere Hook-Eintraege und die installierte Extension wieder entfernen.

.PARAMETER Force
Auch eine FREMDE statusLine ersetzen. Ohne das bleibt sie stehen (und Modell,
Effort sowie Kontext-% bleiben auf den Kacheln leer).

.PARAMETER NoStart
Das Panel am Ende nicht starten.

.PARAMETER SettingsPath
Gegen eine ANDERE settings.json laufen statt gegen ~/.claude/settings.json. Nur zum
Probelauf gedacht: so biegt ein Test die Hooks des pruefenden Rechners nicht um.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\install.ps1
  powershell -ExecutionPolicy Bypass -File .\install.ps1 -Check
  powershell -ExecutionPolicy Bypass -File .\install.ps1 -Remove
#>
param([switch]$Check, [switch]$Remove, [switch]$Force, [switch]$NoStart,
      [string]$SettingsPath)

$ErrorActionPreference = 'Stop'
$Here = $PSScriptRoot
$ExtDst = Join-Path $env:USERPROFILE '.vscode\extensions\agent-deck-bridge'
$StateDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck\state'

$script:Fails = 0
$script:Warns = 0
function Ok   ($t) { Write-Host "  [ok]   $t" -ForegroundColor Green }
function Warn ($t) { Write-Host "  [warn] $t" -ForegroundColor Yellow; $script:Warns++ }
function Fail ($t) { Write-Host "  [FAIL] $t" -ForegroundColor Red;    $script:Fails++ }
function Step ($t) { Write-Host ''; Write-Host $t -ForegroundColor Cyan }
function Info ($t) { Write-Host "         $t" -ForegroundColor DarkGray }

# Ein Kommando still laufen lassen und nur sagen, ob es geklappt hat.
# Der Parameter heisst NICHT $args: das ist eine automatische PowerShell-Variable
# (die Argumente der Funktion selbst). Ein gleichnamiger Parameter wird ueberschrieben,
# das Kommando startet dann ohne Argumente - bei python heisst das: die REPL geht auf,
# und ihr Banner landet als "Ausgabe" in der Auswertung.
#
# $ErrorActionPreference steht im Skript auf 'Stop', und damit macht Windows PowerShell
# 5.1 aus `2>&1` bei einem NATIVEN Programm einen terminierenden Fehler: die erste
# stderr-Zeile kommt als NativeCommandError im catch an, der REST IST WEG. So wurde am
# 2026-07-30 aus einem viergezeiligen Python-SyntaxError die Meldung
#
#   [FAIL] python.exe startet nicht (Microsoft-Store-Platzhalter?):   File "<string>", line 1
#
# - ausgerechnet die Zeile mit dem `SyntaxError` fehlte, und darum las sich ein Fehler IM
# Aufruf wie ein fehlendes Python. Hier also lokal absenken: die Ausgabe eines
# Kommandos, das scheitern DARF, ist ein Ergebnis und kein Fehler.
function Try-Run([string]$exe, [string[]]$argv) {
    $alt = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $exe @argv 2>&1
        return @{ ok = ($LASTEXITCODE -eq 0); code = $LASTEXITCODE
                  out = (($out | ForEach-Object { "$_" }) -join "`n") }
    } catch {
        return @{ ok = $false; code = -1; out = "$_" }
    } finally {
        $ErrorActionPreference = $alt
    }
}

# Ein deck-Modul laufen lassen, das seine eigenen Urteile faellt. Die Urteile gehoeren
# nach Python, weil sie dort getestet sind (tests/test_claude_hook_setup.py,
# tests/test_ops_vscode_ext.py); dieses Skript zeigt sie nur an und zaehlt mit.
#
# Gezaehlt wird an der ausdruecklichen Bilanzzeile (## fails=N warns=N), nicht am
# Meldungstext: eine Textkopplung bricht beim naechsten Umformulieren still und zeigt
# dann falsche Zahlen. Die Zeile wird aus der Anzeige wieder herausgefiltert.
function Invoke-DeckTool([string]$mod, [string[]]$extra) {
    if (-not $py) { Fail 'ohne Python nicht moeglich'; return }
    $a = @('-m', $mod, '--porcelain')
    if ($extra) { $a += $extra }     # ohne Guard haengt $null als leeres Argument an
    Push-Location $Here              # damit `python -m deck...` das Paket findet
    $script:Bilanz = $false
    $alt = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'   # 2>&1 mit 'Stop' verschluckt Tracebacks, s.o.
    try {
        & $py @a 2>&1 | ForEach-Object {
            $zeile = "$_"
            if ($zeile -match '^## fails=(\d+) warns=(\d+)$') {
                $script:Fails += [int]$Matches[1]
                $script:Warns += [int]$Matches[2]
                $script:Bilanz = $true
            } else { Write-Host $zeile }
        }
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $alt
        Pop-Location
    }
    # Beide Module drucken die Bilanz auf JEDEM Rueckgabeweg. Fehlt sie, ist das Modul
    # nicht bis zum Ende gekommen (Traceback, Tippfehler im Modulnamen, halbe
    # Installation) - und dann zaehlt dieser Lauf null Befunde und meldet am Ende gruen.
    # Das ist dieselbe Sorte Luege wie ein Hook mit Exit 0, der nichts schreibt.
    if (-not $script:Bilanz) {
        Fail "python -m $mod kam ohne Bilanz zurueck (Exit $code) - seine Ausgabe steht darueber."
    }
}

Write-Host ''
Write-Host 'Agent Deck - Einrichtung' -ForegroundColor White
Write-Host "  Repo: $Here" -ForegroundColor DarkGray

# ── 1. Voraussetzungen ───────────────────────────────────────────────────────
Step '1/5  Voraussetzungen'

$py = (Get-Command python.exe -ErrorAction SilentlyContinue).Source
if (-not $py) {
    Fail 'python.exe nicht auf dem PATH. Installer von python.org (3.12+), "Add to PATH" anhaken.'
} else {
    # Ob die Version reicht, entscheidet Python selbst (Exit 9 = zu alt). Ein
    # [version]-Cast in PowerShell verliert sich sonst an Ausgaben wie "3.14.0rc1"
    # oder am REPL-Banner, falls der Aufruf ins Interaktive kippt.
    # Der Store-Stub heisst auch python.exe, oeffnet aber nur den Microsoft Store.
    #
    # KEINE Anfuehrungszeichen in dieses Snippet: Windows PowerShell 5.1 entfernt sie
    # beim Weitergeben an ein natives Programm. Aus print("%d") wurde so print(%d),
    # Python warf einen SyntaxError - und der Doctor meldete "python.exe startet nicht
    # (Microsoft-Store-Platzhalter?)", obwohl Python tadellos lief. Unter pwsh 7 lief
    # dasselbe Snippet durch, unter `powershell -File install.ps1` also nicht: genau
    # die Zeile, die in der Doku steht. sys.version.split() braucht keine Quotes.
    $v = Try-Run $py @('-c',
        'import sys; print(sys.version.split()[0]); sys.exit(0 if sys.version_info >= (3,12) else 9)')
    $ver = ($v.out -split "`n")[0].Trim()
    if ($v.code -eq 9)      { Fail "Python $ver gefunden, gebraucht wird 3.12+." }
    elseif (-not $v.ok)     {
        # Die Ausgabe MEHRZEILIG zeigen und nichts behaupten, was sie nicht sagt: wer
        # eine Fehlermeldung liefert, ist gestartet. Ein Store-Platzhalter sagt "Python
        # wurde nicht gefunden"; ein SyntaxError in "<string>" sagt, dass die
        # Kommandozeile verstuemmelt ankam - zwei Fehler, eine Zeile, verschiedene Wege.
        Fail "python.exe liefert keine Version (Exit $($v.code)). Wortlaut:"
        $v.out -split "`n" | ForEach-Object { Info $_ }
        Info 'SyntaxError in "<string>"? Dann kam das Snippet zerhackt an - install.ps1 aktualisieren (git pull).'
    }
    else {
        Ok "Python $ver ($py)"
        # tkinter fehlt bei der Store-Python - und ohne es gibt es kein Fenster.
        if ((Try-Run $py @('-c', 'import tkinter')).ok) { Ok 'tkinter vorhanden' }
        else { Fail 'tkinter fehlt. Store-Python? Den Installer von python.org nehmen.' }
    }
}

if ((Get-Command code -ErrorAction SilentlyContinue)) { Ok 'VS Code auf dem PATH' }
elseif (Test-Path (Join-Path $env:USERPROFILE '.vscode')) { Ok 'VS Code gefunden (~/.vscode)' }
else { Warn 'VS Code nicht gefunden - ohne es gibt es keine Terminals zu ueberwachen.' }

# claude.cmd (npm) ist der Weg; eine native claude.exe auf dem PATH macht Probleme.
$claude = Get-Command claude -ErrorAction SilentlyContinue
if ($claude) { Ok "Claude Code CLI ($($claude.Source))" }
else { Warn 'claude nicht auf dem PATH - ohne die CLI gibt es nichts zu ueberwachen.' }

if (Test-Path (Join-Path $env:USERPROFILE '.claude\.credentials.json')) {
    Ok 'Claude Code ist angemeldet'
} else {
    Warn 'Keine .credentials.json - einmal "claude auth login" ausfuehren (sonst bleibt die Usage-Anzeige auf "-").'
}

# ── 2. Pillow ────────────────────────────────────────────────────────────────
Step '2/5  Abhaengigkeit (Pillow)'

if (-not $py) {
    Fail 'ohne Python nicht pruefbar'
} elseif ((Try-Run $py @('-c', 'import PIL')).ok) {
    Ok 'Pillow vorhanden'
} elseif ($Check -or $Remove) {
    Fail 'Pillow fehlt - install.ps1 ohne -Check holt es.'
} else {
    Info 'pip install -r requirements.txt'
    $r = Try-Run $py @('-m', 'pip', 'install', '-q', '-r', (Join-Path $Here 'requirements.txt'))
    if ($r.ok -and (Try-Run $py @('-c', 'import PIL')).ok) { Ok 'Pillow installiert' }
    else { Fail "pip fehlgeschlagen: $($r.out)" }
}

# ── 3. Extension ─────────────────────────────────────────────────────────────
Step '3/5  VS-Code-Extension'

# Die Hauptdatei steht stellvertretend fuer "ueberhaupt installiert"; verglichen wird
# darunter der ganze Ordner.
$dstJs = Join-Path $ExtDst 'extension.js'

if ($Remove) {
    if (Test-Path $ExtDst) { Remove-Item $ExtDst -Recurse -Force; Ok 'Extension entfernt' }
    else { Ok 'Extension war nicht installiert' }
    # Auch aus der Registratur nehmen: ein Eintrag ohne Ordner ist genau der Zustand,
    # der die Pruefung unten ueberhaupt erst noetig gemacht hat.
    Invoke-DeckTool 'deck.ops.vscode_ext' @('--remove')
} elseif ($Check) {
    if (-not (Test-Path $dstJs)) {
        Fail "Extension nicht installiert ($ExtDst)"
    } else {
        # ALLE Dateien vergleichen, nicht nur extension.js. Die Extension besteht aus
        # mehreren Modulen (detect.js, killable.js), und ein fehlendes davon ist kein
        # Teilausfall: `require` wirft beim Aktivieren, VS Code laedt die Extension GAR
        # NICHT - auch die Bruecke zum Panel ist dann tot. Ein Check, der nur die
        # Hauptdatei ansieht, meldete dabei gruen. Dieselbe Sorte blinder Fleck wie der
        # Ordner ohne Registratur-Eintrag.
        $fehlend = @(); $abweichend = @()
        foreach ($f in Get-ChildItem (Join-Path $Here 'extension') -File) {
            $dst = Join-Path $ExtDst $f.Name
            if (-not (Test-Path $dst)) { $fehlend += $f.Name }
            elseif ((Get-FileHash $f.FullName).Hash -ne (Get-FileHash $dst).Hash) { $abweichend += $f.Name }
        }
        # Genau dieses Fehlerbild stand schon zweimal hinter "verbindet nicht mehr".
        if ($fehlend) {
            Fail "Extension unvollstaendig - fehlt: $($fehlend -join ', '). install.ps1 neu laufen lassen, dann Reload Window."
        } elseif ($abweichend) {
            Fail "Installierte Extension weicht vom Repo ab ($($abweichend -join ', ')) - install.ps1 neu laufen lassen, dann Reload Window."
        } else {
            Ok "Extension-Ordner ist da und aktuell (alle Dateien)"
        }
    }
    # Der Ordner allein beweist NICHTS: geladen wird, was in VS Codes extensions.json
    # steht. Am 2026-07-30 zeigte der Eintrag dort auf einen umbenannten Ordner, und
    # dieser Schritt meldete gruen, waehrend VS Code die Extension nie geladen hat.
    Invoke-DeckTool 'deck.ops.vscode_ext' @('--check')
} else {
    # Bilanz ueber ALLE Dateien ziehen, sonst meldet ein Lauf "war schon aktuell",
    # weil extension.js gleich blieb - waehrend ein neues Modul daneben fehlt.
    $vorher = @{}
    if (Test-Path $ExtDst) {
        foreach ($f in Get-ChildItem $ExtDst -File) { $vorher[$f.Name] = (Get-FileHash $f.FullName).Hash }
    }
    New-Item -ItemType Directory -Force -Path $ExtDst | Out-Null
    Copy-Item (Join-Path $Here 'extension\*') $ExtDst -Recurse -Force
    $neu = @(Get-ChildItem $ExtDst -File | Where-Object { $vorher[$_.Name] -ne (Get-FileHash $_.FullName).Hash })
    if (-not $neu) { Ok 'Extension war schon aktuell' }
    else { Ok "Extension kopiert -> $ExtDst ($($neu.Name -join ', '))" }
    # Erst kopieren, DANN registrieren - umgekehrt legt der Lauf einen Eintrag auf einen
    # Ordner an, den es noch nicht gibt, und VS Code bricht beim Laden ab.
    Invoke-DeckTool 'deck.ops.vscode_ext'
    Info 'In JEDEM offenen VS-Code-Fenster: "Developer: Reload Window"'
}

# ── 4. Hooks in ~/.claude/settings.json ──────────────────────────────────────
$SettingsShown = if ($SettingsPath) { $SettingsPath } else { '~/.claude/settings.json' }
Step "4/5  Hooks und statusLine in $SettingsShown"

$a = @()
if ($Check)  { $a += '--check' }
if ($Remove) { $a += '--remove' }
if ($Force)  { $a += '--force' }
if ($SettingsPath) { $a += @('--settings', $SettingsPath) }
Invoke-DeckTool 'deck.claude.hook_setup' $a

# ── 5. Der Beweis: schreibt ein Hook wirklich? ───────────────────────────────
Step '5/5  Beweis (Hook feuern und state\ pruefen)'

if ($Remove) {
    Ok 'uebersprungen (-Remove)'
} elseif (-not $py) {
    Fail 'ohne Python nicht moeglich'
} else {
    # Ein Slot-Name, den kein Fenster benutzt. Die Datei wird danach geloescht - sonst
    # liegt eine Phantom-Meldung in state\ und das Deck poll sie ewig mit.
    $slot = '__deck_doctor__'
    $probe = Join-Path $StateDir "$slot.json"
    Remove-Item $probe -Force -ErrorAction SilentlyContinue
    $alt = $env:AGENT_SLOT
    $env:AGENT_SLOT = $slot
    try {
        # Leeres JSON auf stdin: so ruft Claude Code den Hook auch auf.
        '{}' | & $py (Join-Path $Here 'report.py') 'idle' 2>&1 | Out-Null
        $code = $LASTEXITCODE
    } finally {
        $env:AGENT_SLOT = $alt
    }
    if (-not (Test-Path $probe)) {
        Fail "report.py hat nichts geschrieben (Exit $code). Erwartet: $probe"
        Info 'Mit start_debug.bat / "python report.py idle" von Hand nachsehen.'
    } else {
        $age = (Get-Date) - (Get-Item $probe).LastWriteTime
        if ($age.TotalSeconds -gt 30) { Fail "Datei in state\ ist $([int]$age.TotalSeconds)s alt - nicht von diesem Lauf." }
        else { Ok "report.py schreibt nach state\ (Exit $code)" }
        Remove-Item $probe -Force -ErrorAction SilentlyContinue
    }
}

# Laeuft schon ein Panel? Der Broker-Port ist der funktionale Test - dort verbinden
# sich die Extensions. SO_REUSEADDR ist in broker.py bewusst AUS, ein zweiter
# Listener auf 8765 ist also nicht harmlos.
$tcp = New-Object Net.Sockets.TcpClient
try {
    $tcp.Connect('127.0.0.1', 8765)
    Info 'Ein Panel laeuft bereits (Broker auf 127.0.0.1:8765 antwortet).'
    $script:PanelLaeuft = $true
} catch { $script:PanelLaeuft = $false } finally { $tcp.Close() }

# ── Fazit ────────────────────────────────────────────────────────────────────
Write-Host ''
if ($script:Fails -gt 0) {
    Write-Host "Ergebnis: $($script:Fails) Problem(e), $($script:Warns) Hinweis(e)." -ForegroundColor Red
    Write-Host 'Oben stehen sie einzeln. Nach dem Beheben nochmal laufen lassen.' -ForegroundColor DarkGray
    exit 1
}
if ($Remove) { Write-Host 'Entfernt. Die Laufzeitdateien im Repo und in state\ sind geblieben.' -ForegroundColor White; exit 0 }
if ($Check)  { Write-Host "Alles in Ordnung ($($script:Warns) Hinweis(e))." -ForegroundColor Green; exit 0 }

Write-Host "Fertig ($($script:Warns) Hinweis(e))." -ForegroundColor Green
Write-Host ''
Write-Host 'Noch zwei Handgriffe, die niemand fuer dich machen kann:' -ForegroundColor White
Write-Host '  1. In jedem offenen VS-Code-Fenster: "Developer: Reload Window"'
Write-Host '  2. Im Panel oben auf "Fenster A" klicken, dann das VS-Code-Fenster anklicken'
Write-Host ''

if (-not $NoStart -and -not $script:PanelLaeuft) {
    Write-Host 'Panel starten...' -ForegroundColor DarkGray
    Start-Process -FilePath (Join-Path $Here 'start.bat') -WorkingDirectory $Here
}
exit 0
