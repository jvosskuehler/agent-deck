# Startet einen Claude-Agenten in einem Windows-Terminal-Pane, das das Agent Deck
# geoeffnet hat (app/src-tauri/src/wt_host.rs). Laeuft mit -NoExit: endet claude, bleibt die Shell im
# Pane stehen - wie ein VS-Code-Terminal nach `exit` aus claude.
#
# -Spec ist Base64-JSON aus deck-core wt::launch_spec: { slot, state, args }.
param([Parameter(Mandatory = $true)][string]$Spec)

$s = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Spec)) | ConvertFrom-Json

# 1) Die PID-Datei ZUERST: an ihr erkennt das Deck, dass der Slot lebt. Startzeit dazu,
#    damit eine spaeter neu vergebene PID nicht als dieser Slot durchgeht.
$me = Get-Process -Id $PID
$rec = @{ pid = $PID; created = $me.StartTime.ToFileTimeUtc(); slot = $s.slot } | ConvertTo-Json -Compress
$path = Join-Path $s.state ($s.slot + '.wtpid')
[IO.Directory]::CreateDirectory($s.state) | Out-Null
[IO.File]::WriteAllText($path + '.tmp', $rec)
Move-Item -LiteralPath ($path + '.tmp') -Destination $path -Force

# 2) Den Slot fuer die Hooks (deck-report/deck-statusline lesen AGENT_SLOT). wt.exe kann einem
#    neuen Pane keine Umgebungsvariable mitgeben, darum steht das hier.
$env:AGENT_SLOT = $s.slot

# 3) Saubere Umgebung. Das Deck laeuft oft selbst unter einer Claude-Session und vererbt
#    deren Variablen an jedes Pane: CLAUDE_CODE_CHILD_SESSION schaltet im neuen Agenten
#    das Speichern des Transcripts ab (dann findet --resume nichts), NO_COLOR (setzt
#    Claude fuer seine Werkzeug-Shells) nimmt ihm alle Farben.
foreach ($n in 'CLAUDECODE', 'CLAUDE_CODE_CHILD_SESSION', 'CLAUDE_CODE_SESSION_ID',
               'CLAUDE_CODE_SESSION_ATTENDED', 'CLAUDE_CODE_ENTRYPOINT', 'CLAUDE_CODE_EXECPATH',
               'CLAUDE_CODE_MESSAGING_SOCKET', 'CLAUDE_CODE_MESSAGING_TOKEN', 'NO_COLOR') {
    Remove-Item "Env:$n" -ErrorAction SilentlyContinue
}
# Windows Terminal kann 24-Bit-Farben, sagt es aber nicht an; ohne die Variable rechnet
# Claude seine Farben auf 256 herunter.
$env:COLORTERM = 'truecolor'

# 4) claude mit den Argumenten aus dem Deck (Modell, beim Neustart --resume). Gibt es
#    auf diesem Rechner eine Settings-Datei nur fuer die Terminal-Agenten (etwa ein Theme
#    passend zum Profil "Agent Deck"), kommt sie per --settings dazu.
$argv = @()
if ($s.args) { $argv = @($s.args) }
$wtSettings = Join-Path $HOME '.claude\agent-deck-wt.settings.json'
if (Test-Path -LiteralPath $wtSettings) { $argv = @('--settings', $wtSettings) + $argv }
& claude @argv
