# Einmal je Test-VM, in der Desktop-Sitzung (session.ps1): Auflösung, einfarbiger
# Desktop (Magenta, damit die Auswertung Lücken zwischen Fenstern als Desktop erkennt),
# Attrappe als `claude` im PATH, ein Repos-Ordner mit einem Repo darin.
param([int]$Width = 1920, [int]$Height = 1080)
$ErrorActionPreference = 'Continue'
Add-Type -Path C:\probe\Probe.cs
"Auflösung ${Width}x${Height}: $([Probe]::SetResolution($Width, $Height))"
[Probe]::SolidDesktop(255, 0, 255)
Set-ItemProperty 'HKCU:\Control Panel\Colors' -Name Background -Value '255 0 255'
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name Wallpaper -Value ''

# Bildschirm nie abschalten: ein schwarzer Bildschirm ergibt ein schwarzes Video.
powercfg /change monitor-timeout-ac 0
powercfg /change standby-timeout-ac 0
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name ScreenSaveActive -Value '0'

# Attrappe statt Claude: wt_agent.ps1 ruft `& claude`, PowerShell findet claude.ps1 im PATH.
New-Item -ItemType Directory -Force C:\probe\bin | Out-Null
Copy-Item C:\probe\fake-claude.ps1 C:\probe\bin\claude.ps1 -Force
$p = [Environment]::GetEnvironmentVariable('PATH', 'User')
if ($p -notlike '*C:\probe\bin*') { [Environment]::SetEnvironmentVariable('PATH', "C:\probe\bin;$p", 'User') }

# Heimat-Block: ein Ordner mit mindestens einem Repo darin (deck_settings::repos_state).
New-Item -ItemType Directory -Force C:\Repos\demo\.git | Out-Null
'vorbereitet'
