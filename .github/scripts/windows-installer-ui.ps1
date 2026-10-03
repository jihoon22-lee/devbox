# Native UI actions are limited to an exact process in an owned disposable fixture.
param(
  [Parameter(Mandatory=$true)][int]$TargetProcessId,
  [Parameter(Mandatory=$true)][string]$ExpectedExecutable,
  [Parameter(Mandatory=$true)][string]$ExpectedStartTimeUtc,
  [Parameter(Mandatory=$true)][string]$FixtureRoot,
  [Parameter(Mandatory=$true)][ValidateSet('Invoke','Close','Inspect')][string]$Action,
  [string]$ControlName
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$root=(Resolve-Path -LiteralPath $FixtureRoot).Path.TrimEnd('\')+'\'
$exe=(Resolve-Path -LiteralPath $ExpectedExecutable).Path
if(-not $exe.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)){throw 'Executable outside fixture'}
$process=Get-Process -Id $TargetProcessId
if(-not [string]::Equals($process.Path,$exe,[StringComparison]::OrdinalIgnoreCase)){throw 'Process executable mismatch'}
$started=$process.StartTime.ToUniversalTime().ToString('o')
if($started -ne $ExpectedStartTimeUtc){throw 'Process start time mismatch'}
$condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$TargetProcessId)
$windows=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$condition)
if($windows.Count -ne 1){throw 'Expected one owned top-level window'}
$window=$windows.Item(0)
if($Action -eq 'Inspect') {
  @{processId=$TargetProcessId;startTimeUtc=$started;name=$window.Current.Name;enabled=$window.Current.IsEnabled} | ConvertTo-Json -Compress
  exit 0
}
if($Action -eq 'Close') {
  $pattern=$window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
  $pattern.Close()
  exit 0
}
if([string]::IsNullOrWhiteSpace($ControlName)){throw 'Exact control name required'}
$byName=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$ControlName)
$controls=$window.FindAll([System.Windows.Automation.TreeScope]::Descendants,$byName)
if($controls.Count -ne 1){throw 'Expected one named owned control'}
$control=$controls.Item(0)
if(-not $control.Current.IsEnabled -or $control.Current.IsOffscreen){throw 'Control unavailable'}
$control.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
