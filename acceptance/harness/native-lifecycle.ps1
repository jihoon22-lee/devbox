param(
  [ValidateSet('install','upgrade','launch','stop-owned','assert-stopped','uninstall','installed-identity','remove-owned-registry',
    'update-begin','update-observe','update-adopt')][string]$Action,
  [string]$Ownership, [string]$Stage, [string]$Distro, [string]$LinuxSha256, [int]$CdpPort=0, [string]$HelperRoot,
  [string]$UpdateFixture, [string]$Foreign, [int]$WaitSeconds=0
)
# Ported from the reviewed S4/S6 helper. All calls run inside the one outer windows-acceptance supervisor.
# S7 (changes sections 5 and 6): stage subdirs baseline\ and candidate\ under one owned stage root, the
# two-artifact upgrade action, installs[] and the 30 s registration poll at uninstall.
# No global environment/settings/policy changes; only owned accept paths recorded before mutation.
# 1.0.1: pure ASCII (D4: Windows PowerShell 5.1 reads a script without a BOM in the ANSI code page); the self-check
# runs through the handle-list launcher (D3); update-begin/-observe/-adopt and -Foreign serve the update flow (D5).
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
# 1.0.1: where an accept installer without /D= installs when NSIS has no memory of the product. Never an owned path:
# it must not exist at any point of a run (the preflight proved it absent).
$defaultInstall="$local\Devbox Accept"
# The app's own update runs its installer with /S only (app/src-tauri/src/platform/update_runner/execute.rs:230-232), so
# the target is what NSIS remembers: the (default) value of $nsisKey. It must be this run's owned install path.
function Test-InstallMemory {
  if (-not (Test-Path -LiteralPath $nsisKey)) { return $false }
  $key=Get-Item -LiteralPath $nsisKey
  $value=$key.GetValue('')
  $key.Close()
  return (($value -is [string]) -and [string]::Equals($value.TrimEnd('\'),$install.TrimEnd('\'),[StringComparison]::OrdinalIgnoreCase))
}
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
# <root>\candidate (changes section 6 step 3). The receipt must be that side's SHA.
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
# D3: the one launcher for every start of the owned executable. Start-Process with redirected output would hand this
# PowerShell's inheritable handles, the WSL interop pipes included, to the child.
function Load-Launcher {
  if (-not $HelperRoot -or -not (Test-Path -LiteralPath (Join-Path $HelperRoot 'native-lifecycle.ps1'))) {throw 'Reviewed helper root required'}
  return [ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $HelperRoot 'process-launch.ps1')))
}
function Hash-OrNull($p) {
  if (Test-Path -LiteralPath $p -PathType Leaf) { try { return (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash } catch { return $null } }
  return $null
}
function Version-Text {
  $file=Join-Path $install 'devbox-version.txt'
  if (Test-Path -LiteralPath $file -PathType Leaf) { try { return [IO.File]::ReadAllText($file).Trim() } catch { return $null } }
  return $null
}
# The update runner and its installer copy run from <accept app data>\updates\install-*\ (runner_files.rs:158-163, 293, 306).
function Staged-Processes { return @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -like ($data+'\updates\install-*\*.exe') }) }
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
  . (Load-Launcher)
  $self=Invoke-OwnedLoggedProcess -Executable $exe -Arguments '--self-check --channel accept' -WaitMilliseconds 60000 -StandardOutput (Join-Path (Split-Path $Ownership) 'self-check.stdout.log') -StandardError (Join-Path (Split-Path $Ownership) 'self-check.stderr.log')
  $native.processes=@($native.processes)+@(@{pid=$self.pid;executable=$exe;startedUtc=$self.startedUtc;purpose='self-check';stoppedOnTimeout=$self.stoppedOnTimeout})
  Save-Native $native
  if (-not $self.exited) {throw ('Installed accept self-check timed out; that exact process was '+$(if ($self.stoppedOnTimeout) {'stopped'} else {'NOT stopped: retain and report'}))}
  if ($self.exitCode -ne 0) {throw 'Installed accept self-check failed'}
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
if ($Action -like 'update-*') {
  # 1.0.1 (D5): the app's own update replaces the owned executable, so these three actions read the owned install
  # without the executable pin (Check-Exe). update-adopt pins the new bytes, keeping both hashes, before any later action.
  $native=Get-Content -LiteralPath $nativeFile -Raw -Encoding UTF8 | ConvertFrom-Json
  if (-not [string]::Equals([string]$native.install,$install,[StringComparison]::Ordinal)) {throw 'Install path differs from the recorded owned path'}
  $exePath=Join-Path $install 'Devbox.exe'
  $uninstaller=Join-Path $install 'uninstall.exe'
  if ($Action -eq 'update-begin') {
    if ($native.update) {throw 'This run already began an update'}
    Check-Owner $install
    $apps=@($native.processes | Where-Object { [int]$_.cdpPort -gt 0 -and $_.updateFixture -and (Same $_) })
    if ($apps.Count -ne 1) {throw 'Exactly one running owned app with a debug port and the update fixture is required'}
    if ((Hash-OrNull $exePath) -ne $native.exeSha256) {throw 'Owned installed executable changed before the update'}
    if (@(Staged-Processes).Count) {throw 'An update runner is already running'}
    # Before the update can change anything: its installer must be going to write into the owned install path only.
    if (-not (Test-InstallMemory)) {throw 'The NSIS install location of Devbox Accept is not this run''s owned install path; the update is not started'}
    if (Test-Path -LiteralPath $defaultInstall) {throw 'The default Devbox Accept install folder exists; the update is not started'}
    $update=@{startedUtc=[DateTime]::UtcNow.ToString('o');appPid=$apps[0].pid;appStartedUtc=$apps[0].startedUtc;cdpPort=[int]$apps[0].cdpPort;
      exeSha256Before=$native.exeSha256;uninstallerSha256Before=$native.uninstallerSha256;versionBefore=(Version-Text)}
    $native | Add-Member -NotePropertyName update -NotePropertyValue $update -Force
    Save-Native $native
    $update | ConvertTo-Json -Depth 6
    exit
  }
  if (-not $native.update) {throw 'No update was begun in this run'}
  $began=[DateTime]::Parse([string]$native.update.startedUtc,[Globalization.CultureInfo]::InvariantCulture,[Globalization.DateTimeStyles]::RoundtripKind).ToUniversalTime()
  function Fresh-Apps {
    # Started from the owned unique install path after the update began, and not recorded yet: the runner's relaunch.
    $known=@($native.processes | ForEach-Object { ([string]$_.pid)+'|'+[string]$_.startedUtc })
    return @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $exePath -and $_.StartTime.ToUniversalTime() -ge $began -and
      ($known -notcontains (([string]$_.Id)+'|'+$_.StartTime.ToUniversalTime().ToString('o'))) })
  }
  # `$null | ForEach-Object` runs once, so an empty result is filtered before it is mapped.
  function Rows($list) { return @($list | Where-Object { $_ } | ForEach-Object { @{pid=$_.Id;name=[IO.Path]::GetFileName($_.Path);startedUtc=$_.StartTime.ToUniversalTime().ToString('o')} }) }
  if ($Action -eq 'update-observe') {
    # Read-only. Polled by the driver: what runs, what the runner recorded, and the installed bytes.
    $staged=@(Staged-Processes)
    $runnerPids=@($staged | Where-Object { [IO.Path]::GetFileName($_.Path) -ieq 'runner.exe' } | ForEach-Object { $_.Id })
    # The runner keeps WSL running with `wsl.exe -d <distro> --exec sleep 900` (runner_files.rs:73-81); only its identity leaves.
    $keep=@()
    if ($runnerPids.Count) {
      $keep=@(Get-CimInstance Win32_Process -Filter "Name='wsl.exe'" -ErrorAction SilentlyContinue | Where-Object { $runnerPids -contains [int]$_.ParentProcessId } |
        ForEach-Object { @{pid=[int]$_.ProcessId;creation=[string]$_.CreationDate.ToFileTimeUtc();parentPid=[int]$_.ParentProcessId;sleep900=([string]$_.CommandLine).EndsWith(' --exec sleep 900')} })
    }
    $outcomes=@()
    $updates=Join-Path $data 'updates'
    if (Test-Path -LiteralPath $updates -PathType Container) {
      foreach ($dir in @(Get-ChildItem -LiteralPath $updates -Directory -Force -Filter 'install-*' -ErrorAction SilentlyContinue)) {
        $row=@{stage=$dir.Name;outcome=$null;error=$null}
        $file=Join-Path $dir.FullName 'outcome.json'
        try {
          if ((Test-Path -LiteralPath $file -PathType Leaf) -and (Get-Item -LiteralPath $file -Force).Length -le 32768) {
            $record=[IO.File]::ReadAllText($file) | ConvertFrom-Json
            $row.outcome=[string]$record.outcome; $row.error=[string]$record.error
            $row.targetVersion=[string]$record.binding.targetVersion; $row.fixtureInstall=[bool]$record.binding.fixtureInstall
          }
        } catch { $row.error='unreadable' }
        $outcomes+=$row
      }
    }
    $uninstallKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Devbox Accept'
    $display=$null
    if (Test-Path -LiteralPath $uninstallKey) { $display=[string](Get-ItemProperty -LiteralPath $uninstallKey -ErrorAction SilentlyContinue).DisplayVersion }
    $port=[int]$native.update.cdpPort
    @{observedUtc=[DateTime]::UtcNow.ToString('o');oldAlive=@($native.processes | Where-Object { Same $_ } | ForEach-Object { $_.pid });
      newApps=@(Rows @(Fresh-Apps));staged=@(Rows $staged);keepAlive=$keep;outcomes=$outcomes;
      exeSha256=(Hash-OrNull $exePath);uninstallerSha256=(Hash-OrNull $uninstaller);version=(Version-Text);displayVersion=$display;
      markerPresent=(Test-Path -LiteralPath (Join-Path $install $marker));
      debugPortListening=[bool]@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue).Count} | ConvertTo-Json -Depth 6 -Compress
    exit
  }
  if ($Action -eq 'update-adopt') {
    # After the update, and again before every teardown of this flow: wait (bounded) until no runner or installer runs,
    # record the relaunched app by exact path and start time, and pin the bytes the update installed. A runner or
    # installer that still runs is never stopped: the action throws and everything is retained.
    if ($WaitSeconds -lt 0 -or $WaitSeconds -gt 600) {throw 'WaitSeconds out of range'}
    $deadline=[DateTime]::UtcNow.AddSeconds($WaitSeconds)
    while (@(Staged-Processes).Count -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (@(Staged-Processes).Count) {throw 'The update runner or installer is still running; nothing is adopted; retain and report'}
    $adopted=@(@(Fresh-Apps) | Where-Object { $_ } | ForEach-Object { @{pid=$_.Id;executable=$exePath;startedUtc=$_.StartTime.ToUniversalTime().ToString('o');purpose='update-relaunch';cdpPort=[int]$native.update.cdpPort} })
    $exeNow=Hash-OrNull $exePath
    $uninstallerNow=Hash-OrNull $uninstaller
    if (-not $exeNow -or -not $uninstallerNow) {throw 'The owned install has no executable or uninstaller after the update; retain and report'}
    $row=@{adoptedUtc=[DateTime]::UtcNow.ToString('o');adopted=$adopted;exeSha256After=$exeNow;uninstallerSha256After=$uninstallerNow;versionAfter=(Version-Text);
      exeChanged=($exeNow -ne $native.update.exeSha256Before);markerRecreated=$false;
      installMemoryIsOwned=(Test-InstallMemory);defaultInstallPresent=(Test-Path -LiteralPath $defaultInstall)}
    # NSIS may run the old uninstaller first: re-create the marker only at the recorded owned unique path (as the upgrade action does).
    if (-not (Test-Path -LiteralPath (Join-Path $install $marker))) {
      @{token=$m.token;sourceSha=$m.sourceSha} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $install $marker) -Encoding UTF8
      $row.markerRecreated=$true
    }
    Check-Owner $install
    $native | Add-Member -NotePropertyName processes -NotePropertyValue (@($native.processes)+$adopted) -Force
    $native | Add-Member -NotePropertyName exeSha256 -NotePropertyValue $exeNow -Force
    $native | Add-Member -NotePropertyName uninstallerSha256 -NotePropertyValue $uninstallerNow -Force
    $native | Add-Member -NotePropertyName updateAdoptions -NotePropertyValue (@($native.updateAdoptions | Where-Object { $_ })+@($row)) -Force
    Save-Native $native
    # The relaunched app is recorded first, so the exact stop can still end it. An install outside the owned path is
    # never adopted as a result: the action fails, assert-stopped fails too, and everything is retained.
    if ($row.defaultInstallPresent) {throw 'The default Devbox Accept install folder exists after the update: the installer did not use the owned path; retain and report'}
    $row | ConvertTo-Json -Depth 6
    exit
  }
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
  # 1.0.1 (D5): no update runner or installer of the owned app may run, and a debug port this run opened must be closed
  # (the WebView2 processes end shortly after their app; waited for at most 15 s, never stopped by name).
  if (@(Staged-Processes).Count) {throw 'An update runner or installer of the owned app is still running; preserve and report'}
  if (Test-Path -LiteralPath $defaultInstall) {throw 'The default Devbox Accept install folder exists; it is not owned by this run; preserve and report'}
  $ports=@($native.processes | ForEach-Object { [int]$_.cdpPort } | Where-Object { $_ -gt 0 } | Select-Object -Unique)
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do {
    $open=@($ports | Where-Object { @(Get-NetTCPConnection -State Listen -LocalPort $_ -ErrorAction SilentlyContinue).Count })
    if (-not $open.Count) { break }
    Start-Sleep -Milliseconds 300
  } while ([DateTime]::UtcNow -lt $deadline)
  if (@($open).Count) {throw ('An owned debug port is still listening; preserve and report: '+($open -join ','))}
  @{stopped=$true;checkedPids=@($native.processes|ForEach-Object {$_.pid});debugPortsClosed=@($ports)}|ConvertTo-Json
  exit
}
if ($Action -eq 'upgrade') {
  # Stage 2 of the two-artifact flow (changes section 6 step 9): the candidate installer over the owned stage-1
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
  . (Load-Launcher)
  $self=Invoke-OwnedLoggedProcess -Executable $exe -Arguments '--self-check --channel accept' -WaitMilliseconds 60000 -StandardOutput (Join-Path (Split-Path $Ownership) 'self-check-upgrade.stdout.log') -StandardError (Join-Path (Split-Path $Ownership) 'self-check-upgrade.stderr.log')
  $native | Add-Member -NotePropertyName processes -NotePropertyValue (@($native.processes)+@(@{pid=$self.pid;executable=$exe;startedUtc=$self.startedUtc;purpose='self-check-upgrade';stoppedOnTimeout=$self.stoppedOnTimeout})) -Force
  Save-Native $native
  if (-not $self.exited) {throw ('Upgraded accept self-check timed out; that exact process was '+$(if ($self.stoppedOnTimeout) {'stopped'} else {'NOT stopped: retain and report'}))}
  if ($self.exitCode -ne 0) {throw 'Upgraded accept self-check failed'}
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
  $oldFixture=$env:DEVBOX_UPDATE_FIXTURE
  try {
    if ($CdpPort -gt 0) {
      if ($CdpPort -lt 1024 -or $CdpPort -gt 65535) {throw 'Owned CDP port out of range'}
      if (@(Get-NetTCPConnection -State Listen -LocalPort $CdpPort -ErrorAction SilentlyContinue).Count) {throw 'Owned CDP port already occupied'}
      # Per-process only (this helper's environment block), never a user/machine variable.
      $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$CdpPort --remote-debugging-address=127.0.0.1"
    }
    if ($UpdateFixture) {
      # 1.0.1 (D5): an accept build reads DEVBOX_UPDATE_FIXTURE once at start (app/src-tauri/src/platform/update.rs:26-48).
      # Per-process only, like the debug port. The folder is this run's <owned stage>\update with exactly its three files.
      $stageRoot=[string]$native.stage
      if (-not $stageRoot -or -not [string]::Equals($UpdateFixture,(Join-Path $stageRoot 'update'),[StringComparison]::Ordinal)) {throw 'The update fixture must be the update folder of the owned stage'}
      Check-Owner $stageRoot
      if ((Get-Item -LiteralPath $UpdateFixture -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {throw 'Update fixture became reparse point'}
      $names=@(Get-ChildItem -LiteralPath $UpdateFixture -Force | ForEach-Object { $_.Name } | Sort-Object)
      if (($names -join '|') -ne 'installer.exe|metadata.json|owner.json') {throw 'The update fixture must hold exactly owner.json, metadata.json and installer.exe'}
      # The installer the app will run is byte for byte the staged candidate installer, whose receipt Check-Stage ties to
      # the bound candidate SHA.
      $candidate=Check-Stage (Join-Path $stageRoot 'candidate')
      if ((Get-FileHash -LiteralPath (Join-Path $UpdateFixture 'installer.exe') -Algorithm SHA256).Hash -ne $candidate.sha256) {throw 'The update fixture installer is not the staged candidate installer'}
      $env:DEVBOX_UPDATE_FIXTURE=$UpdateFixture
    }
    . (Load-Launcher)
    $p=Start-OwnedLoggedProcess -Executable $exe -StandardOutput $stdout -StandardError $stderr
    $null=$p.Handle
  } finally {$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$old;$env:DEVBOX_UPDATE_FIXTURE=$oldFixture}
  $record=@{pid=$p.Id;executable=$exe;startedUtc=$p.StartTime.ToUniversalTime().ToString('o');stdout=$stdout;stderr=$stderr;cdpPort=$CdpPort;updateFixture=$UpdateFixture;launchedUtc=[DateTime]::UtcNow.ToString('o')}
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
  $plan=$null
  if ($Foreign) {
    # 1.0.1 (A7): the plan is read and every name checked before the uninstaller starts; a bad plan changes nothing.
    $plan=Get-Content -LiteralPath $Foreign -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($plan.token -ne $m.token) {throw 'Foreign file plan belongs to another run'}
    $prefix='^'+[regex]::Escape('s7-foreign-'+$m.token+'-')
    foreach ($entry in @($plan.files)) {
      if (([string]$entry.relative) -notmatch ($prefix+'[a-z0-9]+(\.[a-z0-9]+)?(/[a-z0-9]+\.[a-z0-9]+)?$')) {throw 'Unexpected foreign file name'}
    }
  }
  $p=Start-Process -FilePath $uninstall -ArgumentList '/S' -PassThru
  $null=$p.Handle
  if (-not $p.WaitForExit(120000)) {throw 'Uninstall needs attention; stop and report'}
  $p.Refresh()
  if ($p.ExitCode -ne 0) {throw 'Uninstall failed; stop and report'}
  $deadline=[DateTime]::UtcNow.AddSeconds(20)
  while ((Test-Path -LiteralPath $exe) -and [DateTime]::UtcNow -lt $deadline) {Start-Sleep -Milliseconds 200}
  if (Test-Path -LiteralPath $exe) {throw 'Uninstaller left installed executable; preserve and report'}
  if ($plan) {
    # 1.0.1 (D5, A7): what the product's uninstaller left of this run's planted files, read before the owned tree is
    # removed. Only names, lengths and hashes leave (the driver compares them); then exactly those files and their one
    # folder are removed, the folder only when it is empty.
    function Foreign-Rows {
      $rows=@()
      foreach ($entry in @($plan.files)) {
        $relative=[string]$entry.relative
        $path=Join-Path $install $relative.Replace('/','\')
        $row=@{relative=$relative;exists=(Test-Path -LiteralPath $path -PathType Leaf)}
        if ($row.exists) {
          $item=Get-Item -LiteralPath $path -Force
          $row.reparsePoint=[bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
          $row.length=$item.Length
          $row.sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        }
        $rows+=$row
      }
      return ,$rows
    }
    # The NSIS uninstaller goes on in a copy of itself after uninstall.exe returned. Its file work is over once
    # uninstall.exe is gone and it removed its own Uninstall registration (read-only wait, at most 30 s). A read before
    # that could pass on files the uninstaller was still going to remove, so `foreignUninstallerEnded` must be true for
    # A7; the files are then read twice, 1 s apart, and both reads must agree (`foreignStable`).
    $foreignKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Devbox Accept'
    $foreignStart=[DateTime]::UtcNow
    $deadline=$foreignStart.AddSeconds(30)
    while (((Test-Path -LiteralPath $uninstall) -or (Test-Path -LiteralPath $foreignKey)) -and [DateTime]::UtcNow -lt $deadline) {Start-Sleep -Milliseconds 200}
    $foreignEnded=-not ((Test-Path -LiteralPath $uninstall) -or (Test-Path -LiteralPath $foreignKey))
    $foreignFirst=Foreign-Rows
    Start-Sleep -Milliseconds 1000
    $foreignRows=Foreign-Rows
    function Foreign-Text($rows) { return (@($rows | ForEach-Object { [string]$_.relative+'|'+[string]$_.exists+'|'+[string]$_.length+'|'+[string]$_.sha256+'|'+[string]$_.reparsePoint }) -join ';') }
    $foreignStable=((Foreign-Text $foreignFirst) -ceq (Foreign-Text $foreignRows))
    $native | Add-Member -NotePropertyName foreignAfterUninstall -NotePropertyValue @($foreignRows) -Force
    $native | Add-Member -NotePropertyName foreignUninstallerEnded -NotePropertyValue $foreignEnded -Force
    $native | Add-Member -NotePropertyName foreignStable -NotePropertyValue $foreignStable -Force
    $native | Add-Member -NotePropertyName foreignWaitMs -NotePropertyValue ([int]([DateTime]::UtcNow-$foreignStart).TotalMilliseconds) -Force
    Save-Native $native
    foreach ($row in $foreignRows) { if ($row.exists) { Remove-Item -LiteralPath (Join-Path $install $row.relative.Replace('/','\')) -Force } }
    $folder=[string]$plan.directory
    if ($folder -match ($prefix+'[a-z0-9]+$')) {
      $folderPath=Join-Path $install $folder
      if ((Test-Path -LiteralPath $folderPath -PathType Container) -and -not @(Get-ChildItem -LiteralPath $folderPath -Force).Count) { Remove-Item -LiteralPath $folderPath -Force }
    }
  }
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
  # Read-only 30 s poll (changes section 5): Uninstall\Devbox Accept belongs to the product's NSIS uninstaller,
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
