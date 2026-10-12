param(
    [string]$WinRoot = (Join-Path $env:LOCALAPPDATA ('Devbox\build-' + [guid]::NewGuid().ToString('N'))),
    [string]$Distro = 'Ubuntu'
)
$ErrorActionPreference = 'Stop'
& "$PSScriptRoot\win-dev.ps1" -Mode build -Release -WinRoot $WinRoot -Distro $Distro
