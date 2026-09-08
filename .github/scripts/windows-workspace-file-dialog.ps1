# Drive only a chooser belonging to the exact disposable Workspace executable.
param(
    [Parameter(Mandatory = $true)][int]$TargetProcessId,
    [Parameter(Mandatory = $true)][string]$ExpectedExecutable,
    [Parameter(Mandatory = $true)][string]$FixtureRoot,
    [Parameter(Mandatory = $true)][ValidateSet('Open', 'Cancel')][string]$Action,
    [string]$SelectedFile
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public static class WorkspaceFixturePath {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint GetFinalPathNameByHandleW(SafeFileHandle handle, StringBuilder path, uint capacity, uint flags);
    public static string Canonical(string path) {
        using (var handle = CreateFileW(path, 0, 7, IntPtr.Zero, 3, 0x02000000, IntPtr.Zero)) {
            if (handle.IsInvalid) throw new InvalidOperationException("Fixture path unavailable");
            var result = new StringBuilder(32768);
            uint size = GetFinalPathNameByHandleW(handle, result, 32768, 0);
            if (size == 0 || size >= 32768) throw new InvalidOperationException("Fixture path unavailable");
            return result.ToString();
        }
    }
}
'@
$fixturePath = [WorkspaceFixturePath]::Canonical($FixtureRoot).TrimEnd('\') + '\'
$expectedPath = [WorkspaceFixturePath]::Canonical($ExpectedExecutable)
if (-not $expectedPath.StartsWith($fixturePath, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The executable is outside the owned fixture.'
}
$ownedProcess = Get-Process -Id $TargetProcessId -ErrorAction Stop
$started = $ownedProcess.StartTime.ToUniversalTime().Ticks
function Assert-OwnedProcess {
    $current = Get-Process -Id $TargetProcessId -ErrorAction Stop
    if ($current.StartTime.ToUniversalTime().Ticks -ne $started -or
        -not [string]::Equals([WorkspaceFixturePath]::Canonical($current.Path), $expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'The owned Workspace process changed.'
    }
}
Assert-OwnedProcess
if ($Action -eq 'Open') {
    if ([string]::IsNullOrWhiteSpace($SelectedFile)) { throw 'A selected fixture file is required.' }
    $selectedPath = [WorkspaceFixturePath]::Canonical($SelectedFile)
    if (-not $selectedPath.StartsWith($fixturePath, [StringComparison]::OrdinalIgnoreCase) -or
        -not [IO.File]::Exists($selectedPath)) { throw 'The selected file is outside the owned fixture.' }
}
$processCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $TargetProcessId)
$classCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ClassNameProperty, '#32770')
$dialogCondition = [System.Windows.Automation.AndCondition]::new($processCondition, $classCondition)
$deadline = [DateTime]::UtcNow.AddSeconds(15)
$dialog = $null
while ([DateTime]::UtcNow -lt $deadline) {
    Assert-OwnedProcess
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
        [System.Windows.Automation.TreeScope]::Children, $dialogCondition)
    if ($windows.Count -gt 1) { throw 'More than one owned dialog is open.' }
    if ($windows.Count -eq 1) { $dialog = $windows[0]; break }
    Start-Sleep -Milliseconds 100
}
if ($null -eq $dialog) { throw 'The owned file chooser did not appear.' }
function Find-Control([string]$automationId) {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::AutomationIdProperty, $automationId)
    return $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
}
if ($Action -eq 'Open') {
    $filename = Find-Control 'FileNameControlHost'
    if ($null -eq $filename) { $filename = Find-Control '1148' }
    if ($null -eq $filename) { throw 'The owned chooser filename control is unavailable.' }
    if ($filename.Current.ControlType -ne [System.Windows.Automation.ControlType]::Edit) {
        $editCondition = [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.ControlType]::Edit)
        $filename = $filename.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $editCondition)
    }
    if ($null -eq $filename) { throw 'The owned chooser filename editor is unavailable.' }
    $valuePattern = $filename.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    Assert-OwnedProcess
    $dialogPath = if ($selectedPath.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
        '\\' + $selectedPath.Substring(8)
    } elseif ($selectedPath.StartsWith('\\?\', [StringComparison]::Ordinal)) {
        $selectedPath.Substring(4)
    } else { $selectedPath }
    $valuePattern.SetValue($dialogPath)
}
$buttonId = if ($Action -eq 'Open') { '1' } else { '2' }
$button = Find-Control $buttonId
if ($null -eq $button -or -not $button.Current.IsEnabled) { throw 'The owned chooser action is unavailable.' }
$invokePattern = $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
Assert-OwnedProcess
$invokePattern.Invoke()
@{ action = $Action; ownedProcessMatched = $true } | ConvertTo-Json -Compress
