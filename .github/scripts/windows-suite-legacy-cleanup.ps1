param([Parameter(Mandatory=$true)][string]$SuiteRoot)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Legacy cleanup requires disposable hosted Windows.' }
. "$PSScriptRoot/windows-installer-helpers.ps1"
$ScratchRoot = Split-Path -Parent (Resolve-Path -LiteralPath $SuiteRoot).Path
if ([IO.Path]::GetFileName($ScratchRoot) -notlike 'devbox-suite-delivery-*') { throw 'Unexpected fixture root' }
$baseline = Read-Json "$PSScriptRoot/product-foundation-baseline.json"
$configuration = Read-Json "$PSScriptRoot/windows-installer-acceptance-config.json"
$definition = $configuration.apps | Where-Object id -eq 'port-manager'
if ($null -ne (Find-App-Entry $definition)) { throw 'Legacy fixture requires absent installation' }
$manager = Join-Path $env:LOCALAPPDATA 'com.devbox.devboxmanager'
$userData = Join-Path $env:LOCALAPPDATA 'com.devbox.portmanager'
$locator = Join-Path $env:LOCALAPPDATA 'devbox/install-roots/v1/registry.json'
if ((Test-Path -LiteralPath $manager) -or (Test-Path -LiteralPath $userData) -or (Test-Path -LiteralPath $locator)) { throw 'Legacy fixture requires absent Manager/profile/locator' }
$assets = Join-Path $ScratchRoot 'legacy-assets'
New-Item -ItemType Directory -Path $assets | Out-Null
$claimed = $false
$legacyRoot = $null
$marker = [guid]::NewGuid().ToString()
try {
  gh release download $baseline.tag --repo jihoon22-lee/devbox --pattern release-manifest.json --dir $assets
  if ($LASTEXITCODE -ne 0) { throw 'Pinned manifest download failed' }
  $manifestPath = Join-Path $assets 'release-manifest.json'
  if ((Sha256 $manifestPath) -cne $baseline.manifestSha256) { throw 'Pinned manifest changed' }
  $manifest = Read-Json $manifestPath
  $legacy = $manifest.apps | Where-Object id -eq 'port-manager'
  $release = [pscustomobject]@{ byId=@{'port-manager'=$legacy}; notices=$manifest.notices }
  foreach ($asset in @($legacy.installer,$legacy.portable)) {
    Assert-Safe-Leaf $asset.name
    gh release download $baseline.tag --repo jihoon22-lee/devbox --pattern $asset.name --dir $assets
    if ($LASTEXITCODE -ne 0) { throw 'Pinned legacy asset download failed' }
    $file = Join-Path $assets $asset.name
    if ((Get-Item -LiteralPath $file).Length -ne $asset.size -or (Sha256 $file) -cne $asset.sha256) { throw 'Pinned legacy asset changed' }
  }
  $legacyRoot = Join-Path $ScratchRoot 'Legacy Custom Install'
  Install-App $definition $release $assets 'custom' $legacyRoot | Out-Null
  $legacyRoot = $script:ownedInstalls['port-manager'].InstallDir
  [IO.File]::WriteAllText((Join-Path $legacyRoot 'fixture-user-file.txt'), 'preserve unlisted data')
  New-Item -ItemType Directory -Path $manager,$userData | Out-Null
  [IO.File]::WriteAllText((Join-Path $manager 'suite-fixture-owner'), $marker)
  [IO.File]::WriteAllText((Join-Path $userData 'suite-fixture-owner'), $marker)
  $claimed = $true
  $portable = Join-Path $manager "apps/port-manager/versions/$($legacy.version)"
  New-Item -ItemType Directory -Path $portable -Force | Out-Null
  Copy-Item -LiteralPath (Join-Path $assets $legacy.portable.name) -Destination (Join-Path $portable 'port-manager.exe')
  $executable = Join-Path $portable 'port-manager.exe'
  if (-not (Test-Path -LiteralPath $executable)) { throw 'Unexpected legacy portable layout' }
  @(@{app='port-manager';version=$legacy.version;mode='portable';exe_path=$executable}) | ConvertTo-Json -AsArray | Set-Content -Encoding utf8 (Join-Path $manager 'registry.json')
  node .github/scripts/windows-suite-delivery-native.mjs $SuiteRoot cleanup $legacyRoot $executable $marker
  if ($LASTEXITCODE -ne 0) { throw 'Native legacy cleanup failed' }
  if ($null -ne (Find-App-Entry $definition)) { throw 'Legacy ARP cleanup incomplete' }
  if ((Get-Content -Raw (Join-Path $legacyRoot 'fixture-user-file.txt')) -ne 'preserve unlisted data') { throw 'Unlisted legacy file changed' }
  if ((Get-Content -Raw (Join-Path $userData 'suite-fixture-owner')) -ne $marker) { throw 'Legacy user data changed' }
  if (Test-Path -LiteralPath $executable) { throw 'Managed portable still present' }
  if (Test-Path -LiteralPath $locator) { throw 'Embedded cleanup published a foreign default root' }
  [void]$script:ownedInstalls.Remove('port-manager')
} finally {
  # The legacy uninstaller's fixture helper expects its directory to disappear.
  # Retire only our own synthetic preservation marker before fallback cleanup;
  # otherwise it masks the original failure with an expected retained-directory error.
  if ($legacyRoot) {
    $unknown = Join-Path $legacyRoot 'fixture-user-file.txt'
    if ((Test-Path -LiteralPath $unknown) -and [IO.File]::ReadAllText($unknown) -ceq 'preserve unlisted data') { Remove-Item -LiteralPath $unknown }
  }
  foreach ($id in @($script:ownedInstalls.Keys)) { Uninstall-App $definition $script:ownedInstalls[$id] | Out-Null }
  if ($claimed) {
    foreach ($directory in @($manager,$userData)) {
      if ([IO.File]::ReadAllText((Join-Path $directory 'suite-fixture-owner')) -ne $marker) { throw 'Legacy fixture owner changed' }
      Remove-Item -LiteralPath $directory -Recurse -Force
    }
  }
}
