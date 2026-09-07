[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Config,
  [Parameter(Mandatory = $true)][string]$BaselineAssets,
  [Parameter(Mandatory = $true)][string]$BaselineMetadata,
  [Parameter(Mandatory = $true)][string]$CandidateAssets,
  [Parameter(Mandatory = $true)][string]$CandidateMetadata,
  [Parameter(Mandatory = $true)][string]$CandidateTag,
  [Parameter(Mandatory = $true)][string]$CandidateCommit,
  [Parameter(Mandatory = $true)][string]$Output,
  [Parameter(Mandatory = $true)][string]$ScratchRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. "$PSScriptRoot/windows-installer-helpers.ps1"

$report = [ordered]@{
  schemaVersion = 1
  status = 'RUNNING'
  candidateTag = $CandidateTag
  candidateCommit = $CandidateCommit
  startedAt = [DateTime]::UtcNow.ToString('o')
  completedAt = $null
  host = [ordered]@{
    runnerImage = $env:ImageOS
    os = [Environment]::OSVersion.VersionString
    powershell = $PSVersionTable.PSVersion.ToString()
  }
  releases = $null
  apps = @()
  cleanup = [ordered]@{
    uninstallResidue = 0
    registryKeyResidue = 0
    installDirectoryResidue = 0
    markerResidue = 0
    appDataDirectoryResidue = 0
    integrationDirectoryResidue = 0
    failures = @()
  }
  scope = 'installer artifact and lifecycle subset of W4-B'
  limitations = @(
    'the apps are not launched, so real v0.4.1 data-schema migration is not covered',
    'marker preservation proves installer non-deletion but is not a substitute for application data migration',
    'locked-file and ACL-denied fault injection are not covered by this initial lifecycle matrix',
    'real low-disk injection requires a dedicated bounded volume and is not covered by this hosted-runner matrix',
    'per-machine UAC behavior is not covered by the current-user hosted-runner matrix'
  )
  failures = @()
}

$allMarkers = @()
$exitCode = 0
$outputSafe = $false
try {
  if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    Fail 'installer acceptance is restricted to a GitHub-hosted disposable runner'
  }
  if ($CandidateTag -notmatch '^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$' -or $CandidateCommit -notmatch '^[0-9a-f]{40}$') {
    Fail 'candidate release identity is invalid'
  }
  foreach ($path in @($Config, $BaselineAssets, $BaselineMetadata, $CandidateAssets, $CandidateMetadata, $ScratchRoot)) {
    if (-not (Test-Path -LiteralPath $path)) { Fail 'required acceptance input is missing' }
  }
  if (Test-Path -LiteralPath $Output) { Fail 'refusing to overwrite acceptance evidence' }
  Assert-Descendant $ScratchRoot $env:RUNNER_TEMP 'scratch root'
  Assert-Descendant $Output $ScratchRoot 'acceptance output'
  $outputSafe = $true

  $configuration = Read-Json $Config
  if ($configuration.schemaVersion -ne 1 -or @($configuration.apps).Count -ne 15) {
    Fail 'installer acceptance config is invalid'
  }
  $baselineApps = @($configuration.apps | Where-Object { [bool]$_.baseline })
  if ($baselineApps.Count -ne 15 -or $baselineApps.Count -ne @($configuration.apps).Count) {
    Fail 'baseline app count mismatch'
  }
  if ($configuration.baseline.tag -notmatch '^v\d+\.\d+\.\d+$' -or $configuration.baseline.commit -notmatch '^[0-9a-f]{40}$') {
    Fail 'baseline release identity is invalid'
  }

  $protectedNames = @($configuration.apps | ForEach-Object {
    @($_.binaryName, "$($_.productName).exe")
  } | Select-Object -Unique)
  $preexistingProcesses = @(Get-CimInstance Win32_Process | Where-Object { $protectedNames -contains $_.Name })
  if ($preexistingProcesses.Count -ne 0) { Fail 'pre-existing Devbox process detected' }
  $sharedDataRoot = Join-Path $env:LOCALAPPDATA 'devbox'
  if (Test-Path -LiteralPath $sharedDataRoot) { Fail 'pre-existing Devbox integration data detected' }
  foreach ($app in $configuration.apps) {
    if ($null -ne (Find-App-Entry $app)) { Fail 'pre-existing Devbox installation detected' }
    if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $app.productName)) {
      Fail 'pre-existing Devbox install directory detected'
    }
    if ((Get-Potential-Shortcut-Count $app) -ne 0) { Fail 'pre-existing Devbox shortcut detected' }
    foreach ($identifier in @($app.identifier) + @($app.legacyIdentifiers)) {
      if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $identifier)) {
        Fail 'pre-existing Devbox app-data directory detected'
      }
    }
  }

  $baselineRelease = Verify-Release $BaselineAssets $BaselineMetadata $configuration.baseline.tag $configuration.baseline.commit $baselineApps.Count $false $baselineApps
  $candidateIsPrerelease = $CandidateTag.Contains('-')
  $candidateRelease = Verify-Release $CandidateAssets $CandidateMetadata $CandidateTag $CandidateCommit 15 $candidateIsPrerelease @($configuration.apps)
  $report.releases = [ordered]@{
    baseline = [ordered]@{ tag = $baselineRelease.tag; commit = $baselineRelease.commit; assets = $baselineRelease.assets; manifestSha256 = $baselineRelease.manifestSha256; metadataSha256 = $baselineRelease.metadataSha256 }
    candidate = [ordered]@{ tag = $candidateRelease.tag; commit = $candidateRelease.commit; assets = $candidateRelease.assets; manifestSha256 = $candidateRelease.manifestSha256; metadataSha256 = $candidateRelease.metadataSha256 }
  }

  $runId = "$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
  foreach ($app in $configuration.apps) {
    $baselineVersion = if ([bool]$app.baseline) {
      [string]$baselineRelease.byId[$app.id].version
    } else {
      $null
    }
    $candidateVersion = [string]$candidateRelease.byId[$app.id].version
    $versionChanged = [bool]$app.baseline -and $baselineVersion -cne $candidateVersion
    $lifecycle = if (-not [bool]$app.baseline) {
      'new-app'
    } elseif ($versionChanged) {
      'version-change'
    } else {
      'same-version'
    }
    $appResult = [ordered]@{
      id = $app.id
      baseline = [bool]$app.baseline
      baselineVersion = $baselineVersion
      candidateVersion = $candidateVersion
      versionChanged = $versionChanged
      lifecycle = $lifecycle
      status = 'RUNNING'
      phases = @()
      markers = @()
      failure = $null
    }
    try {
      if ([bool]$app.baseline -and $versionChanged) {
        $baselineInstall = Install-App $app $baselineRelease $BaselineAssets 'install'
        $appResult.phases += $baselineInstall
        $baselineBinarySha256 = [string]$baselineInstall.binarySha256
        $markers = @(New-Markers $app $runId)
        $allMarkers += $markers
        $appResult.markers = @($markers | ForEach-Object { [ordered]@{ identifier = $_.Identifier; sha256 = $_.Sha256 } })

        $candidateUpdate = Install-App $app $candidateRelease $CandidateAssets 'update'
        $appResult.phases += $candidateUpdate
        $candidateBinarySha256 = [string]$candidateUpdate.binarySha256
        if ($candidateBinarySha256 -ceq $baselineBinarySha256) {
          Fail 'candidate update did not replace the baseline executable'
        }
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers

        $appResult.phases += Install-App $app $candidateRelease $CandidateAssets 'install' '' $candidateBinarySha256
        Assert-Markers $markers
        $appResult.phases += Install-App $app $baselineRelease $BaselineAssets 'update' '' $baselineBinarySha256
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers
      } elseif ([bool]$app.baseline) {
        $baselineInstall = Install-App $app $baselineRelease $BaselineAssets 'install'
        $appResult.phases += $baselineInstall
        $markers = @(New-Markers $app $runId)
        $allMarkers += $markers
        $appResult.markers = @($markers | ForEach-Object { [ordered]@{ identifier = $_.Identifier; sha256 = $_.Sha256 } })
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers

        $candidateInstall = Install-App $app $candidateRelease $CandidateAssets 'install'
        $appResult.phases += $candidateInstall
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers
      } else {
        $customDir = Join-Path $ScratchRoot "custom-install\$($app.id)"
        $candidateCustom = Install-App $app $candidateRelease $CandidateAssets 'custom' $customDir
        $appResult.phases += $candidateCustom
        $candidateBinarySha256 = [string]$candidateCustom.binarySha256
        $markers = @(New-Markers $app $runId)
        $allMarkers += $markers
        $appResult.markers = @($markers | ForEach-Object { [ordered]@{ identifier = $_.Identifier; sha256 = $_.Sha256 } })
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers

        $appResult.phases += Install-App $app $candidateRelease $CandidateAssets 'install' '' $candidateBinarySha256
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers
      }
      Remove-Markers $markers
      $allMarkers = @($allMarkers | Where-Object { $_.Path -notin $markers.Path })
      $appResult.status = 'PASS'
    } catch {
      $appResult.status = 'FAIL'
      $appResult.failure = Public-Error $_
      throw
    } finally {
      $report.apps += $appResult
      if ($outputSafe) { Write-Report $report $Output }
    }
  }
  $report.status = 'PASS'
} catch {
  $report.status = 'FAIL'
  $report.failures += Public-Error $_
  $exitCode = 1
} finally {
  foreach ($appId in @($script:ownedInstalls.Keys)) {
    try {
      $app = @($configuration.apps | Where-Object { $_.id -eq $appId })[0]
      $state = $script:ownedInstalls[$appId]
      Uninstall-App $app $state | Out-Null
    } catch {
      $report.cleanup.failures += "uninstall cleanup failed: $(Public-Error $_)"
    }
  }
  foreach ($marker in @($allMarkers)) {
    try { Remove-Markers @($marker) } catch { $report.cleanup.failures += "marker cleanup failed: $(Public-Error $_)" }
  }
  try {
    if (Get-Variable configuration -ErrorAction SilentlyContinue) {
      foreach ($app in $configuration.apps) {
        if ($null -ne (Find-App-Entry $app)) { $report.cleanup.uninstallResidue += 1 }
      }
    }
    if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'devbox')) {
      $report.cleanup.integrationDirectoryResidue = 1
    }
    foreach ($marker in @($allMarkers)) {
      if (Test-Path -LiteralPath $marker.Path) { $report.cleanup.markerResidue += 1 }
    }
    foreach ($directory in $script:observedInstallDirs) {
      if (Test-Path -LiteralPath $directory) { $report.cleanup.installDirectoryResidue += 1 }
    }
    foreach ($registryKey in $script:observedRegistryKeys) {
      if (Test-Path -LiteralPath $registryKey) { $report.cleanup.registryKeyResidue += 1 }
    }
    if (Get-Variable configuration -ErrorAction SilentlyContinue) {
      foreach ($identifier in @($configuration.apps | ForEach-Object { @($_.identifier) + @($_.legacyIdentifiers) }) | Select-Object -Unique) {
        if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $identifier)) {
          $report.cleanup.appDataDirectoryResidue += 1
        }
      }
    }
  } catch {
    $report.cleanup.failures += "cleanup read-back failed: $(Public-Error $_)"
  }
  if (
    $report.cleanup.uninstallResidue -ne 0 -or
    $report.cleanup.registryKeyResidue -ne 0 -or
    $report.cleanup.installDirectoryResidue -ne 0 -or
    $report.cleanup.markerResidue -ne 0 -or
    $report.cleanup.appDataDirectoryResidue -ne 0 -or
    $report.cleanup.integrationDirectoryResidue -ne 0 -or
    $report.cleanup.failures.Count -ne 0
  ) {
    $report.status = 'FAIL'
    $exitCode = 1
  }
  $report.completedAt = [DateTime]::UtcNow.ToString('o')
  if ($outputSafe) { Write-Report $report $Output }
}

exit $exitCode
