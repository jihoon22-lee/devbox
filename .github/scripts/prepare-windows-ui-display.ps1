param([int]$ClientWidth=1180,[int]$ClientHeight=780,[ValidatePattern('^[a-z][a-z0-9-]{0,63}$')][string]$EvidenceName='hosted-display')
$ErrorActionPreference='Stop'
# Display mutation is allowed only on the disposable hosted runner used by CI.
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows' -or $env:GITHUB_REPOSITORY -ne 'jihoon22-lee/devbox' -or $env:GITHUB_RUN_ID -notmatch '^\d+$' -or $env:GITHUB_SHA -notmatch '^[a-f0-9]{40}$') {
  throw 'Display preparation requires disposable GitHub-hosted Windows; never spoof CI environment variables.'
}
if ($ClientWidth -lt 720 -or $ClientWidth -gt 2000 -or $ClientHeight -lt 480 -or $ClientHeight -gt 1200) { throw 'Invalid required client dimensions' }
$destination=Join-Path (Get-Location) "product-foundation-evidence/$EvidenceName.json"
New-Item -ItemType Directory -Force -Path (Split-Path $destination) | Out-Null
$report=[ordered]@{sourceSha=$env:GITHUB_SHA;runId=$env:GITHUB_RUN_ID;requestedClient=@{width=$ClientWidth;height=$ClientHeight};status='FAIL';temporary=$true;before=$null;after=$null;modes=@();selected=$null;testResult=$null;applyResult=$null;failure=$null}
# EnumDisplaySettings returns the complete driver-supported DEVMODEW buffer.
# Pass that same buffer to ChangeDisplaySettings (CDS_TEST then CDS_FULLSCREEN).
# No CDS_UPDATEREGISTRY and no fabricated driver mode are used.
# Dot-source this script in the same pwsh process that runs the GUI journeys: the
# temporary mode belongs to that process and must remain alive during node.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class DevboxHostedDisplay {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
  public sealed class Mode {
    public uint Width,Height,Bits,Frequency;
    public byte[] Buffer;
  }
  public sealed class Snapshot {
    public int Width,Height,WorkLeft,WorkTop,WorkWidth,WorkHeight,FrameWidth,FrameHeight;
    public uint Dpi;
  }
  [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="EnumDisplaySettingsW")]
  static extern bool EnumDisplaySettings(string device,int mode,IntPtr data);
  [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="ChangeDisplaySettingsW")]
  static extern int ChangeDisplaySettings(IntPtr data,uint flags);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int metric);
  [DllImport("user32.dll")] static extern int GetSystemMetricsForDpi(int metric,uint dpi);
  [DllImport("user32.dll")] static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll",EntryPoint="SystemParametersInfoW")]
  static extern bool SystemParametersInfo(uint action,uint parameter,out Rect rect,uint flags);
  static Mode ReadMode(int index) {
    IntPtr pointer=Marshal.AllocHGlobal(220);
    try {
      Marshal.Copy(new byte[220],0,pointer,220);
      Marshal.WriteInt16(pointer,68,220);
      if(!EnumDisplaySettings(null,index,pointer)) return null;
      byte[] data=new byte[220]; Marshal.Copy(pointer,data,0,220);
      return new Mode {Width=(uint)Marshal.ReadInt32(pointer,172),Height=(uint)Marshal.ReadInt32(pointer,176),Bits=(uint)Marshal.ReadInt32(pointer,168),Frequency=(uint)Marshal.ReadInt32(pointer,184),Buffer=data};
    } finally {Marshal.FreeHGlobal(pointer);}
  }
  public static Mode[] Modes() {
    var modes=new List<Mode>();
    for(int index=0;index<10000;index++) {var mode=ReadMode(index);if(mode==null)return modes.ToArray();modes.Add(mode);}
    throw new InvalidOperationException("Display mode enumeration exceeded bounded inventory");
  }
  public static Snapshot Observe() {
    IntPtr previous=SetThreadDpiAwarenessContext(new IntPtr(-4));
    try {
      Rect work;
      if(!SystemParametersInfo(0x30,0,out work,0))throw new InvalidOperationException("Primary work area unavailable");
      uint dpi=GetDpiForSystem();
      int padding=GetSystemMetricsForDpi(92,dpi);
      return new Snapshot {Width=GetSystemMetrics(0),Height=GetSystemMetrics(1),WorkLeft=work.Left,WorkTop=work.Top,WorkWidth=work.Right-work.Left,WorkHeight=work.Bottom-work.Top,Dpi=dpi,FrameWidth=2*(GetSystemMetricsForDpi(32,dpi)+padding),FrameHeight=2*(GetSystemMetricsForDpi(33,dpi)+padding)+GetSystemMetricsForDpi(4,dpi)};
    } finally {if(previous!=IntPtr.Zero)SetThreadDpiAwarenessContext(previous);}
  }
  public static int Change(Mode mode,uint flags) {
    IntPtr pointer=Marshal.AllocHGlobal(220);
    try {Marshal.Copy(mode.Buffer,0,pointer,220);return ChangeDisplaySettings(pointer,flags);}
    finally {Marshal.FreeHGlobal(pointer);}
  }
}
'@
function Mode-Summary($Mode) { return @{width=$Mode.Width;height=$Mode.Height;bits=$Mode.Bits;frequency=$Mode.Frequency} }
try {
  $before=[DevboxHostedDisplay]::Observe()
  $report.before=$before
  if($before.Dpi -lt 96 -or $before.Dpi -gt 384 -or $before.WorkWidth -le 0 -or $before.WorkHeight -le 0){throw 'Invalid actual desktop metrics'}
  $requiredWidth=[math]::Ceiling($ClientWidth*$before.Dpi/96)+$before.FrameWidth
  $requiredHeight=[math]::Ceiling($ClientHeight*$before.Dpi/96)+$before.FrameHeight
  $report.requiredPhysical=@{width=$requiredWidth;height=$requiredHeight}
  $modes=@([DevboxHostedDisplay]::Modes())
  $report.modes=@($modes | ForEach-Object {Mode-Summary $_})
  # Prefer 1920x1080, then the smallest enumerated mode able to retain the
  # existing work-area reservation and fit the configured client plus frame.
  $reservedWidth=$before.Width-$before.WorkWidth
  $reservedHeight=$before.Height-$before.WorkHeight
  $candidates=@($modes | Where-Object {$_.Bits -ge 32 -and $_.Width-$reservedWidth -ge $requiredWidth -and $_.Height-$reservedHeight -ge $requiredHeight} | Sort-Object @{Expression={if($_.Width -eq 1920 -and $_.Height -eq 1080){0}else{1}}},@{Expression={[long]$_.Width*$_.Height}},@{Expression={[math]::Abs([int]$_.Frequency-60)}})
  if(-not $candidates.Count){throw 'No supported hosted display mode can fit the configured native client area'}
  $selected=$candidates[0]
  $report.selected=Mode-Summary $selected
  $report.testResult=[DevboxHostedDisplay]::Change($selected,2) # CDS_TEST
  if($report.testResult -ne 0){throw "Hosted display mode test failed: $($report.testResult)"}
  $report.applyResult=[DevboxHostedDisplay]::Change($selected,4) # CDS_FULLSCREEN: temporary, no registry write
  if($report.applyResult -ne 0){throw "Hosted display mode change failed: $($report.applyResult)"}
  for($attempt=0;$attempt -lt 20;$attempt++) {
    $after=[DevboxHostedDisplay]::Observe()
    $report.after=$after
    if($after.Width -eq $selected.Width -and $after.Height -eq $selected.Height -and $after.WorkWidth -ge $requiredWidth -and $after.WorkHeight -ge $requiredHeight){break}
    Start-Sleep -Milliseconds 100
  }
  if($after.Width -ne $selected.Width -or $after.Height -ne $selected.Height -or $after.WorkWidth -lt $requiredWidth -or $after.WorkHeight -lt $requiredHeight){throw 'Actual hosted display/work area does not satisfy the requested native client dimensions'}
  $report.status='PASS'
} catch {
  $report.failure=$_.Exception.Message
  throw
} finally {
  [IO.File]::WriteAllText($destination,($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
}
