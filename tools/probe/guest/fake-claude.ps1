# Attrappe für Claude Code im Messstand (tools/probe/README.md). Liegt als claude.ps1 im
# PATH der Test-VM; wt_agent.ps1 startet sie wie das echte `claude`.
#
# Verhält sich für das Deck wie ein Agent: Vollbild-Oberfläche im alternativen Puffer,
# zeichnet nach jeder Größenänderung neu (wie Claude Code mit Ink), meldet einen Zustand
# in die Zustandsdatei (Statusring in Farbe). Dazu zwei Marken für die Auswertung: ein
# Block oben links und einer unten rechts in der Farbe des Slots. Aus ihrer Lage im Video
# folgen Ort und Größe des Fensters in jedem Bild, ohne dass der Glow am Rand stört.
# Wann die erste Zeichnung stand, steht in C:\probe\ready\<slot>.txt (UTC-Ticks).
$slot = $env:AGENT_SLOT
if (-not $slot) { $slot = 'X0' }
$n = 0
[int]::TryParse(($slot -replace '\D', ''), [ref]$n) | Out-Null
# Markenfarben je Slot-Nummer: weit weg von Statusfarben, Glow-Gold und Desktop-Magenta.
$palette = @(@(30, 60, 230), @(200, 30, 30), @(20, 160, 160), @(120, 40, 200), @(110, 90, 20), @(0, 0, 0), @(60, 60, 140), @(150, 150, 150))
$c = $palette[($n - 1) % $palette.Count]
$e = [char]27
$mark = "$e[48;2;$($c[0]);$($c[1]);$($c[2])m"
# Ring blau (arbeitet); nur Slot 2 wartet (orange), damit die Sprung-Taste ein Ziel hat.
$status = if ($n -eq 2) { 'waiting' } else { 'thinking' }
$stateDir = Join-Path $env:LOCALAPPDATA 'claude-agent-deck\state'

function Write-State {
    $path = Join-Path $stateDir "$slot.json"
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0
    $json = '{"slot":"' + $slot + '","status":"' + $status + '","ts":' + $ts.ToString([Globalization.CultureInfo]::InvariantCulture) + '}'
    try { [IO.File]::WriteAllText("$path.tmp", $json); Move-Item -LiteralPath "$path.tmp" -Destination $path -Force } catch {}
}

function Draw([int]$w, [int]$h) {
    $sb = [Text.StringBuilder]::new()
    [void]$sb.Append("$e[0m$e[2J$e[H")
    [void]$sb.Append("$e[1;3H$e[0m✻ Claude Code (Attrappe) $slot")
    [void]$sb.Append("$e[2;3H$e[2m  /help für Hilfe, ${w}x${h} Zeichen$e[0m")
    for ($i = 0; $i -lt [Math]::Min(12, $h - 14); $i++) {
        $line = "● Zeile ${i}: Lorem ipsum dolor sit amet, consectetur adipiscing elit"
        [void]$sb.Append("$e[$(7 + $i);3H" + $line.Substring(0, [Math]::Min($line.Length, $w - 4)))
    }
    # Marken: oben links (Zeile 4, Spalte 6) und unten rechts (fünf Zeilen über dem Ende),
    # beide gut 60 Pixel vom Rand: dort färbt der Glow eines Nachbarn nicht mehr.
    foreach ($row in 0, 1) {
        [void]$sb.Append("$e[$(4 + $row);6H$mark        $e[0m")
        [void]$sb.Append("$e[$($h - 5 + $row);$($w - 13)H$mark        $e[0m")
    }
    # Eingabefeld wie bei Claude.
    [void]$sb.Append("$e[$($h - 2);1H$e[2m" + ('─' * $w) + "$e[0m")
    [void]$sb.Append("$e[$($h - 1);1H> ")
    [Console]::Out.Write($sb.ToString())
    [Console]::Out.Flush()
}

[Console]::Out.Write("$e[?1049h$e[?25l")
$w = 0; $h = 0; $ready = $false; $beat = [Diagnostics.Stopwatch]::StartNew()
Write-State
while ($true) {
    $nw = [Console]::WindowWidth; $nh = [Console]::WindowHeight
    if ($nw -ne $w -or $nh -ne $h) {
        $w = $nw; $h = $nh
        if ($w -gt 20 -and $h -gt 12) { Draw $w $h }
        if (-not $ready) {
            $ready = $true
            New-Item -ItemType Directory -Force C:\probe\ready | Out-Null
            Set-Content "C:\probe\ready\$slot.txt" ([DateTime]::UtcNow.Ticks)
        }
    }
    if ($beat.ElapsedMilliseconds -gt 2000) { Write-State; $beat.Restart() }
    Start-Sleep -Milliseconds 30
}
