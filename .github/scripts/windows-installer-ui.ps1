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
using System.Runtime.CompilerServices;
using System.Windows.Automation;
public static class DevboxInstallerAutomation {
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
$condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$TargetProcessId)
$windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)
if($WindowName) {
  $windows=@($windows | Where-Object {$_.Current.Name -ceq $WindowName})
}
if($Action -eq 'Inspect' -and $windows.Count -ne 1) {
  @{processId=$TargetProcessId;startTimeUtc=$started;windows=@($windows | ForEach-Object { @{name=$_.Current.Name;className=$_.Current.ClassName;enabled=$_.Current.IsEnabled} })} | ConvertTo-Json -Depth 4 -Compress
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
if($windows.Count -ne 1){throw 'Expected one owned top-level window'}
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
  @{processId=$TargetProcessId;startTimeUtc=$started;name=$window.Current.Name;enabled=$window.Current.IsEnabled;buttons=$buttons;controls=$controls} | ConvertTo-Json -Depth 4 -Compress
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
