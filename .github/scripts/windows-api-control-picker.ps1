param([int]$OwnerProcessId, [string]$SelectedFile)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Hosted fixture required' }
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$owner = [Diagnostics.Process]::GetProcessById($OwnerProcessId)
if ($owner.MainModule.FileName -notmatch 'api-s03-[0-9a-f-]+\.exe$') { throw 'Not the owned API fixture' }
$deadline = [DateTime]::UtcNow.AddSeconds(20)
while ([DateTime]::UtcNow -lt $deadline) {
  $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $OwnerProcessId))
  foreach ($window in $windows) {
    if ($window.Current.ClassName -ne '#32770') { continue }
    $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $fileName = $null; $open = $null
    foreach ($element in $elements) {
      if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit -and $element.Current.Name -eq 'File name:') { $fileName = $element }
      if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $element.Current.Name -eq 'Open') { $open = $element }
    }
    if ($fileName -and $open) {
      $fileName.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($SelectedFile)
      $open.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
      exit 0
    }
  }
  Start-Sleep -Milliseconds 100
}
throw 'Owned file picker did not expose its filename/Open controls'
