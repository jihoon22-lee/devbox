# L4 provisioning only. This is not evidence for interactive first installation.
param([switch]$Cleanup)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Owned user-flow installation requires disposable hosted Windows.' }
function Run-Owned([string]$File, [string[]]$Arguments, [int]$TimeoutMilliseconds=180000) {
  $start=[Diagnostics.ProcessStartInfo]::new()
  $start.FileName=$File
  $start.UseShellExecute=$false
  $start.WorkingDirectory=(Get-Location).ProviderPath
  foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
  $process=[Diagnostics.Process]::new()
  $process.StartInfo=$start
  try {
    if (-not $process.Start()) { throw 'Owned user-flow provisioning operation did not start.' }
    # GUI executables do not reliably set LASTEXITCODE. Wait for this exact child.
    if (-not $process.WaitForExit($TimeoutMilliseconds)) { throw 'Owned user-flow provisioning operation timed out; preserve fixture rather than force terminate.' }
    if ($process.ExitCode -ne 0) { throw 'Owned user-flow provisioning operation failed.' }
  } finally {
    $process.Dispose()
  }
}
$payloadSource=$env:GITHUB_SHA
$payloadRun=$env:GITHUB_RUN_ID
if ($env:DEVBOX_USER_FLOW_DIAGNOSTIC -or $env:DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE -or $env:DEVBOX_USER_FLOW_DIAGNOSTIC_RUN -or $env:DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT) {
  if ($env:DEVBOX_USER_FLOW_DIAGNOSTIC -cne 'true' -or $env:GITHUB_EVENT_NAME -cne 'workflow_dispatch' -or $env:GITHUB_WORKFLOW -cne 'Product foundation acceptance' -or $env:DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE -notmatch '^[a-f0-9]{40}$' -or $env:DEVBOX_USER_FLOW_DIAGNOSTIC_RUN -notmatch '^\d+$' -or -not $env:DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT) { throw 'Invalid retained UI diagnostic identity.' }
  $diagnostic=Get-Content -LiteralPath $env:DEVBOX_USER_FLOW_DIAGNOSTIC_RECEIPT -Raw | ConvertFrom-Json
  if ($diagnostic.purpose -cne 'retained-installer-ui-diagnostic-only' -or $diagnostic.runnerSourceSha -cne $env:GITHUB_SHA -or $diagnostic.runnerRunId -cne $env:GITHUB_RUN_ID -or $diagnostic.payloadSourceSha -cne $env:DEVBOX_USER_FLOW_DIAGNOSTIC_SOURCE -or $diagnostic.payloadRunId -cne $env:DEVBOX_USER_FLOW_DIAGNOSTIC_RUN -or $diagnostic.repository -cne $env:GITHUB_REPOSITORY -or $diagnostic.sourceWorkflow -cne '.github/workflows/windows-package-candidate.yml' -or $diagnostic.assemblySucceeded -ne $true -or $diagnostic.diagnosticOnly -ne $true -or $diagnostic.promotionEvidence -ne $false) { throw 'Retained UI diagnostic receipt mismatch.' }
  $payloadSource=$diagnostic.payloadSourceSha
  $payloadRun=$diagnostic.payloadRunId
}
if (-not $Cleanup) {
  $scratch = Join-Path $env:RUNNER_TEMP ('devbox-suite-delivery-' + [guid]::NewGuid().ToString('N'))
  $root = Join-Path $scratch 'Suite UI Fixture'
  $staging = Join-Path $scratch 'staging'
  New-Item -ItemType Directory -Path $scratch | Out-Null
  "DEVBOX_USER_FLOW_INSTALL_ROOT=$root" >> $env:GITHUB_ENV
  $env:DEVBOX_USER_FLOW_INSTALL_ROOT=$root
  Run-Owned 'python' @('.github/scripts/prepare-suite-fixture.py',$env:DEVBOX_USER_FLOW_ASSETS,$staging,'--source',$payloadSource,'--run-id',$payloadRun)
  $payloadPath = Join-Path $staging 'suite-payload.json'
  $payload = Get-Content -LiteralPath $payloadPath -Raw | ConvertFrom-Json
  if ($payload.sourceSha -cne $payloadSource) { throw 'User-flow payload source mismatch.' }
  $setup = Join-Path $staging "Devbox_$($payload.suiteVersion)_x64-setup.exe"
  $process = Start-Process -FilePath $setup -ArgumentList "/S /D=$root" -PassThru
  if (-not $process.WaitForExit(180000) -or $process.ExitCode -ne 0) { throw 'Owned user-flow setup failed.' }
  $registration = Get-Content -LiteralPath (Join-Path $root 'suite-registration.json') -Raw | ConvertFrom-Json
  if ($registration.installationKey -notmatch '^[0-9a-f]{64}$') { throw 'Owned installation key missing.' }
  @{schemaVersion=1;sourceSha=$payloadSource;installationKey=$registration.installationKey;root=$root;staging=$staging} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $scratch 'user-flow-owner.json') -Encoding utf8
  $helper = Join-Path $staging 'devbox-suite-bootstrap.exe'
  Run-Owned 'node' @('.github/scripts/windows-suite-delivery-native.mjs',$root,'import')
  Run-Owned $helper @('--activate-clean-install',$root,$payloadPath)
  Run-Owned 'node' @('.github/scripts/windows-suite-delivery-native.mjs',$root,'health')
  Run-Owned $helper @('--commit-clean-install',$root,$payloadPath)
  $activation = Get-Content -LiteralPath (Join-Path $root 'devbox-activation.json') -Raw | ConvertFrom-Json
  if ($activation.phase -cne 'committed') { throw 'Owned user-flow installation was not committed.' }
  exit 0
}
if (-not $env:DEVBOX_USER_FLOW_INSTALL_ROOT) { exit 0 }
$root = [IO.Path]::GetFullPath($env:DEVBOX_USER_FLOW_INSTALL_ROOT)
$scratch = Split-Path -Parent $root
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $scratch.StartsWith($runnerRoot,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $scratch) -notmatch '^devbox-suite-delivery-[a-f0-9]{32}$' -or (Split-Path -Leaf $root) -cne 'Suite UI Fixture') { throw 'Unowned user-flow cleanup target.' }
$receiptPath = Join-Path $scratch 'user-flow-owner.json'
if (-not (Test-Path -LiteralPath $receiptPath)) { throw 'Incomplete provisioning has no owner receipt; preserved for inspection.' }
$receipt=Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
if ($receipt.root -cne $root -or $receipt.sourceSha -cne $payloadSource -or $receipt.installationKey -notmatch '^[a-f0-9]{64}$') { throw 'User-flow cleanup receipt mismatch.' }
$key=$receipt.installationKey
$registration=Get-Content -LiteralPath (Join-Path $root 'suite-registration.json') -Raw | ConvertFrom-Json
if ($registration.installationKey -cne $key) { throw 'User-flow installation changed.' }
$manifest=Get-Content -LiteralPath (Join-Path $root 'devbox-installation.json') -Raw | ConvertFrom-Json
$center=Join-Path $root ($manifest.members | Where-Object product -eq 'control-center').executable
Run-Owned $center @('--stop-agent-for-update')
$live=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase) })
if ($live.Count -ne 0) { throw 'Owned product process still running; preserve fixture rather than force terminate.' }
$uninstall=Start-Process -FilePath (Join-Path $root 'Uninstall.exe') -ArgumentList '/S' -PassThru
if (-not $uninstall.WaitForExit(180000)) { throw 'Owned user-flow uninstall timed out.' }
$timer=[Diagnostics.Stopwatch]::StartNew()
while (-not (Test-Path -LiteralPath (Join-Path $root 'uninstall-complete.json'))) {
  if ($timer.Elapsed.TotalSeconds -gt 180) { throw 'Owned user-flow removal receipt missing.' }
  Start-Sleep -Milliseconds 250
}
foreach ($namespace in @('workspace','apistudio','knowledge','controlcenter','agent','suite-restore','suite-backups','suite-updates')) {
  $data=Join-Path $env:LOCALAPPDATA "com.devbox.v08.$namespace.i$key"
  if (Test-Path -LiteralPath $data) {
    if (@(Get-ChildItem -LiteralPath $data -Recurse -Force | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count) { throw 'Linked fixture namespace; preserved.' }
    Remove-Item -LiteralPath $data -Recurse -Force
  }
}
if (@(Get-ChildItem -LiteralPath $scratch -Recurse -Force | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count) { throw 'Linked installation fixture; preserved.' }
Remove-Item -LiteralPath $scratch -Recurse -Force
