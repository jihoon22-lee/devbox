param([Parameter(Mandatory=$true)][string]$Staging, [Parameter(Mandatory=$true)][string]$Bootstrap)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
  throw 'Suite delivery fixtures require disposable GitHub-hosted Windows.'
}
$Staging = (Resolve-Path -LiteralPath $Staging).Path
$Bootstrap = (Resolve-Path -LiteralPath $Bootstrap).Path
$payloadPath = Join-Path $Staging 'suite-payload.json'
$payload = Get-Content -Raw -LiteralPath $payloadPath | ConvertFrom-Json
$source = (git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Fixture source unavailable.' }
$artifactSource = if ($env:DEVBOX_SUITE_ARTIFACT_SOURCE) { $env:DEVBOX_SUITE_ARTIFACT_SOURCE } else { $source }
if ($artifactSource -notmatch '^[0-9a-f]{40}$' -or $payload.sourceSha -ne $artifactSource) { throw 'Suite payload does not match its original artifact source.' }
$scratch = Join-Path $env:RUNNER_TEMP ('devbox-suite-delivery-' + [guid]::NewGuid().ToString('N'))
$install = Join-Path $scratch 'Suite Custom Directory'
New-Item -ItemType Directory -Path $scratch | Out-Null
$evidence = [ordered]@{ sourceSha=$artifactSource; fixtureSourceSha=$source; artifactRun=$env:DEVBOX_SUITE_ARTIFACT_RUN; scope='installed-activation-source-cutover-generation-update-reinstall-data-restore-removal'; result='failed'; checks=[ordered]@{} }
$key = $null
$registration = $null
$dataRoots = @()
$installations = @()
$legacyRoot = $null
$legacyMarker = $null
function Require([bool]$Condition, [string]$Check) {
  if (-not $Condition) { throw "Suite fixture failed: $Check" }
  $evidence.checks[$Check] = $true
}
function Helper([string[]]$Arguments, [string]$ExpectedFailure = '') {
  $output = & $Bootstrap @Arguments 2>&1
  $code = $LASTEXITCODE
  $text = ($output | Out-String).Trim()
  if ($ExpectedFailure) {
    Require ($code -ne 0 -and $text.Contains($ExpectedFailure)) $ExpectedFailure
    return
  }
  if ($code -ne 0) { throw "Suite helper failed ($code): $text" }
  return $text | ConvertFrom-Json
}
function Run-Installer([string]$File, [string]$Arguments) {
  $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru
  if (-not $process.WaitForExit(180000)) { throw 'Owned Suite installer timed out.' }
  if ($process.ExitCode -ne 0) { throw "Owned Suite installer failed ($($process.ExitCode))." }
}
function Native([string]$Root, [string]$Mode) {
  node .github/scripts/windows-suite-delivery-native.mjs $Root $Mode
  if ($LASTEXITCODE -ne 0) { throw "Native installed product stage failed: $Mode" }
}
function Wait-Until([scriptblock]$Condition) {
  $timer = [Diagnostics.Stopwatch]::StartNew()
  while (-not (& $Condition)) {
    if ($timer.Elapsed.TotalSeconds -gt 60) { throw 'Owned Suite removal did not finish.' }
    Start-Sleep -Milliseconds 250
  }
}
try {
  Run-Installer (Join-Path $Staging "Devbox_$($payload.suiteVersion)_x64-setup.exe") "/S /D=$install"
  $registration = Get-Content -Raw -LiteralPath (Join-Path $install 'suite-registration.json') | ConvertFrom-Json
  $key = $registration.installationKey
  $installations += @{ root=$install; key=$key; registration=$registration }
  Require ($key -match '^[0-9a-f]{64}$') 'physicalInstallationKey'
  $arp = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DevboxSuite.$key"
  $entry = Get-ItemProperty -LiteralPath $arp
  Require ($entry.DevboxInstallationKey -eq $key -and $entry.DisplayVersion -eq $payload.suiteVersion) 'oneOwnedSuiteRegistration'
  $links = @(Get-ChildItem -LiteralPath $registration.shortcutDirectory -Filter '*.lnk' -File)
  Require ($links.Count -eq 4) 'fourProductShortcuts'
  $shell = New-Object -ComObject WScript.Shell
  foreach ($link in $links) {
    $resolved = $shell.CreateShortcut($link.FullName)
    Require ($resolved.TargetPath.EndsWith('devbox-suite-bootstrap.exe') -and $resolved.Arguments.Contains($install)) "ownedShortcut_$($link.BaseName.Replace(' ','_'))"
  }
  $marker = Get-Content -Raw -LiteralPath (Join-Path $install 'devbox-activation.json') | ConvertFrom-Json
  Require ($marker.phase -eq 'import') 'registrationDoesNotCommitActivation'
  $catalog = Get-Content -Raw apps/products.json | ConvertFrom-Json
  foreach ($product in $catalog.products) {
    $data = Join-Path $env:LOCALAPPDATA "$($product.identifier).i$key"
    if ($product.id -ne 'control-center') { Require (-not (Test-Path -LiteralPath $data)) "freshNamespace_$($product.id)" }
    New-Item -ItemType Directory -Path $data -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $data 'fixture-preserve.json'), '{"synthetic":"before"}')
    $dataRoots += $data
  }
  Native $install 'import'
  $activated = Helper @('--activate-clean-install',$install,$payloadPath)
  Require ($activated.state -eq 'nativeHealthRequired') 'cleanActivationNeedsNativeHealth'
  Helper @('--commit-clean-install',$install,$payloadPath) 'suite_health_required'
  Native $install 'health'
  $committed = Helper @('--commit-clean-install',$install,$payloadPath)
  Require ($committed.state -eq 'cleanInstallationCommitted') 'cleanInstallCommittedAfterFourNativeOwners'
  $snapshot = Helper @('--snapshot-install',$install,$payloadPath)
  Require ($snapshot.state -eq 'dataCheckpointPreserved') 'closedSnapshotRecorded'
  $workspace = Join-Path $env:LOCALAPPDATA "com.devbox.v08.workspace.i$key"
  [IO.File]::WriteAllText((Join-Path $workspace 'fixture-preserve.json'), '{"synthetic":"newer"}')
  $review = Helper @('--prepare-data-restore',$install,$payloadPath,$snapshot.checkpoint.id)
  Require ($review.state -eq 'dataRestorePrepared') 'restoreReviewPreservesNewerData'
  $applied = Helper @('--apply-data-restore',$install,$payloadPath,$review.operationId)
  Require ($applied.state -eq 'dataRestoreHealthRequired') 'restoredDataRequiresHealth'
  Require ((Get-Content -Raw -LiteralPath (Join-Path $workspace 'fixture-preserve.json')) -eq '{"synthetic":"before"}') 'selectedCheckpointApplied'
  Helper @('--commit-data-restore',$install,$payloadPath,$review.operationId) 'suite_health_required'
  $rolledBack = Helper @('--rollback-data-restore',$install,$payloadPath,$review.operationId)
  Require ($rolledBack.state -eq 'dataRestoreRolledBack') 'uncommittedRestoreCanUndo'
  Require ((Get-Content -Raw -LiteralPath (Join-Path $workspace 'fixture-preserve.json')) -eq '{"synthetic":"newer"}') 'newerOriginalPreserved'
  $unknown = Join-Path $install 'fixture-user-file.txt'
  [IO.File]::WriteAllText($unknown, 'synthetic unlisted file')
  $before = @{}
  foreach ($data in $dataRoots) { $before[$data] = (Get-FileHash -LiteralPath (Join-Path $data 'fixture-preserve.json')).Hash }
  Run-Installer (Join-Path $install 'Uninstall.exe') '/S'
  Wait-Until { Test-Path -LiteralPath (Join-Path $install 'uninstall-complete.json') }
  Require (-not (Test-Path -LiteralPath $arp)) 'suiteRegistrationRemoved'
  Require (@(Get-ChildItem -LiteralPath $registration.shortcutDirectory -Filter '*.lnk' -File).Count -eq 0) 'ownedShortcutsRemoved'
  Require ((Get-Content -Raw -LiteralPath $unknown) -eq 'synthetic unlisted file') 'unlistedFilePreserved'
  foreach ($data in $dataRoots) { Require ((Get-FileHash -LiteralPath (Join-Path $data 'fixture-preserve.json')).Hash -eq $before[$data]) "userDataPreserved_$([IO.Path]::GetFileName($data).Split('.')[3])" }
  Require (@(Get-ChildItem -LiteralPath $install -Recurse -Filter 'devbox-*.exe' -File).Count -eq 0) 'ownedProductAndHelperBinariesRemoved'
  Run-Installer (Join-Path $Staging "Devbox_$($payload.suiteVersion)_x64-setup.exe") "/S /D=$install"
  $reinstalled = Get-Content -Raw -LiteralPath (Join-Path $install 'suite-registration.json') | ConvertFrom-Json
  Require ($reinstalled.installationKey -eq $key) 'reinstallKeepsInstallationAndDataIdentity'
  foreach ($data in $dataRoots) { Require ((Get-FileHash -LiteralPath (Join-Path $data 'fixture-preserve.json')).Hash -eq $before[$data]) "reinstallPreserves_$([IO.Path]::GetFileName($data).Split('.')[3])" }
  Helper @('--commit-reinstall',$install,$payloadPath) 'suite_health_required'
  Native $install 'health'
  $reinstalled = Helper @('--commit-reinstall',$install,$payloadPath)
  Require ($reinstalled.state -eq 'reinstallationCommitted') 'reinstallRequiresFreshNativeHealth'

  # A different private payload encoding uses the same exact four binaries. This
  # exercises generation/self-update recovery without inventing a version build.
  $candidate = Join-Path $scratch 'Candidate Payload'
  New-Item -ItemType Directory -Path $candidate | Out-Null
  foreach ($product in $payload.products) { Copy-Item -LiteralPath (Join-Path $Staging $product.portable.name) -Destination $candidate }
  Copy-Item -LiteralPath $Bootstrap -Destination (Join-Path $candidate 'devbox-suite-bootstrap.exe')
  $nextPayload = Join-Path $candidate 'suite-payload.json'
  [IO.File]::WriteAllText($nextPayload, ($payload | ConvertTo-Json -Depth 30 -Compress) + "`n ")
  Require ((Get-FileHash $nextPayload).Hash -ne (Get-FileHash $payloadPath).Hash) 'distinctReviewedGenerationPayload'
  $previousManifest = [IO.File]::ReadAllText((Join-Path $install 'devbox-installation.json'))
  $update = Helper @('--prepare-update',$install,$nextPayload)
  Helper @('--apply-update',$install,$nextPayload,$update.operationId) | Out-Null
  Helper @('--commit-update',$install,$nextPayload,$update.operationId) 'suite_health_required'
  Helper @('--rollback-update',$install,$nextPayload,$update.operationId) | Out-Null
  Require ([IO.File]::ReadAllText((Join-Path $install 'devbox-installation.json')) -eq $previousManifest) 'updateRollbackRestoresExactPackageSelection'
  foreach ($data in $dataRoots) { Require ((Get-FileHash -LiteralPath (Join-Path $data 'fixture-preserve.json')).Hash -eq $before[$data]) "updateRollbackPreserves_$([IO.Path]::GetFileName($data).Split('.')[3])" }
  $update = Helper @('--prepare-update',$install,$nextPayload)
  Helper @('--apply-update',$install,$nextPayload,$update.operationId) | Out-Null
  Native $install 'health'
  Helper @('--commit-update',$install,$nextPayload,$update.operationId) | Out-Null
  Native $install 'committed'
  Require ([IO.File]::ReadAllText((Join-Path $install 'devbox-installation.json')) -ne $previousManifest) 'generationUpdateCommitted'
  # This original NSIS uninstaller must delegate to the retained current helper.
  Run-Installer (Join-Path $install 'Uninstall.exe') '/S'
  Wait-Until { Test-Path -LiteralPath (Join-Path $install 'uninstall-complete.json') }
  Require (-not (Test-Path -LiteralPath $arp)) 'originalUninstallerRemovesUpdatedGeneration'
  foreach ($data in $dataRoots) { Require ((Get-FileHash -LiteralPath (Join-Path $data 'fixture-preserve.json')).Hash -eq $before[$data]) "updatedUninstallPreserves_$([IO.Path]::GetFileName($data).Split('.')[3])" }

  $legacyCandidate = Join-Path $env:LOCALAPPDATA 'com.devbox.devboxlauncher'
  Require (-not (Test-Path -LiteralPath $legacyCandidate)) 'legacyFixtureRequiresAbsentOriginal'
  New-Item -ItemType Directory -Path $legacyCandidate | Out-Null
  $legacyRoot = $legacyCandidate
  $legacyMarker = [guid]::NewGuid().ToString()
  [IO.File]::WriteAllText((Join-Path $legacyRoot 'suite-fixture-owner'), $legacyMarker)
  $legacyPreferences = Join-Path $legacyRoot 'launcher-preferences.json'
  [IO.File]::WriteAllText($legacyPreferences, '{"version":1,"favorites":[],"recents":[]}')
  $migrationInstall = Join-Path $scratch 'Suite Migration Directory'
  Run-Installer (Join-Path $Staging "Devbox_$($payload.suiteVersion)_x64-setup.exe") "/S /D=$migrationInstall"
  $migrationRegistration = Get-Content -Raw -LiteralPath (Join-Path $migrationInstall 'suite-registration.json') | ConvertFrom-Json
  $installations += @{ root=$migrationInstall; key=$migrationRegistration.installationKey; registration=$migrationRegistration }
  Native $migrationInstall 'legacyImport'
  Helper @('--activate-clean-install',$migrationInstall,$payloadPath) 'bootstrap_source_cutover_required'
  # Mutating the owned synthetic original after review must invalidate cutover.
  [IO.File]::WriteAllText($legacyPreferences, '{"version":1,"favorites":[],"recents":[]} ')
  Helper @('--activate-reviewed-install',$migrationInstall,$payloadPath) 'cutover_source_changed'
  Native $migrationInstall 'legacyReview'
  Helper @('--activate-reviewed-install',$migrationInstall,$payloadPath) | Out-Null
  Native $migrationInstall 'health'
  Helper @('--commit-reviewed-install',$migrationInstall,$payloadPath) | Out-Null
  Require ([IO.File]::ReadAllText($legacyPreferences).EndsWith(' ')) 'explicitlySkippedChangedOriginalPreserved'
  Run-Installer (Join-Path $migrationInstall 'Uninstall.exe') '/S'
  Wait-Until { Test-Path -LiteralPath (Join-Path $migrationInstall 'uninstall-complete.json') }
  $evidence.result = 'passed'
} catch {
  $evidence.failure = $_.Exception.Message
  throw
} finally {
  $cleanupFailures = [Collections.Generic.List[string]]::new()
  foreach ($ownedInstall in $installations) {
    $ownedKey = $ownedInstall.key
    if ($ownedKey -notmatch '^[0-9a-f]{64}$') { $cleanupFailures.Add('invalid captured fixture key'); continue }
    foreach ($product in $payload.products) {
      $definition = $catalog.products | Where-Object id -eq $product.id
      $ownedData = Join-Path $env:LOCALAPPDATA "$($definition.identifier).i$ownedKey"
      try { if (Test-Path -LiteralPath $ownedData) { Remove-Item -LiteralPath $ownedData -Recurse -Force } } catch { $cleanupFailures.Add($_.Exception.Message) }
    }
    foreach ($prefix in @('com.devbox.v08.suite-backups.i','com.devbox.v08.suite-restore.i','com.devbox.v08.suite-updates.i')) {
      $ownedData = Join-Path $env:LOCALAPPDATA "$prefix$ownedKey"
      try { if (Test-Path -LiteralPath $ownedData) { Remove-Item -LiteralPath $ownedData -Recurse -Force } } catch { $cleanupFailures.Add($_.Exception.Message) }
    }
    $ownedArp = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DevboxSuite.$ownedKey"
    try {
      if (Test-Path -LiteralPath $ownedArp) {
        $entry = Get-ItemProperty -LiteralPath $ownedArp
        if ($entry.DevboxInstallationKey -ne $ownedKey -or $entry.InstallLocation -ne $ownedInstall.root) { throw 'Fixture registration identity changed' }
        Remove-Item -LiteralPath $ownedArp -Recurse
      }
      $shortcuts = $ownedInstall.registration.shortcutDirectory
      if (Test-Path -LiteralPath $shortcuts) { Remove-Item -LiteralPath $shortcuts -Recurse -Force }
    } catch { $cleanupFailures.Add($_.Exception.Message) }
  }
  if ($legacyRoot) {
    try {
      if ([IO.File]::ReadAllText((Join-Path $legacyRoot 'suite-fixture-owner')) -ne $legacyMarker) { throw 'Legacy fixture ownership changed' }
      Remove-Item -LiteralPath $legacyRoot -Recurse -Force
    } catch { $cleanupFailures.Add($_.Exception.Message) }
  }
  try { Remove-Item -LiteralPath $scratch -Recurse -Force } catch { $cleanupFailures.Add($_.Exception.Message) }
  $evidence.cleanupFailures = @($cleanupFailures)
  if ($cleanupFailures.Count -gt 0) { $evidence.result = 'failed' }
  New-Item -ItemType Directory -Path product-foundation-evidence -Force | Out-Null
  $evidence | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 product-foundation-evidence/suite-delivery.json
  if ($cleanupFailures.Count -gt 0 -and -not $evidence.failure) { throw 'Suite fixture cleanup incomplete; see retained evidence.' }
}
