# UI-Automation-Arbeiter fuer Windows Terminal - Gegenstueck zu deck/platform/wt_uia.py.
#
# Laeuft dauerhaft unter Windows PowerShell 5.1 (die UIAutomationClient-Assemblies sind
# dort immer da) und beantwortet je Zeile auf stdin eine JSON-Frage mit einer JSON-
# Zeile auf stdout. Dauerhaft, weil allein das Laden der Assemblies rund eine Sekunde
# kostet - pro Kachelklick waere das zu traege.
#
# Was Windows Terminal ueber UIA zeigt (gemessen 2026-11-06, WT 1.24): jedes Pane ist
# ein Element der Klasse "TermControl", sein Name ist der Pane-Titel - beim Deck also
# der Slot-Name. Panes in einem NICHT aktiven Tab stehen nicht im Baum; dafuer gibt es
# die Tab-Elemente (TabItem), die sich per SelectionItemPattern auswaehlen lassen.
# Beides funktioniert auch bei minimiertem Fenster.
#
# Seit 2026-11-06 hat jeder Agent ein eigenes Fenster (ops/wt_tiles), und das Deck
# findet es am Fenstertitel. Dieser Arbeiter bleibt der Rueckfall fuer Agenten, die
# noch als Pane in einem alten, geteilten Fenster laufen. Nachgemessen am selben Tag
# hiess jedes TermControl dort allerdings "Agent Deck" (Profilname), nicht wie der Slot.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
[Console]::InputEncoding = [Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object Text.UTF8Encoding $false

$AE = [Windows.Automation.AutomationElement]
$TS = [Windows.Automation.TreeScope]
$SelPattern = [Windows.Automation.SelectionItemPattern]::Pattern
$condWin = New-Object Windows.Automation.PropertyCondition($AE::ClassNameProperty, 'CASCADIA_HOSTING_WINDOW_CLASS')
$condTerm = New-Object Windows.Automation.PropertyCondition($AE::ClassNameProperty, 'TermControl')
$condTab = New-Object Windows.Automation.PropertyCondition($AE::ControlTypeProperty, [Windows.Automation.ControlType]::TabItem)

function Get-WtWindows { $AE::RootElement.FindAll($TS::Children, $condWin) }

function Get-Pane($win, [string]$slot) {
    $byName = New-Object Windows.Automation.PropertyCondition($AE::NameProperty, $slot)
    $win.FindFirst($TS::Descendants, (New-Object Windows.Automation.AndCondition($condTerm, $byName)))
}

function Find-Pane([string]$slot) {
    # Erst im jeweils aktiven Tab (kein sichtbarer Nebeneffekt) ...
    foreach ($w in Get-WtWindows) {
        $p = Get-Pane $w $slot
        if ($p) { return ,@($w, $p) }
    }
    # ... dann Tab fuer Tab. Findet sich nichts, wird der vorher aktive Tab wieder
    # ausgewaehlt: eine erfolglose Suche soll nichts umgestellt hinterlassen.
    foreach ($w in Get-WtWindows) {
        $tabs = @($w.FindAll($TS::Descendants, $condTab))
        $before = $null
        foreach ($t in $tabs) {
            try { if ($t.GetCurrentPattern($SelPattern).Current.IsSelected) { $before = $t } } catch {}
        }
        foreach ($t in $tabs) {
            try { $t.GetCurrentPattern($SelPattern).Select() } catch { continue }
            Start-Sleep -Milliseconds 80
            $p = Get-Pane $w $slot
            if ($p) { return ,@($w, $p) }
        }
        if ($before) { try { $before.GetCurrentPattern($SelPattern).Select() } catch {} }
    }
    return $null
}

while ($true) {
    $line = [Console]::In.ReadLine()
    if ($null -eq $line) { break }
    try {
        $q = $line | ConvertFrom-Json
        switch ($q.op) {
            'find' {
                $r = Find-Pane $q.slot
                $res = if ($r) { @{ hwnd = $r[0].Current.NativeWindowHandle } } else { @{ hwnd = $null } }
            }
            'focus' {
                $r = Find-Pane $q.slot
                if ($r) {
                    $r[1].SetFocus()
                    $res = @{ hwnd = $r[0].Current.NativeWindowHandle; ok = $true }
                } else { $res = @{ hwnd = $null; ok = $false } }
            }
            'ping' { $res = @{ ok = $true } }
            default { $res = @{ error = "unbekannt: $($q.op)" } }
        }
        $out = $res | ConvertTo-Json -Compress
    } catch {
        $out = @{ error = "$_" } | ConvertTo-Json -Compress
    }
    [Console]::Out.WriteLine($out)
    [Console]::Out.Flush()
}
