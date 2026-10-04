# Native UI actions are limited to an exact process in an owned disposable fixture.
param(
  [Parameter(Mandatory=$true)][int]$TargetProcessId,
  [Parameter(Mandatory=$true)][string]$ExpectedExecutable,
  [Parameter(Mandatory=$true)][string]$ExpectedStartTimeUtc,
  [Parameter(Mandatory=$true)][string]$FixtureRoot,
  [Parameter(Mandatory=$true)][ValidateSet('Invoke','Close','Inspect','ChooseFile','SaveFile','Resize','Minimize','Activate')][string]$Action,
  [string]$ControlName,
  [string]$ControlId,
  [string]$FilePath,
  [string]$WindowName,
  [ValidateSet('workspace','api-studio','knowledge','control-center')][string]$ProductWindow,
  [int]$Width,
  [int]$Height
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
# The .NET Framework proxy loader inspects its caller's reflected type. A
# PowerShell dynamic call can have no reflected type and leave classic Win32
# controls as Pane without InvokePattern. Enter through a non-inlined typed
# frame before the first UIA query so the framework loads its standard proxies.
Add-Type -ReferencedAssemblies UIAutomationClient,UIAutomationTypes -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using System.Text;
using System.Runtime.CompilerServices;
using System.Windows.Automation;
public static class DevboxInstallerAutomation {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr window, uint flags);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  private delegate bool EnumWindowCallback(IntPtr window, IntPtr parameter);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowCallback callback, IntPtr parameter);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder name, int capacity);
  public sealed class NativeWindowInfo {
    public long nativeHandle;
    public uint nativeProcessId;
    public bool visible, minimized, topLevel;
    public string className;
  }
  public sealed class NativeInventory {
    public int count;
    public NativeWindowInfo[] windows;
  }
  public static NativeInventory ObserveNativeWindows(uint expectedProcessId) {
    int count=0;
    var owned=new List<NativeWindowInfo>();
    EnumWindowCallback callback=delegate(IntPtr window,IntPtr parameter) {
      uint processId;
      GetWindowThreadProcessId(window,out processId);
      if(processId!=expectedProcessId) return true;
      count++;
      if(owned.Count<32) {
        var name=new StringBuilder(100);
        GetClassName(window,name,name.Capacity);
        owned.Add(new NativeWindowInfo {
          nativeHandle=window.ToInt64(),nativeProcessId=processId,
          visible=IsWindowVisible(window),minimized=IsIconic(window),
          topLevel=GetAncestor(window,2)==window,className=name.ToString()
        });
      }
      return true;
    };
    if(!EnumWindows(callback,IntPtr.Zero)) throw new InvalidOperationException("Owned native window enumeration failed");
    return new NativeInventory {count=count,windows=owned.ToArray()};
  }
  [MethodImpl(MethodImplOptions.NoInlining)]
  public static void Initialize() {
    ClientSettings.RegisterClientSideProviders(new ClientSideProviderDescription[0]);
  }
}
"@
[DevboxInstallerAutomation]::Initialize()
$root=(Resolve-Path -LiteralPath $FixtureRoot).Path.TrimEnd('\')+'\'
$exe=(Resolve-Path -LiteralPath $ExpectedExecutable).Path
if(-not $exe.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)){throw 'Executable outside fixture'}
$process=Get-Process -Id $TargetProcessId
if(-not [string]::Equals($process.Path,$exe,[StringComparison]::OrdinalIgnoreCase)){throw 'Process executable mismatch'}
$started=$process.StartTime.ToUniversalTime().ToString('o')
if($started -ne $ExpectedStartTimeUtc){throw 'Process start time mismatch'}
if($ProductWindow -and -not [string]::Equals([IO.Path]::GetFileName($exe),('devbox-'+$ProductWindow+'.exe'),[StringComparison]::OrdinalIgnoreCase)){throw 'Product window executable mismatch'}
$productLifecycle=$ProductWindow -and $Action -in @('Close','Resize','Minimize','Activate','Inspect')
if($productLifecycle -and $WindowName){throw 'Product lifecycle requires all owned roots for modal review'}
$namespace=@{workspace='workspace';'api-studio'='apistudio';knowledge='knowledge';'control-center'='controlcenter'}
$helperClass=if($ProductWindow){'^com\.devbox\.v08\.'+$namespace[$ProductWindow]+'\.i[a-f0-9]{64}-sic$'}else{''}

$condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$TargetProcessId)
$windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)
if($WindowName) {
  $windows=@($windows | Where-Object {$_.Current.Name -ceq $WindowName})
}
# UIA may expose hidden Tao/WebView helper roots alongside the real main HWND.
# Native visibility includes minimized windows; IsOffscreen cannot select Activate.
$windowRecords=@($windows | Select-Object -First 32 | ForEach-Object {
  $info=$_.Current
  $handle=[IntPtr]::new($info.NativeWindowHandle)
  $nativeOwner=[uint32]0
  [void][DevboxInstallerAutomation]::GetWindowThreadProcessId($handle,[ref]$nativeOwner)
  $metadata=@{
    name=$info.Name.Substring(0,[Math]::Min(200,$info.Name.Length))
    className=$info.ClassName.Substring(0,[Math]::Min(100,$info.ClassName.Length))
    enabled=$info.IsEnabled;offscreen=$info.IsOffscreen;nativeHandle=$handle.ToInt64()
    nativeProcessId=$nativeOwner;visible=[DevboxInstallerAutomation]::IsWindowVisible($handle)
    minimized=[DevboxInstallerAutomation]::IsIconic($handle)
    topLevel=($handle -ne [IntPtr]::Zero -and [DevboxInstallerAutomation]::GetAncestor($handle,2) -eq $handle)
  }
  @{element=$_;metadata=$metadata}
})
$observedWindowCount=$windows.Count
$nativeInventory=[DevboxInstallerAutomation]::ObserveNativeWindows([uint32]$TargetProcessId)
if($Action -in @('Close','Resize','Minimize','Activate','Inspect')) {
  $windows=@($windowRecords | Where-Object {
    $_.metadata.visible -and $_.metadata.topLevel -and $_.metadata.nativeProcessId -eq $TargetProcessId -and
      (-not $productLifecycle -or ($_.metadata.className -cne 'Tao Thread Event Target' -and $_.metadata.className -cnotmatch $helperClass))
  } | ForEach-Object {$_.element})
  if($observedWindowCount -gt 32){$windows=@()}
  if($productLifecycle) {
    # A modal omitted from the UIA root tree still blocks a product lifecycle action.
    $nativeCandidates=@($nativeInventory.windows | Where-Object {
      $_.visible -and $_.topLevel -and $_.className -cne 'Tao Thread Event Target' -and $_.className -cnotmatch $helperClass
    })
    $selectedHandles=@($windows | ForEach-Object {[long]$_.Current.NativeWindowHandle})
    if($nativeInventory.count -gt 32 -or $nativeCandidates.Count -ne $windows.Count -or
      @($nativeCandidates | Where-Object {$_.nativeHandle -notin $selectedHandles}).Count -ne 0 -or
      ($windows.Count -eq 1 -and ($windows[0].Current.ClassName -cne 'Tauri Window' -or $nativeCandidates[0].className -cne 'Tauri Window'))){$windows=@()}
  }
}
if($Action -eq 'Inspect' -and $windows.Count -ne 1) {
  @{processId=$TargetProcessId;startTimeUtc=$started;windowCount=$observedWindowCount;selectedWindowCount=$windows.Count;nativeWindowCount=$nativeInventory.count;nativeWindows=$nativeInventory.windows;windows=@($windowRecords | ForEach-Object {$_.metadata})} | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
if($Action -in @('ChooseFile','SaveFile')) {
  if($Action -eq 'SaveFile') {
    $parent=(Resolve-Path -LiteralPath (Split-Path -Parent $FilePath)).Path
    $file=Join-Path $parent (Split-Path -Leaf $FilePath)
    if(Test-Path -LiteralPath $file){throw 'Save fixture must use a fresh output path'}
  } else {
    $file=(Resolve-Path -LiteralPath $FilePath).Path
    if(-not (Test-Path -LiteralPath $file -PathType Leaf)){throw 'File picker input must be regular file'}
  }
  if(-not $file.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)){throw 'File picker path outside owned fixture'}
  $dialogs=@($windows | Where-Object {$_.Current.ClassName -eq '#32770'})
  if($dialogs.Count -ne 1){throw 'Expected one owned native file picker'}
  $dialog=$dialogs[0]
  $fileNameCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')
  $fields=$dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,$fileNameCondition)
  if($fields.Count -ne 1){throw 'Owned file name field unavailable'}
  $field=$fields.Item(0)
  if(-not $field.Current.IsEnabled -or $field.Current.IsOffscreen){throw 'File name field unavailable'}
  $value=$null
  if(-not $field.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$value)) {
    $editCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit)
    $edits=$field.FindAll([System.Windows.Automation.TreeScope]::Descendants,$editCondition)
    if($edits.Count -ne 1){throw 'Unique file name editor unavailable'}
    $value=$edits.Item(0).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  }
  $value.SetValue($file)
  $openCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1')
  $buttons=$dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants,$openCondition)
  if($buttons.Count -ne 1 -or -not $buttons.Item(0).Current.IsEnabled){throw 'Unique file picker confirmation unavailable'}
  $buttons.Item(0).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  exit 0
}
if($Action -eq 'Invoke' -and $windows.Count -gt 1 -and $ControlId -match '^[0-9]{1,8}$') {
  $wanted=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$ControlId)
  $windows=@($windows | Where-Object {
    $matches=$_.FindAll([System.Windows.Automation.TreeScope]::Descendants,$wanted)
    @($matches | Where-Object { $_.Current.IsEnabled -and -not $_.Current.IsOffscreen }).Count -eq 1
  })
}
if($windows.Count -ne 1){
  $details=@{windowCount=$observedWindowCount;selectedWindowCount=$windows.Count;nativeWindowCount=$nativeInventory.count;nativeWindows=$nativeInventory.windows;windows=@($windowRecords | ForEach-Object {$_.metadata})} | ConvertTo-Json -Depth 4 -Compress
  throw ('Expected one owned top-level window: '+$details)
}
$window=$windows[0]
if($Action -eq 'Inspect') {
  $all=$window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
  $controls=@($all | ForEach-Object {
    $info=$_.Current
    $invokePattern=$null
    @{name=$info.Name;id=$info.AutomationId;enabled=$info.IsEnabled;visible=(-not $info.IsOffscreen);className=$info.ClassName;controlTypeId=$info.ControlType.Id;canInvoke=$_.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$invokePattern)}
  })
  # Compare the UIA identifier, not provider-returned ControlType object identity.
  $buttons=@($controls | Where-Object {$_.controlTypeId -eq [System.Windows.Automation.ControlType]::Button.Id})
  @{processId=$TargetProcessId;startTimeUtc=$started;name=$window.Current.Name;enabled=$window.Current.IsEnabled;windowCount=$observedWindowCount;selectedWindowCount=$windows.Count;nativeWindowCount=$nativeInventory.count;nativeWindows=$nativeInventory.windows;windows=@($windowRecords | ForEach-Object {$_.metadata});selectedWindow=@($windowRecords | Where-Object {$_.element.Current.NativeWindowHandle -eq $window.Current.NativeWindowHandle} | ForEach-Object {$_.metadata})[0];buttons=$buttons;controls=$controls} | ConvertTo-Json -Depth 4 -Compress
  exit 0
}
if($Action -in @('Minimize','Activate')) {
  $pattern=$window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
  if($Action -eq 'Minimize') { $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Minimized) }
  else { $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal); $window.SetFocus() }
  exit 0
}
if($Action -eq 'Resize') {
  if($Width -lt 400 -or $Width -gt 2560 -or $Height -lt 300 -or $Height -gt 1600){throw 'Invalid owned window size'}
  $window.GetCurrentPattern([System.Windows.Automation.TransformPattern]::Pattern).Resize($Width,$Height)
  exit 0
}
if($Action -eq 'Close') {
  $pattern=$window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
  $pattern.Close()
  exit 0
}
if($ControlId) {
  if($ControlName -or $ControlId -notmatch '^[0-9]{1,8}$'){throw 'Exact numeric control id required'}
  $byName=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$ControlId)
} else {
  if([string]::IsNullOrWhiteSpace($ControlName)){throw 'Exact control name required'}
  $byName=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$ControlName)
}
$controls=$window.FindAll([System.Windows.Automation.TreeScope]::Descendants,$byName)
if($controls.Count -ne 1){throw 'Expected one named owned control'}
$control=$controls.Item(0)
if(-not $control.Current.IsEnabled -or $control.Current.IsOffscreen){throw 'Control unavailable'}
$control.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
