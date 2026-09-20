param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Pinned NSIS acquisition requires disposable hosted Windows.' }
if (Test-Path -LiteralPath $Destination) { throw 'New tool directory required.' }
$lock = (Get-Content -Raw "$PSScriptRoot/suite-build-tools.json" | ConvertFrom-Json).nsis
if ($lock.url -cne 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip') { throw 'Unknown compiler origin.' }
New-Item -ItemType Directory -Path $Destination | Out-Null
$archive = Join-Path $Destination 'nsis.zip'
Invoke-WebRequest -Uri $lock.url -OutFile $archive -TimeoutSec 120
if ((Get-Item -LiteralPath $archive).Length -ne $lock.size -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.sha256) { throw 'NSIS size/digest mismatch.' }
Expand-Archive -LiteralPath $archive -DestinationPath $Destination
$compiler = Join-Path $Destination 'nsis-3.11/makensis.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'Pinned NSIS compiler missing.' }
return $compiler
