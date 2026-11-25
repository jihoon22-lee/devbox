param(
  [ValidateSet('install','upgrade','launch','stop-owned','assert-stopped','uninstall','installed-identity','remove-owned-registry')][string]$Action,
  [string]$Ownership, [string]$Stage, [string]$Distro, [string]$LinuxSha256, [int]$CdpPort=0, [string]$HelperRoot
)
# Ported from the reviewed S4/S6 helper. All calls run inside the one outer windows-acceptance supervisor.
# S7 (changes §5, §6): stage subdirs baseline\ and candidate\ under one owned stage root, the
# two-artifact upgrade action, installs[] and the 30 s registration poll at uninstall.
# No global environment/settings/policy changes; only owned accept paths recorded before mutation.
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding
$m=Get-Content -LiteralPath $Ownership -Raw -Encoding UTF8 | ConvertFrom-Json
if ($m.channel -ne 'accept' -or $m.nativePreflight.acceptAbsent -ne $true -or $m.token -notmatch '^owned-[0-9a-f]{12}$') {throw 'Owned accept preflight required'}
$install=$m.nativePreflight.installPath
$data=$m.nativePreflight.appLocalData
$roaming=$m.nativePreflight.appRoamingData
$root=$m.windowsRoot
$local=[Environment]::GetFolderPath('LocalApplicationData')
if ($install -notmatch ('^'+[regex]::Escape($local)+'\\DevboxAccept-s7-[a-z0-9-]+$') -or $data -ne "$local\io.github.jihoon22lee.devbox.accept" -or
    $roaming -ne ([Environment]::GetFolderPath('ApplicationData')+'\io.github.jihoon22lee.devbox.accept') -or $root -notmatch '^C:\\dev\\devbox-s7-accept-[a-z0-9-]+$') {throw 'Unexpected owned paths'}
$marker='.s7-accept-owner.json'
$nativeFile=Join-Path (Split-Path $Ownership) ('native-'+$m.token+'.json')
function Save-Native($value) {$value | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $nativeFile -Encoding UTF8}
function Check-Owner($p) {
  $item=Get-Item -LiteralPath $p -Force
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {throw "Owned path became reparse point: $p"}
  $tag=Get-Content -LiteralPath (Join-Path $p $marker) -Raw -Encoding UTF8 | ConvertFrom-Json
  if ($tag.token -ne $m.token -or $tag.sourceSha -ne $m.sourceSha) {throw "Ownership mismatch: $p"}
}
function Check-Exe($native) {
  $exe=Join-Path $install 'Devbox.exe'
  Check-Owner $install
  if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ne $native.exeSha256) {throw 'Owned installed executable changed'}
  return $exe
}
# Tauri NSIS writes HKCU\Software\<manufacturer>\<product> (default)=$INSTDIR and keeps it on a silent
# uninstall unless app data deletion is chosen. Removed only when its (default) is exactly this run's owned
# install path, it has no subkeys and only NSIS value names; the shared parent key is never touched.
$nsisKey='HKCU:\Software\github\Devbox Accept'
function Remove-OwnedNsisKey {
  $row=[ordered]@{path=$nsisKey;existed=$false;removed=$false;status='PASS'}
  if (-not (Test-Path -LiteralPath $nsisKey)) { return $row }
  $row.existed=$true
  $key=Get-Item -LiteralPath $nsisKey
  $value=$key.GetValue('')
  $names=@($key.GetValueNames()); $subs=@($key.GetSubKeyNames())
  $row.default=[string]$value; $row.valueNames=$names; $row.subKeys=$subs
  $key.Close()
  $foreign=@($names | Where-Object { $_ -ne '' -and $_ -ne 'Installer Language' })
  if (($value -is [string]) -and [string]::Equals($value,$install,[StringComparison]::Ordinal) -and $subs.Count -eq 0 -and $foreign.Count -eq 0) {
    Remove-Item -LiteralPath $nsisKey -Force
    $row.removed=-not (Test-Path -LiteralPath $nsisKey)
    if (-not $row.removed) { $row.status='RETAINED'; $row.reason='NSIS key still present after removal' }
  } else { $row.status='RETAINED'; $row.reason='NSIS key is not exactly this run''s owned install record; retained' }
  return $row
}
# The stage root holds the marker; each side's receipt and its one installer are in <root>\baseline or
# <root>\candidate (changes §6 step 3). The receipt must be that side's SHA.
function Check-Stage($stagePath) {
  if ($stagePath -notmatch ('^'+[regex]::Escape($local)+'\\DevboxAccept-s7-stage-[a-z0-9-]+\\(baseline|candidate)$')) {throw 'Owned stage subdir required'}
  $side=$Matches[1]
  $stageRoot=Split-Path -Parent $stagePath
  Check-Owner $stageRoot
  $item=Get-Item -LiteralPath $stagePath -Force
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {throw 'Stage subdir became reparse point'}
  $sha=if ($side -eq 'baseline') {$m.baselineSha} else {$m.sourceSha}
  if ($sha -notmatch '^[0-9a-f]{40}$') {throw 'No bound SHA for this stage side'}
  $r=Get-Content -LiteralPath (Join-Path $stagePath 'receipt.json') -Raw -Encoding UTF8 | ConvertFrom-Json
  if ($r.channel -ne 'accept' -or $r.source_sha -ne $sha -or $r.repository -ne 'jihoon22-lee/devbox' -or $r.workflow -ne '.github/workflows/windows.yml') {throw 'Receipt provenance mismatch'}
  $entries=@($r.installers.PSObject.Properties)
  if ($entries.Count -ne 1) {throw 'Expected exactly one reviewed NSIS installer'}
  $name=$entries[0].Name
  if ([IO.Path]::GetFileName($name) -ne $name -or $name -notlike 'Devbox Accept_*-setup.exe') {throw 'Invalid accept installer name'}
  $setup=Join-Path $stagePath $name
  $hash=(Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash
  if ($hash -ne $entries[0].Value) {throw 'Installer hash mismatch'}
  return @{side=$side;root=$stageRoot;receipt=$r;name=$name;setup=$setup;sha256=$hash}
}
function Install-Row($stage) {
  $exe=Join-Path $install 'Devbox.exe'
  return @{side=$stage.side;sourceSha=$stage.receipt.source_sha;version=$stage.receipt.version;installer=$stage.name;installerSha256=$stage.sha256;
    exeSha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;uninstallerSha256=(Get-FileHash -LiteralPath (Join-Path $install 'uninstall.exe') -Algorithm SHA256).Hash}
}
function Same($record) {
  $p=Get-Process -Id $record.pid -ErrorAction SilentlyContinue
  if ($p -and $p.Path -ieq $record.executable -and $p.StartTime.ToUniversalTime().ToString('o') -eq $record.startedUtc) { return $p }
  return $null
}
if ($Action -eq 'install') {
  # The installer was copied by the driver into an owned stage subdir; re-verify bytes on the Windows side.
  if (Test-Path -LiteralPath $nativeFile) {throw 'This run already installed; the second artifact uses the upgrade action'}
  $staged=Check-Stage $Stage
  $r=$staged.receipt
  $setup=$staged.setup
  foreach($p in @($install,$data,$roaming,$root)) {if(Test-Path -LiteralPath $p){throw "Path became occupied: $p"}}
  # stage is the owned stage root (marker-checked), so the uninstall step removes both subdirs with it.
  $native=@{token=$m.token;sourceSha=$m.sourceSha;channel='accept';state='ownership-recorded-before-mutation';
    install=$install;appLocalData=$data;appRoamingData=$roaming;windowsRoot=$root;stage=$staged.root;receipt=$r;processes=@();installs=@()}
  Save-Native $native
  foreach($p in @($install,$data,$roaming,$root)) {
    [IO.Directory]::CreateDirectory($p) | Out-Null
    @{token=$m.token;sourceSha=$m.sourceSha} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $p $marker) -Encoding UTF8
  }
  # Only newly-owned accept app data: fixed distro, no autostart/first-run prompts, owned shortcuts.
  $settings=@{distro=$Distro;autostart_asked=$true;autostart=$false;tray_notice_shown=$true;
    global_shortcuts=@{palette=$m.paletteShortcut;bottom_terminal=$m.terminalShortcut}} | ConvertTo-Json -Depth 5
  [IO.File]::WriteAllText((Join-Path $data 'settings.json'),$settings,(New-Object Text.UTF8Encoding($false)))
  $installStarted=[DateTime]::UtcNow
  $p=Start-Process -FilePath $setup -ArgumentList @('/S',('/D='+$install)) -PassThru
  $null=$p.Handle
  $native.installerPid=$p.Id;Save-Native $native
  if (-not $p.WaitForExit(120000)) {throw 'Installer did not exit; stop and report UAC/reboot/user-click possibility; do not kill unknown descendants'}
  $p.Refresh()
  if ($p.ExitCode -ne 0) {throw "Installer failed or requires reboot: $($p.ExitCode)"}
  $exe=Join-Path $install 'Devbox.exe'
  $native.exeSha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
  $native.uninstallerSha256=(Get-FileHash -LiteralPath (Join-Path $install 'uninstall.exe') -Algorithm SHA256).Hash
  # A silent installer may auto-start its new executable: record its exact identity, stop only that path.
  $auto=@(Get-Process | Where-Object {$_.Path -ieq $exe -and $_.StartTime.ToUniversalTime() -ge $installStarted})
  $native.autoStarted=@($auto | ForEach-Object {@{pid=$_.Id;executable=$exe;startedUtc=$_.StartTime.ToUniversalTime().ToString('o')}})
  $native.processes=$native.autoStarted;Save-Native $native
  foreach($process in $auto) {Stop-Process -Id $process.Id; $process.WaitForExit(10000) | Out-Null}
  $self=Start-Process -FilePath $exe -ArgumentList @('--self-check','--channel','accept') -PassThru -RedirectStandardOutput (Join-Path (Split-Path $Ownership) 'self-check.stdout.log') -RedirectStandardError (Join-Path (Split-Path $Ownership) 'self-check.stderr.log')
  $null=$self.Handle
  $native.processes=@($native.processes)+@(@{pid=$self.Id;executable=$exe;startedUtc=$self.StartTime.ToUniversalTime().ToString('o');purpose='self-check'})
  Save-Native $native
  if (-not $self.WaitForExit(60000)) {throw 'Installed accept self-check timed out'}
  $self.Refresh()
  if ($self.ExitCode -ne 0) {throw 'Installed accept self-check failed'}
  $native.installedSelfCheck='PASS'
  $native.installs=@(Install-Row $staged)
  $native.state='installed';Save-Native $native
  $native | ConvertTo-Json -Depth 12
  exit
}
if ($Action -eq 'remove-owned-registry') {
  # Residue only, after the owned tree was already uninstalled (run-1 cleanup): never while the owned
  # install, its uninstall registration or an accept app process exists. No file is touched.
  if (Test-Path -LiteralPath $install) {throw 'Owned install still present; use uninstall'}
  if (Test-Path -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Devbox Accept') {throw 'Accept uninstall registration present; use uninstall'}
  if (@(Get-Process -Name 'Devbox' -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$local\DevboxAccept-*" }).Count) {throw 'Accept app process present; retain'}
  Remove-OwnedNsisKey | ConvertTo-Json -Depth 6
  exit
}
$native=Get-Content -LiteralPath $nativeFile -Raw -Encoding UTF8 | ConvertFrom-Json
$exe=Check-Exe $native
if ($Action -eq 'installed-identity') {
  # Transitive binding: the receipt carries the installer hash; the bundled Linux binary must equal the
  # same run's Linux artifact hash, and self-check proved the compiled channel identity and version.
  $linux=Join-Path $install 'devbox-linux'
  $linuxHash=(Get-FileHash -LiteralPath $linux -Algorithm SHA256).Hash.ToLowerInvariant()
  $version=[IO.File]::ReadAllText((Join-Path $install 'devbox-version.txt')).Trim()
  if ($linuxHash -ne $LinuxSha256) {throw 'Bundled devbox-linux differs from the Linux artifact of the same run'}
  @{exe=$exe;exeSha256=$native.exeSha256;bundledLinuxSha256=$linuxHash;bundledVersion=$version;selfCheck=$native.installedSelfCheck;
    binding='installer SHA256 (receipt) -> installed tree; bundled devbox-linux == linux-receipt sha256; self-check channel/version'} | ConvertTo-Json
  exit
}
if ($Action -eq 'stop-owned') {
  $stopped=@()
  foreach($record in $native.processes) {
    $p=Same $record
    if ($p) { Stop-Process -Id $p.Id; if (-not $p.WaitForExit(10000)) {throw 'Owned app did not stop'}; $stopped+=$record.pid }
  }
  @{stopped=$true;method='exact-owned-pid-exe-start-time';pids=$stopped} | ConvertTo-Json
  exit
}
if ($Action -eq 'assert-stopped') {
  foreach($record in $native.processes) { if (Same $record) {throw 'Owned native app is still alive'} }
  $other=@(Get-Process -Name 'Devbox' -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $exe })
  if ($other.Count) {throw 'An unrecorded process runs the owned executable; preserve and report'}
  @{stopped=$true;checkedPids=@($native.processes|ForEach-Object {$_.pid})}|ConvertTo-Json
  exit
}
if ($Action -eq 'upgrade') {
  # Stage 2 of the two-artifact flow (changes §6 step 9): the candidate installer over the owned stage-1
  # install at the same owned unique path. Every precondition is checked before the installer starts.
  $installs=@($native.installs)
  if ($installs.Count -ne 1) {throw 'Upgrade requires exactly one recorded install (installs[0])'}
  if (-not [string]::Equals([string]$native.install,$install,[StringComparison]::Ordinal)) {throw 'Install path differs from the recorded owned path'}
  if ($native.exeSha256 -ne $installs[0].exeSha256) {throw 'Executable differs from installs[0]'}
  foreach($record in $native.processes) { if (Same $record) {throw 'An owned process is running; stop it exactly before the upgrade'} }
  if (@(Get-Process -Name 'Devbox' -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $exe }).Count) {throw 'An unrecorded process runs the owned executable; preserve and report'}
  $staged=Check-Stage $Stage
  if ($staged.side -ne 'candidate' -or $staged.root -ne $native.stage) {throw 'The upgrade installs the candidate from this run''s stage'}
  $native | Add-Member -NotePropertyName state -NotePropertyValue 'upgrade-started' -Force
  Save-Native $native
  $installStarted=[DateTime]::UtcNow
  $p=Start-Process -FilePath $staged.setup -ArgumentList @('/S',('/D='+$install)) -PassThru
  $null=$p.Handle
  $native | Add-Member -NotePropertyName upgradeInstallerPid -NotePropertyValue $p.Id -Force; Save-Native $native
  if (-not $p.WaitForExit(120000)) {throw 'Second installer did not exit; stop and report; do not kill unknown descendants'}
  $p.Refresh()
  if ($p.ExitCode -ne 0) {throw "Second installer failed or requires reboot: $($p.ExitCode)"}
  $row=Install-Row $staged
  $row.uninstallerReplaced=($row.uninstallerSha256 -ne $installs[0].uninstallerSha256)
  if ($row.exeSha256 -eq $installs[0].exeSha256) {throw 'Second install did not change the executable'}
  # NSIS may run the old uninstaller first: re-create the marker only at the recorded owned unique path.
  $row.markerRecreated=$false
  if (-not (Test-Path -LiteralPath (Join-Path $install $marker))) {
    if (-not [string]::Equals([string]$native.install,$install,[StringComparison]::Ordinal)) {throw 'Marker missing outside the owned path; retain'}
    @{token=$m.token;sourceSha=$m.sourceSha} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $install $marker) -Encoding UTF8
    $row.markerRecreated=$true
  }
  Check-Owner $install
  $auto=@(Get-Process | Where-Object {$_.Path -ieq $exe -and $_.StartTime.ToUniversalTime() -ge $installStarted})
  $autoRows=@($auto | ForEach-Object {@{pid=$_.Id;executable=$exe;startedUtc=$_.StartTime.ToUniversalTime().ToString('o')}})
  $native | Add-Member -NotePropertyName processes -NotePropertyValue (@($native.processes)+$autoRows) -Force
  $native | Add-Member -NotePropertyName exeSha256 -NotePropertyValue $row.exeSha256 -Force
  $native | Add-Member -NotePropertyName uninstallerSha256 -NotePropertyValue $row.uninstallerSha256 -Force
  $native | Add-Member -NotePropertyName installs -NotePropertyValue (@($installs)+@($row)) -Force
  Save-Native $native
  foreach($process in $auto) {Stop-Process -Id $process.Id; $process.WaitForExit(10000) | Out-Null}
  $self=Start-Process -FilePath $exe -ArgumentList @('--self-check','--channel','accept') -PassThru -RedirectStandardOutput (Join-Path (Split-Path $Ownership) 'self-check-upgrade.stdout.log') -RedirectStandardError (Join-Path (Split-Path $Ownership) 'self-check-upgrade.stderr.log')
  $null=$self.Handle
  $native | Add-Member -NotePropertyName processes -NotePropertyValue (@($native.processes)+@(@{pid=$self.Id;executable=$exe;startedUtc=$self.StartTime.ToUniversalTime().ToString('o');purpose='self-check-upgrade'})) -Force
  Save-Native $native
  if (-not $self.WaitForExit(60000)) {throw 'Upgraded accept self-check timed out'}
  $self.Refresh()
  if ($self.ExitCode -ne 0) {throw 'Upgraded accept self-check failed'}
  $native | Add-Member -NotePropertyName upgradeSelfCheck -NotePropertyValue 'PASS' -Force
  $native | Add-Member -NotePropertyName state -NotePropertyValue 'upgraded' -Force
  Save-Native $native
  $native | ConvertTo-Json -Depth 12
  exit
}
if ($Action -eq 'launch') {
  $logToken=[Guid]::NewGuid().ToString('N')
  $stdout=Join-Path (Split-Path $Ownership) ('native-'+$logToken+'.stdout.log')
  $stderr=Join-Path (Split-Path $Ownership) ('native-'+$logToken+'.stderr.log')
  $old=$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
  try {
    if ($CdpPort -gt 0) {
      if (@(Get-NetTCPConnection -State Listen -LocalPort $CdpPort -ErrorAction SilentlyContinue).Count) {throw 'Owned CDP port already occupied'}
      # Per-process only (this helper's environment block), never a user/machine variable.
      $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$CdpPort --remote-debugging-address=127.0.0.1"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $HelperRoot 'native-lifecycle.ps1'))) {throw 'Reviewed helper root required'}
    . ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $HelperRoot 'process-launch.ps1'))))
    $p=Start-OwnedLoggedProcess -Executable $exe -StandardOutput $stdout -StandardError $stderr
    $null=$p.Handle
  } finally {$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$old}
  $record=@{pid=$p.Id;executable=$exe;startedUtc=$p.StartTime.ToUniversalTime().ToString('o');stdout=$stdout;stderr=$stderr;cdpPort=$CdpPort;launchedUtc=[DateTime]::UtcNow.ToString('o')}
  $native.processes=@($native.processes)+$record
  Save-Native $native
  $p.PriorityClass='BelowNormal'
  $p.ProcessorAffinity=[IntPtr]15
  $record | ConvertTo-Json
  exit
}
if ($Action -eq 'uninstall') {
  foreach($record in $native.processes) { if (Same $record) {throw 'Quit owned app before uninstall; never kill by name'} }
  $uninstall=Join-Path $install 'uninstall.exe'
  if ((Get-FileHash -LiteralPath $uninstall -Algorithm SHA256).Hash -ne $native.uninstallerSha256) {throw 'Uninstaller identity changed'}
  $p=Start-Process -FilePath $uninstall -ArgumentList '/S' -PassThru
  $null=$p.Handle
  if (-not $p.WaitForExit(120000)) {throw 'Uninstall needs attention; stop and report'}
  $p.Refresh()
  if ($p.ExitCode -ne 0) {throw 'Uninstall failed; stop and report'}
  $deadline=[DateTime]::UtcNow.AddSeconds(20)
  while ((Test-Path -LiteralPath $exe) -and [DateTime]::UtcNow -lt $deadline) {Start-Sleep -Milliseconds 200}
  if (Test-Path -LiteralPath $exe) {throw 'Uninstaller left installed executable; preserve and report'}
  $removed=@()
  foreach($owned in @($root,$data,$roaming,$install,$native.stage)) {
    if ($owned -and (Test-Path -LiteralPath $owned)) {
      Check-Owner $owned
      $links=@(Get-ChildItem -LiteralPath $owned -Force -Recurse | Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint})
      if ($links.Count) {throw 'Owned tree contains reparse points; retain for review'}
      Remove-Item -LiteralPath $owned -Recurse -Force
      $removed+=$owned
    }
  }
  $nsis=Remove-OwnedNsisKey
  # Read-only 30 s poll (changes §5): Uninstall\Devbox Accept belongs to the product's NSIS uninstaller,
  # whose late removal caused the S6 run-2 A6 FAIL; it is waited for, never deleted here.
  $keys=@('HKCU:\Software\Classes\devbox-accept','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Devbox Accept',$nsisKey)
  $waitStart=[DateTime]::UtcNow
  $deadline=$waitStart.AddSeconds(30)
  do {
    $left=@($keys | Where-Object { $_ -and (Test-Path -LiteralPath $_) })
    if (-not $left.Count) { break }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)
  $native | Add-Member -NotePropertyName registryWaitMs -NotePropertyValue ([int]([DateTime]::UtcNow-$waitStart).TotalMilliseconds) -Force
  # $native comes from ConvertFrom-Json: new properties need Add-Member (run 1 threw assigning the missing 'removed').
  $native | Add-Member -NotePropertyName state -NotePropertyValue 'uninstalled-owned-paths-cleaned' -Force
  $native | Add-Member -NotePropertyName removed -NotePropertyValue @($removed) -Force
  $native | Add-Member -NotePropertyName nsisKey -NotePropertyValue $nsis -Force
  $native | Add-Member -NotePropertyName registryLeft -NotePropertyValue @($left) -Force
  Save-Native $native
  if (@($left).Count) {throw ('Uninstaller left accept registration; retained: '+($left -join ', '))}
  $native | ConvertTo-Json -Depth 12
}
