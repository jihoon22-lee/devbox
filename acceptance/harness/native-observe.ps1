param(
  [ValidateSet('preflight','fingerprint','identity','residue','console-windows','console-watch')][string]$Action,
  [string]$SourceSha, [string]$WindowsRoot, [string]$InstallPath,
  [int]$OwnedPid, [string]$ExpectedExe, [string]$StartedUtc, [string]$Nonce, [string]$ProbeMarker,
  [int]$WatchSeconds, [int]$IntervalMs
)
# Read-only observation (ported from the reviewed S4/S6 helper; S7 owned path prefixes).
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding
$acceptId='io.github.jihoon22lee.devbox.accept'
$prodId='io.github.jihoon22lee.devbox'
$local=[Environment]::GetFolderPath('LocalApplicationData')
$roaming=[Environment]::GetFolderPath('ApplicationData')
function Fingerprint($p) {
  if (-not (Test-Path -LiteralPath $p)) { return @{path=$p;exists=$false} }
  $item=Get-Item -LiteralPath $p -Force
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { return @{path=$p;exists=$true;reparsePoint=$true} }
  if ($item.PSIsContainer) { return @{path=$p;exists=$true;directory=$true;lastWriteUtc=$item.LastWriteTimeUtc.ToString('o')} }
  return @{path=$p;exists=$true;sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash;length=$item.Length;mtime=$item.LastWriteTimeUtc.ToString('o')}
}
function Tree($p, $limit) {
  # Bounded, no-follow listing of an existing prod install dir; never reads outside it.
  if (-not (Test-Path -LiteralPath $p)) { return @() }
  $rows=@(); $count=0
  foreach ($f in @(Get-ChildItem -LiteralPath $p -Force -Recurse -File -Attributes !ReparsePoint -ErrorAction Stop)) {
    $count++; if ($count -gt $limit) { throw "Prod tree fingerprint bound exceeded: $p" }
    $rows+=Fingerprint $f.FullName
  }
  return $rows
}
# Accept registrations, including the Tauri NSIS manufacturer/product key HKCU\Software\github\Devbox Accept
# ((default)=install dir, kept by a silent uninstall). The S1 'HKCU\Software\github\Devbox' key is not accept.
$acceptKeys=@('HKCU:\Software\Classes\devbox-accept','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Devbox Accept',
  "HKCU:\Software\Classes\AppUserModelId\$acceptId",'HKCU:\Software\github\Devbox Accept')
function OwnedShape {
  if ($WindowsRoot -notmatch '^C:\\dev\\devbox-s7-accept-[a-z0-9-]+$') { throw 'Unique owned Windows root required' }
  if ($InstallPath -notmatch ('^'+[regex]::Escape($local)+'\\DevboxAccept-s7-[a-z0-9-]+$')) { throw 'Unique owned install path required' }
  return @($InstallPath,$WindowsRoot,"$local\$acceptId","$roaming\$acceptId","$local\Devbox Accept",
    ([Environment]::GetFolderPath('Desktop')+'\Devbox Accept.lnk'),([Environment]::GetFolderPath('Programs')+'\Devbox Accept.lnk'),([Environment]::GetFolderPath('Programs')+'\Devbox Accept'))
}
function AcceptRunning { return @(Get-Process -Name 'Devbox' -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$local\DevboxAccept-*" -or $_.Path -like "$local\Devbox Accept\*" }) }
if ($Action -eq 'residue') {
  # Read-only: what of one run's exact owned Windows resources still exists (no throw on presence).
  $paths=@(OwnedShape) + @($InstallPath -replace '\\DevboxAccept-s7-([a-z0-9-]+)$','\DevboxAccept-s7-stage-$1')
  $nsis=@{exists=(Test-Path -LiteralPath 'HKCU:\Software\github\Devbox Accept')}
  if ($nsis.exists) { $k=Get-Item -LiteralPath 'HKCU:\Software\github\Devbox Accept'; $nsis.default=[string]$k.GetValue(''); $nsis.valueNames=@($k.GetValueNames()); $nsis.subKeys=@($k.GetSubKeyNames()); $k.Close() }
  @{paths=@($paths | ForEach-Object { @{path=$_;exists=(Test-Path -LiteralPath $_)} });
    registry=@($acceptKeys | ForEach-Object { @{path=$_;exists=(Test-Path -LiteralPath $_)} }); nsisKey=$nsis;
    uninstallEntries=@(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue | Get-ItemProperty | Where-Object {$_.DisplayName -eq 'Devbox Accept'}).Count;
    autostart=[bool](Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name $acceptId -ErrorAction SilentlyContinue);
    running=@(AcceptRunning | ForEach-Object { @{pid=$_.Id;path=$_.Path} })} | ConvertTo-Json -Depth 6
  exit
}
if ($Action -eq 'preflight') {
  if (-not (Test-Path -LiteralPath 'C:\dev' -PathType Container)) { throw 'C:\dev parent absent; the harness never creates shared parents' }
  $paths=@(OwnedShape)
  $existing=@($paths | Where-Object { Test-Path -LiteralPath $_ })
  $acceptRegistry=@($acceptKeys | Where-Object { Test-Path -LiteralPath $_ })
  $uninstall=@(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue | Get-ItemProperty | Where-Object {$_.DisplayName -eq 'Devbox Accept'})
  $auto=Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name $acceptId -ErrorAction SilentlyContinue
  $running=@(AcceptRunning)
  if ($existing.Count -or @($acceptRegistry).Count -or $uninstall.Count -or $auto -or $running.Count) {
    throw ('Existing accept resource without this run ownership: preserve and ask: '+(($existing+@($acceptRegistry)) -join ', '))
  }
  @{channel='accept';acceptAbsent=$true;sourceSha=$SourceSha;windowsRoot=$WindowsRoot;installPath=$InstallPath;
    appLocalData="$local\$acceptId";appRoamingData="$roaming\$acceptId";checkedPaths=$paths} | ConvertTo-Json -Depth 8
  exit
}
if ($Action -eq 'identity') {
  $p=Get-Process -Id $OwnedPid
  if ($p.Path -ine $ExpectedExe -or $p.StartTime.ToUniversalTime().ToString('o') -ne $StartedUtc) { throw 'Owned PID/exe/start-time mismatch' }
  if ($ExpectedExe -notlike "$local\DevboxAccept-s7-*\Devbox.exe") { throw 'Unexpected install path' }
  @{owned=$true;pid=$OwnedPid;exe=$p.Path;startedUtc=$StartedUtc} | ConvertTo-Json -Depth 6
  exit
}
# Visible top-level console windows by class, as "hwnd|pid|class" (read-only user32 calls only). One type for both
# console actions.
$consoleType=@'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class S7ConsoleWindows {
  public delegate bool EnumProc(IntPtr window, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder name, int size);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
  public static List<string> Visible() {
    var rows = new List<string>();
    EnumWindows(delegate(IntPtr window, IntPtr data) {
      if (!IsWindowVisible(window)) return true;
      var name = new StringBuilder(256);
      GetClassName(window, name, name.Capacity);
      var cls = name.ToString();
      if (cls == "ConsoleWindowClass" || cls == "CASCADIA_HOSTING_WINDOW_CLASS") { uint pid; GetWindowThreadProcessId(window, out pid); rows.Add(window.ToInt64() + "|" + pid + "|" + cls); }
      return true;
    }, IntPtr.Zero);
    return rows;
  }
}
'@
if ($Action -eq 'console-watch') {
  # Fix round 1 I3 (W1-F): one long-lived watcher instead of one PowerShell per sample. Every IntervalMs by its own
  # stopwatch, one compact JSON line {i, ms, windows}, flushed; it stops when stdin closes (the harness closes it)
  # or after WatchSeconds. Read-only: the same user32 type, and a process name looked up once per new window.
  # FLAGGED: not parsed or run here; needs validate.py --powershell-parse and a Windows dry run first.
  if ($Nonce -notmatch '^[0-9a-z-]{6,64}$' -or $WatchSeconds -lt 1 -or $WatchSeconds -gt 3600 -or $IntervalMs -lt 100 -or $IntervalMs -gt 500) { throw 'Owned nonce and watch bounds required' }
  Add-Type -TypeDefinition $consoleType
  $stdin=[Console]::OpenStandardInput()
  $stop=$stdin.ReadAsync((New-Object byte[] 1), 0, 1)
  $names=@{}
  $clock=[Diagnostics.Stopwatch]::StartNew()
  $i=0
  while (-not $stop.IsCompleted -and $clock.Elapsed.TotalSeconds -lt $WatchSeconds) {
    $ms=[Math]::Round($clock.Elapsed.TotalMilliseconds, 1)
    $windows=@([S7ConsoleWindows]::Visible() | ForEach-Object {
      $parts=$_.Split('|')
      if (-not $names.ContainsKey($parts[0])) { $names[$parts[0]]=[string](Get-Process -Id ([int]$parts[1]) -ErrorAction SilentlyContinue).ProcessName }
      @{hwnd=[long]$parts[0];pid=[int]$parts[1];class=$parts[2];name=$names[$parts[0]]}
    })
    [Console]::Out.WriteLine((@{i=$i;ms=$ms;windows=$windows} | ConvertTo-Json -Depth 4 -Compress))
    [Console]::Out.Flush()
    $i++
    $wait=$i*$IntervalMs-$clock.Elapsed.TotalMilliseconds
    if ($wait -gt 0) { Start-Sleep -Milliseconds ([int]$wait) }
  }
  exit
}
if ($Action -eq 'console-windows') {
  # Read-only (changes section 8.3): visible top-level console windows by class, and the process rows whose
  # command line holds the owned nonce or the probe marker. Only booleans leave; command lines do not.
  if ($Nonce -notmatch '^[0-9a-z-]{6,64}$' -or $ProbeMarker -notmatch '^[\x20-\x7e]{16,96}$') { throw 'Owned nonce and probe marker required' }
  Add-Type -TypeDefinition $consoleType
  $windows=@([S7ConsoleWindows]::Visible() | ForEach-Object { $parts=$_.Split('|'); @{hwnd=[long]$parts[0];pid=[int]$parts[1];class=$parts[2]} })
  $processes=@(Get-CimInstance Win32_Process | ForEach-Object {
    $line=[string]$_.CommandLine
    $byNonce=$line.Contains($Nonce); $byProbe=$line.Contains($ProbeMarker)
    if ($byNonce -or $byProbe) { @{pid=[int]$_.ProcessId;creation=[string]$_.CreationDate.ToFileTimeUtc();nonce=$byNonce;probe=$byProbe} }
  })
  @{windows=$windows;processes=$processes;sampledUtc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json -Depth 5 -Compress
  exit
}
# fingerprint: prod channel identity only, read-only. A running prod app's DB/log files change by
# themselves, so only installation files, settings and registrations are hashed. v0.9 (com.devbox.*)
# directories are recorded by name/existence only and never entered.
$prod=@(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue | Get-ItemProperty | Where-Object {$_.DisplayName -eq 'Devbox'})
$installs=@($prod | ForEach-Object { $_.InstallLocation } | Where-Object { $_ }) + @("$local\Devbox")
$files=@()
foreach ($dir in ($installs | Select-Object -Unique)) { $files+= @{root=$dir;exists=(Test-Path -LiteralPath $dir);files=@(Tree $dir 4000)} }
$legacy=@(Get-ChildItem -LiteralPath $local,$roaming -Directory -Force -Filter 'com.devbox.*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
# 1.0.1: the running prod app is part of prod. Every process whose image is Devbox.exe in a prod install folder is
# recorded by PID and start time (read-only), so a prod app that was stopped or restarted during a run shows.
$prodExes=@($installs | Where-Object { $_ } | ForEach-Object { ([string]$_).Trim('"').TrimEnd('\')+'\Devbox.exe' } | Select-Object -Unique)
$processes=@(Get-Process -Name 'Devbox' -ErrorAction SilentlyContinue | Where-Object {
    $image=$_.Path
    $image -and @($prodExes | Where-Object { [string]::Equals($image,$_,[StringComparison]::OrdinalIgnoreCase) }).Count
  } | Sort-Object Id | ForEach-Object { @{pid=$_.Id;path=$_.Path;startedUtc=$_.StartTime.ToUniversalTime().ToString('o')} })
# The command Windows runs for a devbox:// link, as text: a rewritten command shows even while the key still exists.
$protocolCommand=$null
$protocolKey='HKCU:\Software\Classes\devbox\shell\open\command'
if (Test-Path -LiteralPath $protocolKey) { $key=Get-Item -LiteralPath $protocolKey; $protocolCommand=[string]$key.GetValue(''); $key.Close() }
@{channel='prod';
  installs=$files;
  appData=@((Fingerprint "$local\$prodId"),(Fingerprint "$roaming\$prodId"),(Fingerprint "$local\$prodId\settings.json"));
  registration=@($prod | Select-Object DisplayName,DisplayVersion,InstallLocation,UninstallString);
  protocol=(Test-Path -LiteralPath 'HKCU:\Software\Classes\devbox');
  protocolCommand=$protocolCommand;
  processes=$processes;
  aumid=(Test-Path -LiteralPath "HKCU:\Software\Classes\AppUserModelId\$prodId");
  autostart=[string](Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name $prodId -ErrorAction SilentlyContinue).$prodId;
  legacyV09Names=$legacy} | ConvertTo-Json -Depth 8
