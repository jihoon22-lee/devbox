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
if ($LASTEXITCODE -ne 0 -or $payload.sourceSha -ne $source) { throw 'Suite payload is not this source.' }
$scratch = Join-Path $env:RUNNER_TEMP ('devbox-suite-delivery-' + [guid]::NewGuid().ToString('N'))
$install = Join-Path $scratch 'Suite Custom Directory'
New-Item -ItemType Directory -Path $scratch | Out-Null
$evidence = [ordered]@{ sourceSha=$source; scope='installation-data-restore-removal'; result='failed'; checks=[ordered]@{} }
$key = $null
$registration = $null
$dataRoots = @()
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
  Wait-Until { -not (Test-Path -LiteralPath $arp) }
  Require (-not (Test-Path -LiteralPath $arp)) 'suiteRegistrationRemoved'
  Require (@(Get-ChildItem -LiteralPath $registration.shortcutDirectory -Filter '*.lnk' -File).Count -eq 0) 'ownedShortcutsRemoved'
  Require ((Get-Content -Raw -LiteralPath $unknown) -eq 'synthetic unlisted file') 'unlistedFilePreserved'
  foreach ($data in $dataRoots) { Require ((Get-FileHash -LiteralPath (Join-Path $data 'fixture-preserve.json')).Hash -eq $before[$data]) "userDataPreserved_$([IO.Path]::GetFileName($data).Split('.')[3])" }
  Require (@(Get-ChildItem -LiteralPath $install -Recurse -Filter 'devbox-*.exe' -File).Count -eq 0) 'ownedProductAndHelperBinariesRemoved'
  $evidence.result = 'passed'
} finally {
  # These exact namespaces were created by this fixture's random physical install.
  # Cleanup never scans unrelated app data, installations, WSL or host services.
  if ($key -and $key -match '^[0-9a-f]{64}$') {
    foreach ($data in $dataRoots) { if (Test-Path -LiteralPath $data) { Remove-Item -LiteralPath $data -Recurse -Force } }
    foreach ($prefix in @('com.devbox.v08.suite-backups.i','com.devbox.v08.suite-restore.i')) {
      $owned = Join-Path $env:LOCALAPPDATA "$prefix$key"
      if (Test-Path -LiteralPath $owned) { Remove-Item -LiteralPath $owned -Recurse -Force }
    }
    $arp = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DevboxSuite.$key"
    if (Test-Path -LiteralPath $arp) {
      $entry = Get-ItemProperty -LiteralPath $arp
      if ($entry.DevboxInstallationKey -eq $key -and $entry.InstallLocation -eq $install) { Remove-Item -LiteralPath $arp -Recurse }
    }
    if ($registration -and (Test-Path -LiteralPath $registration.shortcutDirectory)) {
      Remove-Item -LiteralPath $registration.shortcutDirectory -Recurse -Force
    }
  }
  Remove-Item -LiteralPath $scratch -Recurse -Force
  New-Item -ItemType Directory -Path product-foundation-evidence -Force | Out-Null
  $evidence | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 product-foundation-evidence/suite-delivery.json
}
