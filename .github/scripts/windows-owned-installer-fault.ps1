param([Parameter(Mandatory=$true)][ValidateSet('Lock','CreateVolume','CleanupVolume','Capture')][string]$Action,[Parameter(Mandatory=$true)][string]$FixtureRoot,[string]$FilePath,[string]$ReadyPath,[string]$ReleasePath,[string]$MetadataPath,[int]$TargetProcessId,[string]$ExpectedExecutable,[string]$ExpectedStartTimeUtc,[ValidateSet('workspace','api-studio','knowledge','control-center')][string]$ProductWindow)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted'){throw 'Disposable hosted Windows required'}
$root=(Resolve-Path -LiteralPath $FixtureRoot).Path.TrimEnd('\')+'\'
$runner=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')+'\'
if(-not $root.StartsWith($runner,[StringComparison]::OrdinalIgnoreCase)){throw 'Unowned fault fixture'}
function AssertPath([string]$path) {
 $resolved=[IO.Path]::GetFullPath($path)
 if(-not $resolved.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -or $resolved.Contains('"')){throw 'Path outside owned fixture'}
 if(Test-Path -LiteralPath $resolved){if((Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked fixture forbidden'}}
 return $resolved
}
if($Action -eq 'Lock') {
 $file=AssertPath $FilePath;$ready=AssertPath $ReadyPath;$release=AssertPath $ReleasePath
 if(-not (Test-Path -LiteralPath $file -PathType Leaf)){throw 'Existing owned receipt required'}
 $handle=[IO.File]::Open($file,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
 try {
  [IO.File]::WriteAllText($ready,'locked')
  $deadline=[DateTime]::UtcNow.AddMinutes(4)
  while(-not (Test-Path -LiteralPath $release)) {if([DateTime]::UtcNow -gt $deadline){throw 'Owned lock release timed out'};Start-Sleep -Milliseconds 100}
 }finally{$handle.Dispose()}
 exit 0
}
if($Action -in @('CreateVolume','CleanupVolume')) {
 $metadata=AssertPath $MetadataPath
 if($Action -eq 'CreateVolume') {
  if(Test-Path -LiteralPath $metadata){throw 'Existing volume receipt forbidden'}
  $id=[guid]::NewGuid().ToString('N');$label='DevboxFault'+$id.Substring(0,20);$vhd=AssertPath (Join-Path $root ($id+'.vhd'))
  $letters=@('Z','Y','X','W','V','U','T','S','R')
  $letter=$letters | Where-Object {-not (Get-PSDrive -Name $_ -ErrorAction SilentlyContinue)} | Select-Object -First 1
  if(-not $letter){throw 'No unused fixture drive letter'}
  $script=AssertPath (Join-Path $root ($id+'.diskpart'))
  [IO.File]::WriteAllText($script,"create vdisk file=`"$vhd`" maximum=64 type=expandable`r`nselect vdisk file=`"$vhd`"`r`nattach vdisk`r`ncreate partition primary`r`nformat fs=ntfs label=$label quick`r`nassign letter=$letter`r`n")
  # Receipt precedes attachment so an interrupted partial setup remains owned.
  @{schemaVersion=1;vhd=$vhd;letter=$letter;label=$label;script=$script} | ConvertTo-Json | Set-Content -LiteralPath $metadata -Encoding utf8
  & diskpart.exe /s $script | Out-Null
  if($LASTEXITCODE -ne 0){throw 'Owned disposable volume setup failed'}
  $volume=Get-Volume -DriveLetter $letter
  if($volume.FileSystemLabel -ne $label -or $volume.Size -gt 64MB -or $volume.SizeRemaining -ge 128MB){throw 'Disposable tiny volume identity/space mismatch'}
  $drive=[string]$letter+':\'
  @{root=$drive;size=[long]$volume.Size;available=[long]$volume.SizeRemaining} | ConvertTo-Json -Compress
 } else {
  $owner=Get-Content -LiteralPath $metadata -Raw | ConvertFrom-Json
  $vhd=AssertPath $owner.vhd
  if($owner.schemaVersion -ne 1 -or $owner.letter -notmatch '^[R-Z]$' -or $owner.label -notmatch '^DevboxFault[a-f0-9]{20}$'){throw 'Invalid owned volume receipt'}
  $volume=Get-Volume -DriveLetter $owner.letter -ErrorAction SilentlyContinue
  if($volume -and $volume.FileSystemLabel -ne $owner.label){throw 'Drive letter reused; preserve volume'}
  $script=AssertPath (Join-Path $root ('detach-'+[guid]::NewGuid().ToString('N')+'.diskpart'))
  [IO.File]::WriteAllText($script,"select vdisk file=`"$vhd`"`r`ndetach vdisk`r`n")
  & diskpart.exe /s $script | Out-Null
  if($LASTEXITCODE -ne 0){throw 'Owned volume detach failed'}
  Remove-Item -LiteralPath $vhd
 }
 exit 0
}
function Focus-OwnedCaptureWindow($Window,[int]$ExpectedProcessId,[scriptblock]$NativeFocus) {
 $current=$Window.Current
 if($current.ProcessId -ne $ExpectedProcessId -or $current.NativeWindowHandle -eq 0){throw 'Owned capture HWND identity unavailable'}
 # UIA top-level windows need not implement keyboard focus. Native foreground
 # selection is checked independently before reading/capturing their rectangle.
 & $NativeFocus ([IntPtr]$current.NativeWindowHandle) $ExpectedProcessId
}
function Select-OwnedCaptureWindow($Windows,$Native,[string]$Product) {
 if(-not $Product){if($Windows.Count -ne 1){throw 'One owned installer window required'};return @($Windows)[0]}
 $namespace=@{workspace='workspace';'api-studio'='apistudio';knowledge='knowledge';'control-center'='controlcenter'}
 $helper='^com\.devbox\.v08\.'+$namespace[$Product]+'\.i[a-f0-9]{64}-sic$'
 if($Windows.Count -gt 32 -or $Native.Count -gt 32){throw 'Owned capture roots exceeded bound'}
 $visible=@($Native | Where-Object {$_.visible -and $_.topLevel -and $_.className -cne 'Tao Thread Event Target' -and $_.className -cnotmatch $helper})
 if($visible.Count -ne 1 -or $visible[0].className -cne 'Tauri Window'){throw 'Unique owned product main window required'}
 $selected=@($Windows | Where-Object {$_.Current.NativeWindowHandle -eq $visible[0].handle -and $_.Current.ClassName -ceq 'Tauri Window'})
 if($selected.Count -ne 1){throw 'Owned product UIA/native window mismatch'}
 return $selected[0]
}
if($Action -eq 'Capture') {
 $image=AssertPath $ExpectedExecutable
 $process=[Diagnostics.Process]::GetProcessById($TargetProcessId)
 if($process.MainModule.FileName -ne $image -or $process.StartTime.ToUniversalTime().ToString('o') -ne $ExpectedStartTimeUtc){throw 'Owned installer identity changed'}
 $output=[IO.Path]::GetFullPath($FilePath);$evidence=[IO.Path]::GetFullPath('product-foundation-evidence/user-flows/screenshots').TrimEnd('\')+'\'
 if(-not $output.StartsWith($evidence,[StringComparison]::OrdinalIgnoreCase)){throw 'Screenshot outside evidence'}
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
 $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$TargetProcessId)
 $windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)
 Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using System.Text;
public static class OwnedInstallerCapture {
 [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 private delegate bool Callback(IntPtr window,IntPtr parameter);
 [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback,IntPtr parameter);
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr window,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder name,int capacity);
 public sealed class WindowInfo {public long handle;public bool visible,topLevel;public string className;}
 public static WindowInfo[] Observe(int expectedProcess) {
  var windows=new List<WindowInfo>();
  Callback callback=delegate(IntPtr window,IntPtr parameter) {
   uint owner;GetWindowThreadProcessId(window,out owner);
   if(owner==(uint)expectedProcess) {
    if(windows.Count>=33)return false;
    var name=new StringBuilder(128);GetClassName(window,name,name.Capacity);
    windows.Add(new WindowInfo {handle=window.ToInt64(),visible=IsWindowVisible(window),topLevel=GetAncestor(window,2)==window,className=name.ToString()});
   }
   return true;
  };
  if(!EnumWindows(callback,IntPtr.Zero))throw new InvalidOperationException("Owned capture inventory unavailable");
  return windows.ToArray();
 }
 public static void Focus(IntPtr window,int expectedProcess) {
  uint process;
  GetWindowThreadProcessId(window,out process);
  if(!IsWindow(window) || !IsWindowVisible(window) || process!=(uint)expectedProcess)
   throw new InvalidOperationException("Owned capture HWND identity changed");
  SetForegroundWindow(window);
  GetWindowThreadProcessId(window,out process);
  if(GetForegroundWindow()!=window || !IsWindow(window) || process!=(uint)expectedProcess)
   throw new InvalidOperationException("Owned capture foreground unavailable");
 }
}
'@
 if($ProductWindow -and [IO.Path]::GetFileName($image) -ine ('devbox-'+$ProductWindow+'.exe')){throw 'Product capture executable mismatch'}
 $window=Select-OwnedCaptureWindow $windows @([OwnedInstallerCapture]::Observe($TargetProcessId)) $ProductWindow
 Focus-OwnedCaptureWindow $window $TargetProcessId {param($handle,$owner) [OwnedInstallerCapture]::Focus($handle,$owner)}
 $rectangle=$window.Current.BoundingRectangle
 if($rectangle.IsEmpty -or $rectangle.Width -lt 100 -or $rectangle.Height -lt 100 -or $rectangle.Width -gt 2560 -or $rectangle.Height -gt 1600){throw 'Invalid owned installer rectangle'}
 [void][IO.Directory]::CreateDirectory((Split-Path -Parent $output))
 $bitmap=[Drawing.Bitmap]::new([int]$rectangle.Width,[int]$rectangle.Height);$graphics=[Drawing.Graphics]::FromImage($bitmap)
 try{$graphics.CopyFromScreen([int]$rectangle.Left,[int]$rectangle.Top,0,0,$bitmap.Size);$bitmap.Save($output,[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$bitmap.Dispose()}
}
