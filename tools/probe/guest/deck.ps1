# Hilfen für scenario.ps1: Deck starten, Befehle an die Oberfläche (über das
# DevTools-Protokoll der WebView2, Port 9222), auf Fenster warten. Wird per Punkt geladen.
$ErrorActionPreference = 'Stop'
$DeckDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck'
$DeckExe = Join-Path $DeckDir 'bin\agent-deck.exe'
if (-not ('Probe' -as [type])) { Add-Type -Path C:\probe\Probe.cs }

# Das Deck mit offenem DevTools-Port starten (beendet ein laufendes vorher).
#
# Den Port öffnet das Deck selbst, wenn AGENT_DECK_DEVTOOLS_PORT gesetzt ist
# (ui_windows.rs). Übrig gebliebene WebView2-Prozesse eines früheren Decks müssen mit
# weg: eine neue WebView hängt sich sonst an deren Browser, und der hat den Port nicht. Gestartet wird
# über eine eigene geplante Aufgabe, damit das Deck nicht als Kind dieses Skripts mit
# dessen Aufgabe endet.
function Start-Deck {
    Get-Process agent-deck, deck-watchdog, msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 800
    $cmd = 'C:\probe\start-deck.cmd'
    Set-Content $cmd "@set AGENT_DECK_DEVTOOLS_PORT=9222`r`n@cd /d `"$(Split-Path $DeckExe)`"`r`n@start `"`" `"$DeckExe`"" -Encoding ASCII
    schtasks /Create /F /TN deckprobe-deck /SC ONCE /ST 23:59 /IT /RU Administrator /TR $cmd | Out-Null
    schtasks /Run /TN deckprobe-deck | Out-Null
    for ($i = 0; $i -lt 60; $i++) {
        try { if (Get-PanelTarget) { return } } catch {}
        Start-Sleep -Milliseconds 500
    }
    throw 'Deck-Oberfläche nicht erreichbar'
}

function Get-PanelTarget {
    (Invoke-RestMethod http://127.0.0.1:9222/json -TimeoutSec 2) | Where-Object { $_.type -eq 'page' -and $_.url -like '*#/panel*' } | Select-Object -First 1
}

$script:cdpId = 0
# Einen Tauri-Befehl der Oberfläche aufrufen, z. B. Invoke-Deck 'focus_slot' @{ slot = 'A2' }.
function Invoke-Deck([string]$Command, [hashtable]$Arguments = @{}) {
    $target = Get-PanelTarget
    $ws = [System.Net.WebSockets.ClientWebSocket]::new()
    $ws.ConnectAsync([Uri]$target.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
    $script:cdpId++
    $js = "window.__TAURI_INTERNALS__.invoke('$Command', $($Arguments | ConvertTo-Json -Compress -Depth 5))"
    $msg = @{ id = $script:cdpId; method = 'Runtime.evaluate'; params = @{ expression = $js; awaitPromise = $true } } | ConvertTo-Json -Compress -Depth 6
    $bytes = [Text.Encoding]::UTF8.GetBytes($msg)
    $ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, [Threading.CancellationToken]::None).Wait()
    $buf = [byte[]]::new(65536)
    $text = ''
    while ($true) {
        $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), [Threading.CancellationToken]::None).Result
        $text += [Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
        if ($r.EndOfMessage) {
            if ($text -like "*`"id`":$($script:cdpId)*") { break }
            $text = ''
        }
    }
    $ws.Dispose()
    if ($text -like '*exceptionDetails*') { throw "Deck-Befehl $Command gescheitert: $text" }
}

# Buchstabe des Heimat-Blocks (der erste in wt_hosts.json).
function Get-HomeLetter {
    $h = Get-Content (Join-Path $DeckDir 'wt_hosts.json') -Raw | ConvertFrom-Json
    ($h.PSObject.Properties | Select-Object -First 1).Name
}

# Sichtbare Agentenfenster als Tabelle Slot → Rahmen.
function Get-Agents { [Probe]::Agents() }

# Warten, bis genau diese Agentenfenster offen sind und seit 1,5 s still liegen.
function Wait-Agents([string[]]$Slots, [int]$TimeoutS = 30) {
    $until = (Get-Date).AddSeconds($TimeoutS)
    $last = ''; $since = Get-Date
    while ((Get-Date) -lt $until) {
        $a = Get-Agents
        $now = ($a.Keys | Sort-Object | ForEach-Object { $r = $a[$_]; "$_=$($r.L),$($r.T),$($r.R),$($r.B)" }) -join ';'
        if ($now -ne $last) { $last = $now; $since = Get-Date }
        $ok = ($Slots | Where-Object { $a.ContainsKey($_) }).Count -eq $Slots.Count -and $a.Count -eq $Slots.Count
        if ($ok -and ((Get-Date) - $since).TotalSeconds -ge 1.5) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

# Den Block auf genau die Agenten 1..N bringen. Passt die Menge nicht (nach einem
# Schließen vergibt das Deck den nächsten Namen hinten, A4 statt A2), alle schließen und
# frisch anlegen: so beginnt jeder Ablauf mit denselben Namen und Farben.
function Set-AgentCount([string]$Letter, [int]$N) {
    $want = 1..$N | ForEach-Object { "$Letter$_" }
    $have = @((Get-Agents).Keys | Where-Object { $_ -like "$Letter*" } | Sort-Object)
    if (($have -join ',') -eq (($want | Sort-Object) -join ',')) { return }
    foreach ($s in $have) { Invoke-Deck 'close_slot' @{ slot = $s }; Start-Sleep 3 }
    $until = (Get-Date).AddSeconds(20)
    while ((Get-ChildItem (Join-Path $DeckDir 'state') -Filter "$Letter*.wtpid").Count -gt 0 -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 300 }
    for ($i = 0; $i -lt $N; $i++) { Invoke-Deck 'create_agent' @{ letter = $Letter }; Start-Sleep 4 }
    if (-not (Wait-Agents $want 40)) { throw "Agenten $($want -join ',') nicht bereit: $((Get-Agents).Keys -join ',')" }
}

# Prozessdaten des Decks und der Agenten: CPU-Zeit (ms), Arbeitsspeicher (MB), Anzahl.
function Get-Cost {
    $deck = Get-Process agent-deck, msedgewebview2 -ErrorAction SilentlyContinue
    $wt = Get-Process WindowsTerminal, OpenConsole, pwsh -ErrorAction SilentlyContinue
    [ordered]@{
        deck_cpu_ms = [math]::Round(($deck | ForEach-Object { $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum)
        deck_ws_mb = [math]::Round(($deck | Measure-Object WorkingSet64 -Sum).Sum / 1MB)
        deck_procs = @($deck).Count
        # Nur agent-deck.exe (ohne WebView2): da lebt Fokus-Rahmen, Ringe, Vorhang.
        deck_exe_cpu_ms = [math]::Round((Get-Process agent-deck -ErrorAction SilentlyContinue | ForEach-Object { $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum)
        deck_exe_ws_mb = [math]::Round((Get-Process agent-deck -ErrorAction SilentlyContinue | Measure-Object WorkingSet64 -Sum).Sum / 1MB)
        agent_cpu_ms = [math]::Round(($wt | ForEach-Object { $_.TotalProcessorTime.TotalMilliseconds } | Measure-Object -Sum).Sum)
        agent_procs = @($wt).Count
    }
}
