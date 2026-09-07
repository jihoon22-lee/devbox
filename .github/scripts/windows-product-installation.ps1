[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
  throw 'Product installation proof requires a disposable GitHub-hosted Windows runner'
}
. "$PSScriptRoot/windows-installer-helpers.ps1"

$ScratchRoot = Join-Path $env:RUNNER_TEMP ('devbox-product-installation-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($ScratchRoot) | Out-Null
$baselineAssets = Join-Path $ScratchRoot 'baseline'
[IO.Directory]::CreateDirectory($baselineAssets) | Out-Null
$output = Join-Path (Get-Location) 'product-foundation-evidence/installation.json'
if (Test-Path -LiteralPath $output) { Fail 'refusing to overwrite installation evidence' }
[IO.Directory]::CreateDirectory((Split-Path -Parent $output)) | Out-Null
$baseline = Read-Json "$PSScriptRoot/product-foundation-baseline.json"
$configuration = Read-Json "$PSScriptRoot/windows-installer-acceptance-config.json"
$report = [ordered]@{ schemaVersion = 1; source = $env:GITHUB_SHA; baseline = $baseline.tag; baselineCommit = $baseline.commit; environment = 'github-hosted-windows'; result = 'failed'; products = @(); failure = $null }
$allMarkers = @()
$appDefinitions = @{}

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ProductFixtureWindow {
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);
}
'@

function Start-Fixture-Window([string]$Binary) {
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $process = Start-Process -FilePath $Binary -PassThru
  while ($watch.Elapsed.TotalSeconds -lt 30) {
    $process.Refresh()
    if ($process.HasExited) { Fail 'installed product exited before its window opened' }
    if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
      return [pscustomobject]@{ Process = $process; FirstWindowMs = $watch.ElapsedMilliseconds }
    }
    Start-Sleep -Milliseconds 100
  }
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  Fail 'installed product window deadline exceeded'
}

function Stop-Fixture-Window([object]$Window) {
  if ($null -eq $Window) { return }
  $process = $Window.Process
  $process.Refresh()
  if (-not $process.HasExited) {
    [void]$process.CloseMainWindow()
    if (-not $process.WaitForExit(5000)) {
      & taskkill.exe /PID $process.Id /T /F | Out-Null
    }
  }
}

try {
  $tagRef = (& gh api "repos/jihoon22-lee/devbox/git/ref/tags/$($baseline.tag)" | ConvertFrom-Json)
  if ($LASTEXITCODE -ne 0) { Fail 'baseline tag lookup failed' }
  $tagObject = $tagRef.object
  for ($depth = 0; $tagObject.type -eq 'tag' -and $depth -lt 3; $depth += 1) {
    $tag = (& gh api "repos/jihoon22-lee/devbox/git/tags/$($tagObject.sha)" | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { Fail 'baseline annotated tag lookup failed' }
    $tagObject = $tag.object
  }
  if ($tagObject.type -ne 'commit' -or $tagObject.sha -cne $baseline.commit) { Fail 'baseline tag commit mismatch' }
  & gh release download $baseline.tag --repo jihoon22-lee/devbox --pattern release-manifest.json --dir $baselineAssets
  if ($LASTEXITCODE -ne 0) { Fail 'baseline manifest download failed' }
  $manifestPath = Join-Path $baselineAssets 'release-manifest.json'
  if ((Sha256 $manifestPath) -cne $baseline.manifestSha256) { Fail 'baseline manifest digest mismatch' }
  $manifest = Read-Json $manifestPath
  if ($manifest.schemaVersion -ne 1 -or $manifest.releaseTag -cne $baseline.tag -or @($manifest.apps).Count -ne 15) { Fail 'baseline manifest identity mismatch' }
  $baselineRelease = [pscustomobject]@{ byId = @{}; notices = $manifest.notices }
  foreach ($app in $manifest.apps) { $baselineRelease.byId[$app.id] = $app }
  foreach ($pair in $baseline.anchors.PSObject.Properties) {
    $product = $pair.Name; $anchorId = [string]$pair.Value
    $anchor = @($configuration.apps | Where-Object { $_.id -ceq $anchorId })[0]
    $config = Read-Json (Join-Path (Get-Location) "apps/devbox-$product/src-tauri/tauri.conf.json")
    $candidate = [pscustomobject]@{ id = "devbox-$product"; productName = $config.productName; binaryName = "devbox-$product.exe"; identifier = $config.identifier; legacyIdentifiers = @() }
    foreach ($app in @($anchor, $candidate)) {
      $appDefinitions[$app.id] = $app
      if ($null -ne (Find-App-Entry $app) -or (Get-Potential-Shortcut-Count $app) -ne 0) { Fail 'pre-existing anchor/product installation or shortcut detected' }
    }
    if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $anchor.identifier)) { Fail 'pre-existing anchor data detected' }
    $asset = $baselineRelease.byId[$anchor.id].installer
    Assert-Safe-Leaf $asset.name
    & gh release download $baseline.tag --repo jihoon22-lee/devbox --pattern $asset.name --dir $baselineAssets
    if ($LASTEXITCODE -ne 0) { Fail 'baseline installer download failed' }
    $installer = Join-Path $baselineAssets $asset.name
    if ((Sha256 $installer) -cne $asset.sha256 -or (Get-Item -LiteralPath $installer).Length -ne $asset.size) { Fail 'baseline installer identity mismatch' }
    $candidateAssets = Join-Path (Get-Location) 'target/debug/bundle/nsis'
    $installers = @(Get-ChildItem -LiteralPath $candidateAssets -File | Where-Object { $_.Name -like "$($config.productName)_$($config.version)_*-setup.exe" })
    if ($installers.Count -ne 1) { Fail 'hidden installer is missing or ambiguous' }
    $candidateRelease = [pscustomobject]@{ byId = @{}; notices = [pscustomobject]@{ sha256 = Sha256 (Join-Path (Get-Location) 'THIRD_PARTY_NOTICES.md') } }
    $candidateRelease.byId[$candidate.id] = [pscustomobject]@{ version = $config.version; installer = [pscustomobject]@{ name = $installers[0].Name } }
    $anchorWindow = $null; $productWindow = $null
    try {
      $anchorInstall = Install-App $anchor $baselineRelease $baselineAssets 'custom' (Join-Path $ScratchRoot $anchor.id)
      $markers = @(New-Markers $anchor ([guid]::NewGuid().ToString('N')))
      $allMarkers += $markers
      $anchorState = $script:ownedInstalls[$anchor.id]
      $productInstall = Install-App $candidate $candidateRelease $candidateAssets 'custom' (Join-Path $ScratchRoot $candidate.id)
      $productState = $script:ownedInstalls[$candidate.id]
      if ($anchorState.ProviderPath -ieq $productState.ProviderPath -or $anchorState.InstallDir -ieq $productState.InstallDir) { Fail 'anchor and product installer identities collide' }
      $beforeData = @(Get-ChildItem -LiteralPath $env:LOCALAPPDATA -Directory -Filter "$($candidate.identifier).i*" | ForEach-Object { $_.FullName })
      $anchorWindow = Start-Fixture-Window $anchorState.Binary
      $productWindow = Start-Fixture-Window $productState.Binary
      if ($anchorWindow.Process.HasExited -or $productWindow.Process.HasExited) { Fail 'anchor and product mutex identities collide' }
      if (-not [ProductFixtureWindow]::MoveWindow($productWindow.Process.MainWindowHandle, 80, 80, 1040, 740, $true)) { Fail 'owned window move failed' }
      $newData = @()
      for ($attempt = 0; $attempt -lt 50; $attempt += 1) {
        $newData = @(Get-ChildItem -LiteralPath $env:LOCALAPPDATA -Directory -Filter "$($candidate.identifier).i*" | Where-Object { $beforeData -notcontains $_.FullName -and (Test-Path -LiteralPath (Join-Path $_.FullName 'window-state-v1.json')) })
        if ($newData.Count -eq 1) { break }
        Start-Sleep -Milliseconds 100
      }
      if ($newData.Count -ne 1) { Fail 'installed product did not persist its window in a distinct data namespace' }
      Assert-Markers $markers
      $second = Start-Process -FilePath $productState.Binary -PassThru
      if (-not $second.WaitForExit(10000)) { Stop-Process -Id $second.Id -Force; Fail 'installed product duplicate did not exit' }
      if ($second.ExitCode -ne 0) { Fail 'installed product duplicate failed' }
      $observation = [ordered]@{ product = $product; anchor = $anchor.id; anchorInstall = $anchorInstall; productInstall = $productInstall; simultaneousWindows = $true; dataNamespace = 'separate-v08-location'; legacyMarkersPreserved = $true; anchorFirstWindowMs = $anchorWindow.FirstWindowMs; productFirstWindowMs = $productWindow.FirstWindowMs; timingBoundary = 'process-start-to-native-window; not renderer readiness or R24 acceptance' }
      Stop-Fixture-Window $productWindow; $productWindow = $null
      Stop-Fixture-Window $anchorWindow; $anchorWindow = $null
      $observation.productUninstall = Uninstall-App $candidate $productState
      Assert-Markers $markers
      if ((Sha256 $anchorState.Binary) -cne $anchorState.BinarySha256 -or (Get-App-Shortcut-Count $anchorState.Binary) -ne $anchorState.ShortcutCount -or $null -eq (Find-App-Entry $anchor)) { Fail 'product uninstall changed anchor installation or shortcut' }
      if (-not (Test-Path -LiteralPath (Join-Path $newData[0].FullName 'window-state-v1.json'))) { Fail 'product uninstall deleted v08 data' }
      $observation.anchorUninstall = Uninstall-App $anchor $anchorState
      Assert-Markers $markers
      $report.products += $observation
    } finally {
      Stop-Fixture-Window $productWindow
      Stop-Fixture-Window $anchorWindow
    }
  }
  $report.result = 'pass'
} catch {
  $report.failure = Public-Error $_
  throw
} finally {
  foreach ($id in @($script:ownedInstalls.Keys)) {
    try { Uninstall-App $appDefinitions[$id] $script:ownedInstalls[$id] | Out-Null } catch { $report.result = 'failed'; $report.failure = 'owned installer cleanup failed' }
  }
  Remove-Markers $allMarkers
  Write-Report $report $output
}
if ($report.result -ne 'pass') { Fail 'product installation proof failed' }
