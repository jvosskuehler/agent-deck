# Ein Ablauf des Messstands (tools/probe/README.md), in der Desktop-Sitzung der VM.
#
#   scenario.ps1 -Name new -Phase setup        Ausgangslage herstellen (ohne Messung)
#   scenario.ps1 -Name new -Phase run -Out C:\probe\out\base-new [-Load 6]
#
# `run` schreibt <Out>\events.jsonl (Fensterereignisse, Marken) und <Out>\info.json
# (Kosten vorher/nachher, Zeit bis tippbereit). Die Bildschirmaufnahme macht der Host der VM
# von außen (host/vncrec.py) und richtet sich über das Sync-Feld nach den Marken.
param(
    [Parameter(Mandatory = $true)][ValidateSet('new', 'close', 'focus', 'gather', 'reorder', 'resize', 'idle')][string]$Name,
    [ValidateSet('setup', 'run')][string]$Phase = 'run',
    [string]$Out = "C:\probe\out\$Name",
    [int]$Load = 0
)
. C:\probe\deck.ps1
$L = Get-HomeLetter

# Ausgangslage je Ablauf: so viele Agenten, A1 vorn, fremde Fenster wie nötig.
$count = @{ new = 2; close = 3; focus = 3; gather = 3; reorder = 3; resize = 3; idle = 3 }[$Name]

if ($Phase -eq 'setup') {
    Get-Process notepad -ErrorAction SilentlyContinue | Stop-Process -Force
    # Ausgangslage der Anzeige (nach `resize` steht sie sonst woanders).
    [Probe]::SetResolution(1920, 1080) | Out-Null
    [Probe]::SetScaleStep(0) | Out-Null
    $reachable = try { [bool](Get-PanelTarget) } catch { $false }
    if (-not $reachable) { Start-Deck; Start-Sleep 3 }
    # Versteckte (minimierte) Agenten erst holen, dann zählen.
    $alive = Get-ChildItem (Join-Path $DeckDir 'state') -Filter '*.wtpid' | ForEach-Object { $_.BaseName }
    if ($alive -and (Get-Agents).Count -lt @($alive).Count) { [Probe]::Keys(@(0x11, 0x10, 0xE2)); Start-Sleep 2 }
    Set-AgentCount $L $count
    # Gleiche Reihenfolge wie die Slots (nach einem Umordnen sonst vertauscht).
    Invoke-Deck 'reorder_tiles' @{ letter = $L; slots = @(1..$count | ForEach-Object { "$L$_" }) }
    Start-Sleep 2
    Invoke-Deck 'focus_slot' @{ slot = "${L}1" }
    if ($Name -eq 'gather') {
        Start-Process notepad -WindowStyle Maximized
    }
    Wait-Agents (1..$count | ForEach-Object { "$L$_" }) 20 | Out-Null
    Start-Sleep 1
    "setup ${Name}: $((Get-Agents).Keys -join ',') vorn: $([Probe]::ForegroundTitle())"
    return
}

New-Item -ItemType Directory -Force $Out | Out-Null
Remove-Item C:\probe\ready\* -ErrorAction SilentlyContinue
$spin = @()
if ($Load -gt 0) {
    # Last: so viele Rechenschleifen wie verlangt, normale Priorität, eigene Prozesse.
    $spin = 1..$Load | ForEach-Object { Start-Process powershell -ArgumentList '-NoProfile', '-Command', 'for(;;){}' -WindowStyle Hidden -PassThru }
    Start-Sleep 1
}
$before = Get-Cost
[Probe]::Start("$Out\events.jsonl")
[Probe]::Note((@{ utc_ticks = [DateTime]::UtcNow.Ticks; name = $Name; load = $Load } | ConvertTo-Json -Compress))
Start-Sleep -Milliseconds 1500
[Probe]::Mark('start') | Out-Null
Start-Sleep -Milliseconds 700
$info = [ordered]@{ name = $Name; load = $Load }

switch ($Name) {
    'new' {
        $t = [Probe]::Mark('new')
        $utc = [DateTime]::UtcNow.Ticks
        Invoke-Deck 'create_agent' @{ letter = $L }
        $slot = "${L}3"
        $until = (Get-Date).AddSeconds(15)
        while (-not (Test-Path "C:\probe\ready\$slot.txt") -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 20 }
        if (Test-Path "C:\probe\ready\$slot.txt") {
            $info.ready_ms = [math]::Round(([long](Get-Content "C:\probe\ready\$slot.txt") - $utc) / 10000)
        }
        Start-Sleep 5
    }
    'close' {
        [Probe]::Mark('close') | Out-Null
        Invoke-Deck 'close_slot' @{ slot = "${L}2" }
        Start-Sleep 5
    }
    'focus' {
        foreach ($s in "${L}3", "${L}2", "${L}1") {
            [Probe]::Mark("focus $s") | Out-Null
            Invoke-Deck 'focus_slot' @{ slot = $s }
            Start-Sleep -Milliseconds 1500
        }
        # Sprung-Taste Strg+< zum wartenden Agenten (Slot 2 wartet, fake-claude.ps1).
        [Probe]::Mark('jump') | Out-Null
        [Probe]::Keys(@(0x11, 0xE2))
        Start-Sleep -Milliseconds 1500
    }
    'gather' {
        foreach ($k in 'fetch', 'hide', 'fetch-min') {
            [Probe]::Mark($k) | Out-Null
            [Probe]::Keys(@(0x11, 0x10, 0xE2))
            Start-Sleep 2
        }
    }
    'reorder' {
        [Probe]::Mark('reorder') | Out-Null
        Invoke-Deck 'reorder_tiles' @{ letter = $L; slots = @("${L}3", "${L}1", "${L}2") }
        Start-Sleep 2.5
        [Probe]::Mark('reorder-back') | Out-Null
        Invoke-Deck 'reorder_tiles' @{ letter = $L; slots = @("${L}1", "${L}2", "${L}3") }
        Start-Sleep 2.5
    }
    'resize' {
        # Andere Skalierung auf demselben Monitor (125 %) und zurück: liegen die Fenster
        # danach wieder als Raster (ohne Lücken, ohne Überdeckung)? Eine andere Auflösung
        # kann der Basic Display Adapter der VM nicht, die Skalierung wirkt gleich: die
        # Arbeitsfläche in logischen Pixeln ändert sich.
        [Probe]::Mark('resize') | Out-Null
        [Probe]::SetScaleStep(1) | Out-Null
        Start-Sleep 5
        [Probe]::Mark('resize-back') | Out-Null
        [Probe]::SetScaleStep(0) | Out-Null
        Start-Sleep 5
    }
    'idle' {
        Start-Sleep 30
    }
}
[Probe]::Mark('end') | Out-Null
Start-Sleep -Milliseconds 500
[Probe]::Stop()
$after = Get-Cost
$spin | Stop-Process -Force -ErrorAction SilentlyContinue
$info.before = $before
$info.after = $after
$info | ConvertTo-Json -Depth 4 | Set-Content "$Out\info.json"
"run ${Name} fertig: $((Get-Agents).Keys -join ',')"
