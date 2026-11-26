param(
  [string]$Ownership, [string]$HelperRoot, [int]$WaitSeconds=0
)
# A8 (1.0.1, issue #678): the window agent that driver.py NativeOps.window_agent starts, and S7OwnedWindow, the owned
# app's top-level windows as one type: read-only user32 calls, and the only window messages any helper may post (WM_CLOSE,
# and WM_SYSCOMMAND with SC_MINIMIZE or SC_RESTORE), each a fixed method that the agent's Post-Command calls once
# (validate.py allows these lines in this file only). The ownership checks up to Check-Exe, Same and Load-Launcher are
# native-lifecycle.ps1's, line for line (test_s7_harness.py compares them). Pure ASCII (D4).
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
$native=Get-Content -LiteralPath $nativeFile -Raw -Encoding UTF8 | ConvertFrom-Json
$exe=Check-Exe $native
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class S7OwnedWindow {
 public delegate bool EnumProc(IntPtr window,IntPtr data);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc,IntPtr data);
 [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder name,int size);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window,StringBuilder text,int size);
 [DllImport("user32.dll",EntryPoint="GetWindowLongW")] static extern int GetWindowLong(IntPtr window,int index);
 [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window,uint command);
 [DllImport("user32.dll",SetLastError=true)] static extern bool PostMessage(IntPtr window,uint message,IntPtr wParam,IntPtr lParam);
 [StructLayout(LayoutKind.Sequential)] struct LastInput { public uint size; public uint time; }
 [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LastInput info);
 [DllImport("kernel32.dll")] static extern uint GetTickCount();
 static uint PidOf(IntPtr window) { uint pid=0; GetWindowThreadProcessId(window,out pid); return pid; }
 static string ClassOf(IntPtr window) { var name=new StringBuilder(256); GetClassName(window,name,name.Capacity); return name.ToString(); }
 static string TitleOf(IntPtr window) { var text=new StringBuilder(256); GetWindowText(window,text,text.Capacity); return text.ToString(); }
 static string Flag(bool value) { return value?"1":"0"; }
 // Every top-level window of one process: hwnd|visible|iconic|style|exStyle|owned|class|title (the title last: it may hold '|').
 public static List<string> Of(uint pid) {
  var rows=new List<string>();
  EnumWindows(delegate(IntPtr window,IntPtr data) {
   if(PidOf(window)==pid)rows.Add(window.ToInt64()+"|"+Flag(IsWindowVisible(window))+"|"+Flag(IsIconic(window))+"|"+GetWindowLong(window,-16)+"|"+
    GetWindowLong(window,-20)+"|"+Flag(GetWindow(window,4)!=IntPtr.Zero)+"|"+ClassOf(window)+"|"+TitleOf(window));
   return true;
  },IntPtr.Zero);
  return rows;
 }
 // One window now: pid|visible|iconic|class. The pid is 0 once the handle is no longer a window.
 public static string State(long handle) {
  var window=new IntPtr(handle);
  if(!IsWindow(window))return "0|0|0|";
  return PidOf(window)+"|"+Flag(IsWindowVisible(window))+"|"+Flag(IsIconic(window))+"|"+ClassOf(window);
 }
 // Milliseconds since the last keyboard or mouse input of this session (-1 when it cannot be read). Both clocks are the
 // 32-bit tick count, so the difference is taken modulo 2^32 (unchecked).
 public static long IdleMilliseconds() {
  var info=new LastInput(); info.size=(uint)Marshal.SizeOf(typeof(LastInput));
  if(!GetLastInputInfo(ref info))return -1;
  return unchecked(GetTickCount()-info.time);
 }
 // 0 when the message was posted, else the Win32 error (-1 when none was set).
 static int Posted(bool posted) { if(posted)return 0; int error=Marshal.GetLastWin32Error(); return error==0?-1:error; }
 public static int Close(long handle) { return Posted(PostMessage(new IntPtr(handle),0x0010,IntPtr.Zero,IntPtr.Zero)); }
 public static int Minimize(long handle) { return Posted(PostMessage(new IntPtr(handle),0x0112,new IntPtr(0xF020),IntPtr.Zero)); }
 public static int Restore(long handle) { return Posted(PostMessage(new IntPtr(handle),0x0112,new IntPtr(0xF120),IntPtr.Zero)); }
}
'@
# A8 (1.0.1, issue #678): one long-lived helper. It finds the owned app (the one running launch record: exact PID,
# executable and start time) and that app's one visible main window, then reads one command per stdin line and answers
# each with one JSON line: close (WM_CLOSE), minimize and restore (WM_SYSCOMMAND), launch (a plain second start of the
# owned executable, which the running app answers by showing its main window), idle, state and end. Every answer carries
# `n`, the number of the command it answers, and `ms` on this helper's stopwatch. Right before every message or start it
# checks the owned process again, that the handle is still its window, and that the session had no keyboard or mouse
# input for 30 s; a refused command sends nothing. It never stops a process: a second start that keeps running is
# recorded, so teardown's exact stop ends it.
if ($WaitSeconds -lt 30 -or $WaitSeconds -gt 240) {throw 'WaitSeconds out of range'}
if (-not $HelperRoot -or -not (Test-Path -LiteralPath (Join-Path $HelperRoot 'native-window.ps1'))) {throw 'Reviewed helper root required'}
$clock=[Diagnostics.Stopwatch]::StartNew()
$n=0
function Say($row) {
  $row.ms=[Math]::Round($clock.Elapsed.TotalMilliseconds,1)
  $row.n=$n
  [Console]::Out.WriteLine(($row | ConvertTo-Json -Depth 6 -Compress))
  [Console]::Out.Flush()
}
$apps=@($native.processes | Where-Object { $_.stdout -and -not $_.purpose -and [string]::Equals([string]$_.executable,$exe,[StringComparison]::OrdinalIgnoreCase) -and (Same $_) })
if ($apps.Count -ne 1) { Say @{event='ready';ok=$false;reason=('Exactly one running owned app launch is required; found '+$apps.Count)}; exit }
$app=$apps[0]
$appPid=[uint32]$app.pid
. (Load-Launcher)
function Windows-Now {
  return @([S7OwnedWindow]::Of($appPid) | ForEach-Object {
    $f=$_.Split([char[]]'|',8)
    @{hwnd=[long]$f[0];visible=($f[1] -eq '1');iconic=($f[2] -eq '1');style=[int]$f[3];exStyle=[int]$f[4];owned=($f[5] -eq '1');class=$f[6];title=$f[7]}
  })
}
# The main window: class Tauri Window, title devbox (tauri.conf.json), a caption, not owned, not a tool window. The
# capture window is a borderless tool-like popup and starts hidden. The tray icon's hidden window is tray_icon_app.
$windows=@(Windows-Now)
$tray=@($windows | Where-Object { $_.class -ceq 'tray_icon_app' }).Count
$main=@($windows | Where-Object { $_.class -ceq 'Tauri Window' -and $_.title -ceq 'devbox' -and $_.visible -and -not $_.iconic -and -not $_.owned -and
  (($_.style -band 0x00C00000) -eq 0x00C00000) -and -not ($_.exStyle -band 0x00000080) })
$ready=@{event='ready';ok=$false;pid=[long]$app.pid;executable=[string]$app.executable;startedUtc=[string]$app.startedUtc;trayWindows=$tray;
  windows=@($windows | ForEach-Object { @{hwnd=$_.hwnd;class=$_.class;title=$_.title;visible=$_.visible;iconic=$_.iconic} })}
if ($main.Count -ne 1) {
  $ready.reason=('Exactly one visible, restored main window of the owned app (Tauri Window, devbox, with a caption) is required; found '+$main.Count)
  Say $ready
  exit
}
$hwnd=[long]$main[0].hwnd
function Window-State {
  $f=[S7OwnedWindow]::State($hwnd).Split([char[]]'|',4)
  return @{pid=[long]$f[0];visible=($f[1] -eq '1');iconic=($f[2] -eq '1');class=$f[3]}
}
# Why the handle may not be acted on (else $null): the owned process must still be the exact recorded one, and the
# handle still a Tauri Window of that process.
function Not-Owned($state) {
  if (-not (Same $app)) { return 'the owned app process (exact PID, executable and start time) is gone' }
  if ($state.pid -eq 0) { return 'the handle is no longer a window' }
  if ($state.pid -ne [long]$app.pid) { return 'the handle belongs to another process' }
  if ($state.class -cne 'Tauri Window') { return 'the handle is another window' }
  return $null
}
# The second start may take the foreground (the app's own focus call presses Alt when Windows refuses it), and minimize
# and restore move the focus: a window changes only after 30 s without user input. The driver waits for that first;
# this is the same check right before the action. An unreadable input time refuses the action.
$idleNeedMs=30000
function User-Active($row) {
  $row.idleMs=[S7OwnedWindow]::IdleMilliseconds()
  if ($row.idleMs -lt 0) { $row.refused='The time of the last user input cannot be read'; return $true }
  if ($row.idleMs -lt $idleNeedMs) { $row.userActive=$true; return $true }
  return $false
}
function Acted { return [Math]::Round($clock.Elapsed.TotalMilliseconds,1) }
function Wait-State($want, [int]$boundMs) {
  $from=$clock.Elapsed.TotalMilliseconds
  while ($true) {
    $state=Window-State
    if (& $want $state) { return @{state=$state;reachedMs=[Math]::Round($clock.Elapsed.TotalMilliseconds-$from,1)} }
    if ($clock.Elapsed.TotalMilliseconds-$from -ge $boundMs) { return @{state=$state;reachedMs=$null} }
    Start-Sleep -Milliseconds 50
  }
}
function Post-Command($name) {
  $row=@{event=$name;posted=$false}
  if ($name -eq 'close') {
    # The product hides its main window on close only while its tray exists (lib.rs CloseRequested, tray.rs available);
    # without it the close quits the app.
    $row.trayWindows=@(Windows-Now | Where-Object { $_.class -ceq 'tray_icon_app' }).Count
    if ($row.trayWindows -lt 1) { $row.refused='The owned app has no tray window, so a close would quit it'; return $row }
  }
  $row.before=Window-State
  $why=Not-Owned $row.before
  if ($why) { $row.gone=$true; $row.why=$why; return $row }
  if (User-Active $row) { return $row }
  if ($name -eq 'close') { $code=[S7OwnedWindow]::Close($hwnd); $want={ param($s) -not $s.visible } }
  elseif ($name -eq 'minimize') { $code=[S7OwnedWindow]::Minimize($hwnd); $want={ param($s) $s.visible -and $s.iconic } }
  elseif ($name -eq 'restore') { $code=[S7OwnedWindow]::Restore($hwnd); $want={ param($s) $s.visible -and -not $s.iconic } }
  else { $row.refused='Not a window command'; return $row }
  $row.actedMs=(Acted)
  $row.postError=$code
  $row.posted=($code -eq 0)
  if (-not $row.posted) { return $row }
  $seen=Wait-State $want 1500
  $row.after=$seen.state
  $row.reachedMs=$seen.reachedMs
  return $row
}
function Launch-Again {
  $row=@{event='launch';started=$false}
  $row.before=Window-State
  $why=Not-Owned $row.before
  if ($why) { $row.gone=$true; $row.why=$why; return $row }
  try { $null=Check-Exe $native } catch { $row.refused='The owned executable failed its check: '+$_.Exception.Message; return $row }
  if (User-Active $row) { return $row }
  $logToken=[Guid]::NewGuid().ToString('N')
  $out=Join-Path (Split-Path $Ownership) ('native-'+$logToken+'.stdout.log')
  $err=Join-Path (Split-Path $Ownership) ('native-'+$logToken+'.stderr.log')
  $old=$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
  $oldFixture=$env:DEVBOX_UPDATE_FIXTURE
  try {
    # A plain start, as a user's: no debug port and no update fixture in this helper's own environment block.
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$null
    $env:DEVBOX_UPDATE_FIXTURE=$null
    $second=Invoke-OwnedHandover -Executable $exe -StandardOutput $out -StandardError $err -WaitMilliseconds 5000
    # The handover: the second process has ended (it hands over before it ends), or the 5 s bound passed.
    $row.actedMs=(Acted)
  } finally {$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$old;$env:DEVBOX_UPDATE_FIXTURE=$oldFixture}
  # Recorded whether or not it ended, so that a second app that kept running is stopped exactly by teardown.
  $record=@{pid=$second.pid;executable=$exe;startedUtc=$second.startedUtc;stdout=$out;stderr=$err;purpose='second-launch';
    launchedUtc=[DateTime]::UtcNow.ToString('o');exited=$second.exited;exitCode=$second.exitCode}
  $native.processes=@($native.processes)+$record
  Save-Native $native
  $row.started=$true
  $row.second=$record
  $seen=Wait-State { param($s) $s.visible -and -not $s.iconic } 2000
  $row.after=$seen.state
  $row.reachedMs=$seen.reachedMs
  # Input during the start (for example the app's own Alt when it takes the foreground) shows here.
  $row.idleAfterMs=[S7OwnedWindow]::IdleMilliseconds()
  return $row
}
$ready.ok=$true
$ready.hwnd=$hwnd
$ready.title=[string]$main[0].title
$ready.class=[string]$main[0].class
$ready.style=$main[0].style
$ready.state=(Window-State)
Say $ready
# One line at a time, each read bounded by the time left: a StreamReader on the raw stdin stream reads on the thread
# pool (console-watch reads the same stream asynchronously), so Wait($left) does expire. Console.In's ReadLineAsync
# would not: on .NET Framework it reads synchronously. A closed stdin ends the read (null) and the agent.
$stdin=New-Object IO.StreamReader([Console]::OpenStandardInput())
while ($true) {
  $left=[int]($WaitSeconds*1000-$clock.Elapsed.TotalMilliseconds)
  if ($left -le 0) { Say @{event='expired';state=(Window-State)}; break }
  $read=$stdin.ReadLineAsync()
  if (-not $read.Wait($left)) { Say @{event='expired';state=(Window-State)}; break }
  $line=$read.Result
  if ($null -eq $line) { break }
  $n++
  $name=$line.Trim()
  try {
    if ($name -eq 'end') { Say @{event='end';state=(Window-State)}; break }
    elseif ($name -eq 'idle') { Say @{event='idle';idleMs=[S7OwnedWindow]::IdleMilliseconds()} }
    elseif ($name -eq 'state') { $state=Window-State; Say @{event='state';state=$state;why=(Not-Owned $state)} }
    elseif ($name -eq 'launch') { Say (Launch-Again) }
    elseif ($name -eq 'close' -or $name -eq 'minimize' -or $name -eq 'restore') { Say (Post-Command $name) }
    else { Say @{event='unknown';refused='Unknown command'}; break }
  } catch { Say @{event=$name;error=$_.Exception.Message} }
}
exit
