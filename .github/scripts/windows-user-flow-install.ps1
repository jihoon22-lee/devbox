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
function Wait-OwnedUninstall([Diagnostics.Process]$Process,[object]$ExitCode=$null) {
  try {
    if (-not $Process.WaitForExit(180000)) { throw 'Owned user-flow uninstall timed out.' }
    if ($Process.ExitCode -ne 0) { throw 'Owned user-flow uninstall failed; preserve fixture for inspection.' }
  } finally {
    if($null -ne $ExitCode -and $Process.HasExited){$ExitCode.Value=$Process.ExitCode}
  }
}
function Start-OwnedCleanupUninstaller([string]$Root,[string]$Scratch) {
  if(-not [IO.Path]::IsPathRooted($Root) -or $Root.Contains('"') -or $Root.Contains("`r") -or $Root.Contains("`n") -or
    (Split-Path -Leaf $Root) -cne 'Suite UI Fixture' -or
    -not $Root.StartsWith($Scratch.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid owned uninstaller directory.' }
  $source=Join-Path $Root 'Uninstall.exe'
  foreach($ownedPath in @($Root,$Scratch,$source)) {
    if((Get-Item -LiteralPath $ownedPath -Force -ErrorAction Stop).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked owned uninstaller input; preserved.' }
  }
  $image=Join-Path $Scratch ('owned-uninstall-'+[guid]::NewGuid().ToString('N')+'.exe')
  Copy-Item -LiteralPath $source -Destination $image -ErrorAction Stop
  if((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -cne (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash) { throw 'Owned uninstaller copy changed.' }
  $start=[Diagnostics.ProcessStartInfo]::new()
  $start.FileName=$image
  $start.UseShellExecute=$false
  $start.WorkingDirectory=(Get-Location).ProviderPath
  # NSIS requires the final _?= directory raw, including spaces. The copied
  # child avoids its automatic temporary relaunch, preserving actual exit code.
  $start.Arguments='/S _?='+$Root
  $process=[Diagnostics.Process]::new()
  $process.StartInfo=$start
  try {
    if(-not $process.Start()){throw 'Owned uninstaller did not start.'}
    return $process
  } catch {
    try{$process.Dispose()}catch{}
    throw
  }
}
function Write-OwnedCleanupObservation([string]$Root,[string]$EvidencePath,
  [ValidateSet('stop-agent','check-live-products','start-uninstall','wait-uninstaller','wait-removal-receipt','remove-owned-data','remove-owned-installation')][string]$Stage,
  [ValidateSet('running','failed','completed')][string]$Status,[object]$ExitCode=$null) {
  $observation=[ordered]@{
    schemaVersion=1;stage=$Stage;status=$Status;exitCode=$ExitCode
    updatePending=[bool](Test-Path -LiteralPath (Join-Path $Root 'suite-update.json'))
    restorePending=[bool](Test-Path -LiteralPath (Join-Path $Root 'suite-data-restore.json'))
    uninstallPending=[bool](Test-Path -LiteralPath (Join-Path $Root 'uninstall-plan.json'))
    removalReceipt=[bool](Test-Path -LiteralPath (Join-Path $Root 'uninstall-complete.json'))
  }
  [IO.File]::WriteAllText($EvidencePath,($observation | ConvertTo-Json -Compress),[Text.UTF8Encoding]::new($false))
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class OwnedCleanupIdentity {
 [StructLayout(LayoutKind.Sequential)] public struct Info {
  public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
  public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
 }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle file,out Info info);
 public static ulong[] Read(string path) {
  using(var file=CreateFileW(path,0x80,3,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   Info info;
   if(file.IsInvalid || !GetFileInformationByHandle(file,out info) || (info.Attributes&0x400)!=0) throw new InvalidOperationException("Shortcut identity unavailable");
   return new ulong[]{info.Volume,((ulong)info.IndexHigh<<32)|info.IndexLow};
  }
 }
}
'@
function Assert-OwnedCleanupIdentity([string]$Root,$Expected) {
  $actual=[OwnedCleanupIdentity]::Read($Root)
  if($Expected.Count -ne 2 -or $actual[0] -ne [System.UInt64]$Expected[0] -or $actual[1] -ne [System.UInt64]$Expected[1]){throw 'Owned cleanup root identity changed.'}
}
$payloadSource=$env:GITHUB_SHA
$payloadRun=$env:GITHUB_RUN_ID
if ($env:DEVBOX_USER_FLOW_REVALIDATION_PROOF) {
  $revalidated = & node .github/scripts/revalidation-identity.mjs
  if ($LASTEXITCODE -ne 0) { throw 'Revalidation identity rejected.' }
  $revalidated = $revalidated | ConvertFrom-Json
  $payloadSource=$revalidated.sourceSha
  $payloadRun=[string]$revalidated.buildRunId
}
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
$evidenceDirectory=Join-Path (Get-Location).ProviderPath 'product-foundation-evidence'
New-Item -ItemType Directory -Force -Path $evidenceDirectory | Out-Null
$cleanupEvidence=Join-Path $evidenceDirectory ("owned-cleanup-$key.json")
$cleanupStage='stop-agent'
$cleanupExitCode=$null
$uninstall=$null
$cleanupCompleted=$false
Assert-OwnedCleanupIdentity $root $registration.rootIdentity
$diagnosticInputs=@('suite-registration.json','devbox-installation.json','Uninstall.exe')
$diagnosticHashes=@{}
foreach($name in $diagnosticInputs){$diagnosticHashes[$name]=(Get-FileHash -LiteralPath (Join-Path $root $name) -Algorithm SHA256).Hash}
try {
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running'
  Run-Owned $center @('--stop-agent-for-update')
  $cleanupStage='check-live-products'
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running'
  $live=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase) })
  if ($live.Count -ne 0) { throw 'Owned product process still running; preserve fixture rather than force terminate.' }
  $cleanupStage='start-uninstall'
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running'
  $uninstall=Start-OwnedCleanupUninstaller $root $scratch
  $cleanupStage='wait-uninstaller'
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running'
  Wait-OwnedUninstall $uninstall ([ref]$cleanupExitCode)
  $cleanupStage='wait-removal-receipt'
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running' $cleanupExitCode
  $timer=[Diagnostics.Stopwatch]::StartNew()
  while (-not (Test-Path -LiteralPath (Join-Path $root 'uninstall-complete.json'))) {
    if ($timer.Elapsed.TotalSeconds -gt 180) { throw 'Owned user-flow removal receipt missing.' }
    Start-Sleep -Milliseconds 250
  }
  $cleanupStage='remove-owned-data'
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running' $cleanupExitCode
  foreach ($namespace in @('workspace','apistudio','knowledge','controlcenter','agent','suite-restore','suite-backups','suite-updates')) {
    $data=Join-Path $env:LOCALAPPDATA "com.devbox.v08.$namespace.i$key"
    if (Test-Path -LiteralPath $data) {
      if (@(Get-ChildItem -LiteralPath $data -Recurse -Force | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count) { throw 'Linked fixture namespace; preserved.' }
      Remove-Item -LiteralPath $data -Recurse -Force
    }
  }
  $cleanupStage='remove-owned-installation'
  Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'running' $cleanupExitCode
  if (@(Get-ChildItem -LiteralPath $scratch -Recurse -Force | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count) { throw 'Linked installation fixture; preserved.' }
  Remove-Item -LiteralPath $scratch -Recurse -Force
  $cleanupCompleted=$true
} catch {
  $originalFailure=$_
  try {
    Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage 'failed' $cleanupExitCode
    if($cleanupStage -ceq 'wait-uninstaller' -and $cleanupExitCode -eq 1) {
      Assert-OwnedCleanupIdentity $root $registration.rootIdentity
      $plan=$registration.uninstaller
      $entries=@($plan.files | Where-Object {$_.relative -ceq 'Uninstall.exe'})
      if($plan.schemaVersion -ne 1 -or $plan.installationKey -cne $key -or $entries.Count -ne 1){throw 'Invalid registered uninstall diagnostic plan.'}
      Assert-OwnedCleanupIdentity $root $plan.rootIdentity
      $source=Join-Path $root 'Uninstall.exe'
      Assert-OwnedCleanupIdentity $source $entries[0].identity
      if((Get-Item -LiteralPath $source -Force).Length -ne $entries[0].bytes -or
        (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ine $entries[0].sha256){throw 'Registered uninstaller changed; diagnostic skipped.'}
      foreach($name in $diagnosticInputs){
        $inputPath=Join-Path $root $name
        if((Get-Item -LiteralPath $inputPath -Force).Attributes -band [IO.FileAttributes]::ReparsePoint -or
          (Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash -cne $diagnosticHashes[$name]){throw 'Changed uninstall diagnostic input; preserved.'}
      }
      foreach($name in @('suite-update.json','suite-data-restore.json','uninstall-plan.json','uninstall-complete.json')) {
        if(Test-Path -LiteralPath (Join-Path $root $name)){throw 'Partial or pending uninstall; diagnostic skipped.'}
      }
      $live=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase) })
      if($live.Count -ne 0){throw 'Owned process remains; diagnostic skipped.'}
      Run-Owned 'node' @('.github/scripts/windows-suite-uninstall-diagnostic.mjs',$root,$cleanupEvidence) 90000
    }
  } catch {
    # Diagnostic input refusals and failures never replace original exit 1.
  }
  throw $originalFailure
} finally {
  try {
    if($null -ne $uninstall -and $uninstall.HasExited){$cleanupExitCode=$uninstall.ExitCode}
    $cleanupStatus=if($cleanupCompleted){'completed'}else{'failed'}
    Write-OwnedCleanupObservation $root $cleanupEvidence $cleanupStage $cleanupStatus $cleanupExitCode
  } catch {
    # Evidence must never replace the original owned cleanup failure.
  } finally {
    if($null -ne $uninstall){
      try{$uninstall.Dispose()}catch{}
    }
  }
}
