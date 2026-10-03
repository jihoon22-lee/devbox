param([Parameter(Mandatory=$true)][int]$AgentProcessId,[Parameter(Mandatory=$true)][string]$ExpectedExecutable,[Parameter(Mandatory=$true)][string]$ExpectedStartTimeUtc)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OwnedTrayInput {
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
}
'@
function AssertAgent {
 $p=[Diagnostics.Process]::GetProcessById($AgentProcessId)
 if($p.MainModule.FileName -ne $ExpectedExecutable -or $p.StartTime.ToUniversalTime().ToString('o') -ne $ExpectedStartTimeUtc){throw 'Owned Agent identity changed'}
}
function FindNamed($name) {
 $condition=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$name)
 return ,([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition))
}
function ClickElement($element,$right=$false) {
 $rect=$element.Current.BoundingRectangle
 if($rect.IsEmpty -or $rect.Width -le 0 -or $rect.Height -le 0){throw 'Tray control is not visible'}
 AssertAgent
 [void][OwnedTrayInput]::SetCursorPos([int]($rect.Left+$rect.Width/2),[int]($rect.Top+$rect.Height/2))
 if($right){$down=0x0008;$up=0x0010}else{$down=0x0002;$up=0x0004}
 [OwnedTrayInput]::mouse_event($down,0,0,0,[UIntPtr]::Zero)
 [OwnedTrayInput]::mouse_event($up,0,0,0,[UIntPtr]::Zero)
}
AssertAgent
$icons=FindNamed 'Devbox 백그라운드 서비스'
if($icons.Count -eq 0){
 $overflow=@()
 foreach($name in @('Hidden icon menu','Show hidden icons','숨겨진 아이콘 표시')) {
  foreach($control in (FindNamed $name)) {
   if($control.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and ([Diagnostics.Process]::GetProcessById($control.Current.ProcessId)).ProcessName -eq 'explorer'){$overflow+=,$control}
  }
 }
 if($overflow.Count -ne 1){throw 'Exact Explorer tray overflow button missing or ambiguous'}
 ClickElement $overflow[0]
 Start-Sleep -Milliseconds 300
 $icons=FindNamed 'Devbox 백그라운드 서비스'
}
if($icons.Count -ne 1){throw 'Exact visible owned Suite tray icon required; no global tray fallback'}
$icon=$icons.Item(0)
$explorer=[Diagnostics.Process]::GetProcessById($icon.Current.ProcessId)
if($explorer.ProcessName -ne 'explorer'){throw 'Suite tray icon must be in Windows Explorer notification area'}
ClickElement $icon $true
$deadline=[DateTime]::UtcNow.AddSeconds(5)
do {
 $items=FindNamed '백그라운드 작업 모두 멈추고 종료'
 if($items.Count -eq 1){break}
 Start-Sleep -Milliseconds 100
}while([DateTime]::UtcNow -lt $deadline)
if($items.Count -ne 1){throw 'Exact Suite tray shutdown menu missing or ambiguous'}
$item=$items.Item(0)
if($item.Current.ProcessId -ne $AgentProcessId){throw 'Tray menu does not belong to owned Agent process'}
ClickElement $item
