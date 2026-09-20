[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
  throw 'Legacy reference acquisition requires a disposable GitHub-hosted Windows VM'
}
. "$PSScriptRoot/windows-installer-helpers.ps1"
$ScratchRoot = Join-Path $env:RUNNER_TEMP ('devbox-legacy-reference-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($ScratchRoot) | Out-Null
$assets = Join-Path $ScratchRoot 'assets'
[IO.Directory]::CreateDirectory($assets) | Out-Null
$output = Join-Path $env:GITHUB_WORKSPACE 'legacy-installer-reference.json'
if (Test-Path -LiteralPath $output) { Fail 'reference output already exists' }
$baseline = Read-Json "$PSScriptRoot/product-foundation-baseline.json"
$config = Read-Json "$PSScriptRoot/legacy-v0.7-windows-installer-acceptance-config.json"
$report = [ordered]@{schemaVersion=1; baselineTag=$baseline.tag; baselineCommit=$baseline.commit; manifestSha256=$baseline.manifestSha256; acquisitionRun=$env:GITHUB_RUN_ID; acquisitionSource=$env:GITHUB_SHA; environment='github-hosted-windows'; result='failed'; apps=@(); failure=$null}
$definitions = @{}
try {
  & gh release download $baseline.tag --repo jihoon22-lee/devbox --pattern release-manifest.json --dir $assets
  if ($LASTEXITCODE -ne 0) { Fail 'baseline manifest download failed' }
  $manifestPath = Join-Path $assets 'release-manifest.json'
  if ((Sha256 $manifestPath) -cne $baseline.manifestSha256) { Fail 'baseline manifest changed' }
  $manifest = Read-Json $manifestPath
  if ($manifest.schemaVersion -ne 1 -or $manifest.releaseTag -cne 'v0.7.0' -or @($manifest.apps).Count -ne 15) { Fail 'baseline identity mismatch' }
  $release = [pscustomobject]@{byId=@{};notices=$manifest.notices}
  foreach ($item in $manifest.apps) { $release.byId[$item.id]=$item }
  foreach ($definition in $config.apps) {
    $definitions[$definition.id]=$definition
    if ($null -ne (Find-App-Entry $definition)) { Fail 'VM already has a target installer registration' }
    if (@(Get-Process -Name $definition.id -ErrorAction SilentlyContinue).Count -ne 0) { Fail 'VM already has a target process' }
  }
  foreach ($item in $manifest.apps) {
    $definition=$definitions[$item.id]
    if ($null -eq $definition) { Fail 'baseline app lacks a fixed definition' }
    Assert-Safe-Leaf $item.installer.name
    & gh release download $baseline.tag --repo jihoon22-lee/devbox --pattern $item.installer.name --dir $assets
    if ($LASTEXITCODE -ne 0) { Fail 'baseline installer download failed' }
    $installer=Join-Path $assets $item.installer.name
    if ((Get-Item -LiteralPath $installer).Length -ne $item.installer.size -or (Sha256 $installer) -cne $item.installer.sha256) { Fail 'baseline installer changed' }
    $observations=@()
    foreach ($suffix in @('first','second')) {
      $destination=Join-Path $ScratchRoot ($item.id+'-'+$suffix)
      Install-App $definition $release $assets 'custom' $destination | Out-Null
      $owned=$script:ownedInstalls[$item.id]
      $files=@(Get-ChildItem -LiteralPath $owned.InstallDir -File -Recurse | Sort-Object FullName | ForEach-Object {
        Assert-Plain-Existing-Path $_.FullName 'installed reference file'
        $relative=[IO.Path]::GetRelativePath($owned.InstallDir,$_.FullName).Replace('\','/')
        if ($relative.StartsWith('../') -or $relative.Contains(':')) { Fail 'reference file escaped install root' }
        [ordered]@{name=$relative;sha256=(Sha256 $_.FullName);size=$_.Length}
      })
      if ($files.Count -gt 512 -or @($files | Where-Object name -eq 'uninstall.exe').Count -ne 1) { Fail 'reference layout is not bounded' }
      $observations+=,@($files)
      Uninstall-App $definition $owned | Out-Null
    }
    $first=$observations[0] | ConvertTo-Json -Depth 10 -Compress
    $second=$observations[1] | ConvertTo-Json -Depth 10 -Compress
    if ($first -cne $second) { Fail 'installed file identity depends on installation root' }
    $report.apps+=,[ordered]@{id=$item.id;version=$item.version;installerSha256=$item.installer.sha256;files=$observations[0];twoRootsIdentical=$true}
    Write-Host ('Captured immutable installed-file reference: '+$item.id)
  }
  $report.result='passed'
} catch { $report.failure=Public-Error $_ }
finally {
  foreach ($id in @($script:ownedInstalls.Keys)) {
    try { Uninstall-App $definitions[$id] $script:ownedInstalls[$id] | Out-Null } catch { $report.result='failed'; $report.failure='owned reference installer cleanup failed' }
  }
  Write-Report $report $output
}
if ($report.result -ne 'passed') { exit 1 }
