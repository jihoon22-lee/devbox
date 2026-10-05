param([Parameter(Mandatory=$true)][int]$AgentProcessId,[Parameter(Mandatory=$true)][string]$ExpectedExecutable,[Parameter(Mandatory=$true)][string]$ExpectedStartTimeUtc)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted'){throw 'Tray input requires the disposable hosted fixture'}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -ReferencedAssemblies UIAutomationClient,UIAutomationTypes -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Windows.Automation;
using System.Collections.Generic;
using System.Text;
public static class OwnedTrayInput {
 [MethodImpl(MethodImplOptions.NoInlining)]
 public static void Initialize() { ClientSettings.RegisterClientSideProviders(new ClientSideProviderDescription[0]); }
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr window,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder name,int capacity);
 delegate bool Callback(IntPtr window,IntPtr parameter);
 [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback,IntPtr parameter);
 public sealed class WindowInfo { public long handle;public uint processId;public string className;public bool visible; }
 public static string TopClass(IntPtr window,uint expectedOwner) {
  var root=GetAncestor(window,2);uint owner;GetWindowThreadProcessId(root,out owner);
  if(root==IntPtr.Zero || owner!=expectedOwner || !IsWindowVisible(root))throw new InvalidOperationException("Explorer tray root changed");
  var name=new StringBuilder(100);GetClassName(root,name,name.Capacity);return name.ToString();
 }
 public static WindowInfo[] Observe(uint[] owners) {
  var windows=new List<WindowInfo>();
  Callback callback=delegate(IntPtr window,IntPtr parameter) {
   uint owner;GetWindowThreadProcessId(window,out owner);
   if(Array.IndexOf(owners,owner)>=0 && windows.Count<32) {
    var name=new StringBuilder(100);GetClassName(window,name,name.Capacity);
    windows.Add(new WindowInfo{handle=window.ToInt64(),processId=owner,className=name.ToString(),visible=IsWindowVisible(window)});
   }
   return true;
  };
  if(!EnumWindows(callback,IntPtr.Zero))throw new InvalidOperationException("Owned Explorer inventory unavailable");
  return windows.ToArray();
 }
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
}
'@
[OwnedTrayInput]::Initialize()
function AssertAgent {
 $p=[Diagnostics.Process]::GetProcessById($AgentProcessId)
 if($p.MainModule.FileName -ne $ExpectedExecutable -or $p.StartTime.ToUniversalTime().ToString('o') -ne $ExpectedStartTimeUtc){throw 'Owned Agent identity changed'}
}
function Test-TrayRootClass([string]$ClassName) {
 return $ClassName -cin @('Shell_TrayWnd','Shell_SecondaryTrayWnd','NotifyIconOverflowWindow','TopLevelWindowForOverflowXamlIsland')
}
function AssertExplorer($element) {
 $info=$element.Current
 $process=[Diagnostics.Process]::GetProcessById($info.ProcessId)
 if($process.MainModule.FileName -ine (Join-Path $env:WINDIR 'explorer.exe') -or $process.SessionId -ne ([Diagnostics.Process]::GetProcessById($AgentProcessId)).SessionId -or $info.IsOffscreen -or -not $info.IsEnabled){throw 'Visible session-owned Explorer control required'}
 $current=$element
 for($depth=0;$depth -lt 32 -and $null -ne $current;$depth++) {
  $handle=[IntPtr]$current.Current.NativeWindowHandle
  if($handle -ne [IntPtr]::Zero) {
   $owner=[uint32]0
   [void][OwnedTrayInput]::GetWindowThreadProcessId($handle,[ref]$owner)
   if($owner -ne $info.ProcessId -or -not [OwnedTrayInput]::IsWindowVisible($handle)){throw 'Explorer native control changed'}
   if(-not (Test-TrayRootClass ([OwnedTrayInput]::TopClass($handle,[uint32]$info.ProcessId)))){throw 'Explorer control outside tray root'}
   return
  }
  $current=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($current)
 }
 throw 'Explorer native ancestor unavailable'
}
function Test-TrayOverflowInfo($Info) {
 if($Info.IsOffscreen -or -not $Info.IsEnabled -or $Info.ControlType.Id -ne 50000){return $false}
 $label=[string]$Info.Name
 $known=$false
 foreach($name in @('Hidden icon menu','Show hidden icons','숨겨진 아이콘 표시')) {
  if([string]::Equals($label.Trim(),$name,[StringComparison]::OrdinalIgnoreCase)){$known=$true;break}
 }
 if(-not $known){return $false}
 if($Info.ClassName -ceq 'SystemTray.NormalButton'){return $Info.AutomationId -ceq 'SystemTrayIcon'}
 return $Info.ClassName -ceq 'Button'
}
function Select-TrayOverflow($Candidates) {
 $matches=@($Candidates | Where-Object {Test-TrayOverflowInfo $_.Current})
 if($matches.Count -ne 1){throw 'Exact Explorer tray overflow button missing or ambiguous'}
 return $matches[0]
}
function Find-TrayOverflow {
 $session=([Diagnostics.Process]::GetProcessById($AgentProcessId)).SessionId
 $explorers=@([Diagnostics.Process]::GetProcessesByName('explorer') | Where-Object {$_.SessionId -eq $session -and $_.MainModule.FileName -ieq (Join-Path $env:WINDIR 'explorer.exe')})
 $native=@([OwnedTrayInput]::Observe([uint32[]]@($explorers | ForEach-Object {$_.Id})))
 $queue=[Collections.Generic.Queue[object]]::new();$candidates=@();$visited=0
 foreach($window in $native | Where-Object {$_.visible -and $_.className -cin @('Shell_TrayWnd','Shell_SecondaryTrayWnd')}) {
  $queue.Enqueue(@{element=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$window.handle);owner=$window.processId;depth=0})
 }
 while($queue.Count -gt 0 -and $visited -lt 128) {
  $next=$queue.Dequeue();$element=$next.element;$info=$element.Current;$visited++
  if($info.ProcessId -ne $next.owner){continue}
  if(Test-TrayOverflowInfo $info){AssertExplorer $element;$candidates+=,$element}
  if($next.depth -ge 6){continue}
  $child=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetFirstChild($element)
  for($index=0;$index -lt 16 -and $null -ne $child;$index++) {
   if($queue.Count -ge 128){throw 'Tray subtree observation limit exceeded'}
   $queue.Enqueue(@{element=$child;owner=$next.owner;depth=$next.depth+1})
   $child=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetNextSibling($child)
  }
  if($null -ne $child){throw 'Tray sibling observation limit exceeded'}
 }
 if($queue.Count -gt 0){throw 'Tray subtree observation limit exceeded'}
 return Select-TrayOverflow $candidates
}
function Convert-TrayObservation($Info,[int]$Depth) {
 $known=@('Hidden icon menu','Show hidden icons','숨겨진 아이콘 표시','Devbox 백그라운드 서비스','백그라운드 작업 모두 멈추고 종료')
 $ids=@('SystemTrayIcon','SystemTray.OverflowButton','NotificationChevron','OverflowButton','Chevron')
 @{depth=$Depth;processId=$Info.ProcessId;name=$(if($known -ccontains $Info.Name -or ($Info.ClassName -ceq 'SystemTray.NormalButton' -and $Info.AutomationId -ceq 'SystemTrayIcon' -and $Info.Name.Length -le 128)){$Info.Name}else{$null});namePresent=[bool]$Info.Name;automationId=$(if($ids -ccontains $Info.AutomationId -or $Info.AutomationId -cmatch '^[0-9]{1,8}$'){$Info.AutomationId}else{$null});automationIdPresent=[bool]$Info.AutomationId;className=$Info.ClassName.Substring(0,[Math]::Min(100,$Info.ClassName.Length));controlTypeId=$Info.ControlType.Id;offscreen=$Info.IsOffscreen;enabled=$Info.IsEnabled;nativeHandle=$Info.NativeWindowHandle}
}
function Save-TrayFailureObservation {
 AssertAgent
 $session=([Diagnostics.Process]::GetProcessById($AgentProcessId)).SessionId
 $explorers=@([Diagnostics.Process]::GetProcessesByName('explorer') | Where-Object {$_.SessionId -eq $session -and $_.MainModule.FileName -ieq (Join-Path $env:WINDIR 'explorer.exe')})
 $native=@([OwnedTrayInput]::Observe([uint32[]]@($explorers | ForEach-Object {$_.Id})))
 $observation=@{schemaVersion=1;status='FAIL';uiaAssembly=[System.Windows.Automation.ClientSettings].Assembly.FullName;explorerProcesses=$explorers.Count;nativeWindows=$native;subtree=@();subtreeUnavailable=$false}
 $directory=Join-Path (Get-Location).ProviderPath 'product-foundation-evidence'
 [void][IO.Directory]::CreateDirectory($directory)
 $output=Join-Path $directory 'agent-tray-first-failure.json'
 if(Test-Path -LiteralPath $output){return}
 function SaveObservation { [IO.File]::WriteAllText($output,($observation | ConvertTo-Json -Depth 6 -Compress),[Text.UTF8Encoding]::new($false)) }
 SaveObservation
 try {
  $queue=[Collections.Generic.Queue[object]]::new()
  foreach($window in $native | Where-Object {$_.visible -and (Test-TrayRootClass $_.className)}) {
   $element=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$window.handle)
   if($element.Current.ProcessId -ne $window.processId){throw 'Explorer root changed'}
   $queue.Enqueue(@{element=$element;depth=0;owner=$window.processId})
  }
  while($queue.Count -gt 0 -and $observation.subtree.Count -lt 128) {
   $next=$queue.Dequeue();$element=$next.element;$info=$element.Current
   if($info.ProcessId -ne $next.owner){continue}
   $observation.subtree+=@(Convert-TrayObservation $info $next.depth)
   SaveObservation
   if($next.depth -ge 6){continue}
   $child=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetFirstChild($element)
   for($index=0;$index -lt 16 -and $null -ne $child;$index++) {
    if($queue.Count -ge 128){break}
    $queue.Enqueue(@{element=$child;depth=$next.depth+1;owner=$next.owner})
    $child=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetNextSibling($child)
   }
  }
 } catch {$observation.subtreeUnavailable=$true;SaveObservation}
}
function FindNamed($name) {
 $condition=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$name)
 return ,([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition))
}
function ClickElement($element,$right=$false) {
 $rect=$element.Current.BoundingRectangle
 if($rect.IsEmpty -or $rect.Width -le 0 -or $rect.Height -le 0){throw 'Tray control is not visible'}
 AssertAgent
 if($element.Current.ProcessId -ne $AgentProcessId){AssertExplorer $element}
 [void][OwnedTrayInput]::SetCursorPos([int]($rect.Left+$rect.Width/2),[int]($rect.Top+$rect.Height/2))
 if($right){$down=0x0008;$up=0x0010}else{$down=0x0002;$up=0x0004}
 [OwnedTrayInput]::mouse_event($down,0,0,0,[UIntPtr]::Zero)
 [OwnedTrayInput]::mouse_event($up,0,0,0,[UIntPtr]::Zero)
}
try {
AssertAgent
$icons=FindNamed 'Devbox 백그라운드 서비스'
if($icons.Count -eq 0){
 $overflow=Find-TrayOverflow
 ClickElement $overflow
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

} catch {
 $original=$_
 try{Save-TrayFailureObservation}catch{}
 throw $original
}
