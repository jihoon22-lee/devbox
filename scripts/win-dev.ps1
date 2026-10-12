param(
    [ValidateSet('dev', 'build')][string]$Mode = 'dev',
    [string]$WinRoot = (Join-Path $env:LOCALAPPDATA ('Devbox\build-' + [guid]::NewGuid().ToString('N'))),
    [string]$Distro = 'Ubuntu', [switch]$Release
)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).ProviderPath
$target = & "$PSScriptRoot\win-sync.ps1" -WinRoot $WinRoot
$linuxRepo = & wsl.exe -d $Distro --exec wslpath -a $source
if ($LASTEXITCODE -ne 0) { throw 'WSL source path conversion failed' }
$linuxRepo = ($linuxRepo -join "`n").Trim()
$profile = if ($Release -or $Mode -eq 'build') { 'release' } else { 'debug' }
& wsl.exe -d $Distro --cd $linuxRepo --exec bash scripts/build-linux.sh $profile
if ($LASTEXITCODE -ne 0) { throw 'WSL musl build failed' }
$metadata = & wsl.exe -d $Distro --cd $linuxRepo --exec bash -c 'source ~/.cargo/env && cargo metadata --locked --format-version 1 --no-deps'
if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
$metadata = $metadata -join "`n" | ConvertFrom-Json
$linuxTarget = $metadata.target_directory
if (!$linuxTarget) { throw 'Cargo target directory missing' }
$binary = & wsl.exe -d $Distro --exec wslpath -w "$linuxTarget/x86_64-unknown-linux-musl/$profile/devbox"
if ($LASTEXITCODE -ne 0) { throw 'WSL binary path conversion failed' }
$resources = Join-Path $target 'app\src-tauri\resources'
New-Item -ItemType Directory -Force $resources | Out-Null
Copy-Item -LiteralPath (($binary -join "`n").Trim()) -Destination (Join-Path $resources 'devbox-linux') -Force
$version = (Get-Content -LiteralPath (Join-Path $target 'app\package.json') -Raw | ConvertFrom-Json).version
$linuxVersion = @($metadata.packages | Where-Object { $_.name -eq 'devbox-cli' })
if ($linuxVersion.Count -ne 1 -or $linuxVersion[0].version -ne $version) { throw 'Built CLI and app versions differ' }
[IO.File]::WriteAllText((Join-Path $resources 'devbox-version.txt'), $linuxVersion[0].version, (New-Object Text.UTF8Encoding $false))
& "$target\scripts\win-package.ps1" -Mode $Mode
