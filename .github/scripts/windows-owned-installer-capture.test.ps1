param([string]$Driver=(Join-Path $PSScriptRoot 'windows-owned-installer-fault.ps1'))
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
# Windows PowerShell 5.1 reads non-ASCII script literals correctly only with UTF-8 BOM.
$scriptBytes=[IO.File]::ReadAllBytes($PSCommandPath)
if($scriptBytes.Length -lt 3 -or $scriptBytes[0] -ne 239 -or $scriptBytes[1] -ne 187 -or $scriptBytes[2] -ne 191){throw 'Owned capture test requires UTF-8 BOM for Windows PowerShell 5.1'}
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($Driver,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Owned capture driver syntax invalid'}
$native=$ast.Find({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('public static class OwnedInstallerCapture')},$true)
if($null -eq $native){throw 'Owned capture native boundary missing'}
Add-Type -TypeDefinition $native.Value

$definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Focus-OwnedCaptureWindow'},$true)
if($null -eq $definition){throw 'Owned capture focus boundary missing'}
. ([scriptblock]::Create($definition.Extent.Text))
$window=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=17;NativeWindowHandle=42;IsKeyboardFocusable=$false}}
$window | Add-Member -MemberType ScriptMethod -Name SetFocus -Value {throw 'Target element cannot receive focus.'}
$baselineFailed=$false
try{$window.SetFocus()}catch{$baselineFailed=$true}
if(-not $baselineFailed){throw 'Nonfocusable provider baseline missing'}
$script:calls=0
Focus-OwnedCaptureWindow $window 17 {param($handle,$owner) if($handle -ne 42 -or $owner -ne 17){throw 'Wrong owned HWND'};$script:calls++}
if($script:calls -ne 1){throw 'Native foreground action must happen once'}
foreach($case in @(@{ProcessId=18;NativeWindowHandle=42},@{ProcessId=17;NativeWindowHandle=0})) {
 $rejected=$false
 try{Focus-OwnedCaptureWindow ([pscustomobject]@{Current=[pscustomobject]$case}) 17 {$script:calls++}}catch{$rejected=$true}
 if(-not $rejected -or $script:calls -ne 1){throw 'Changed ownership must reject before foreground action'}
}
$failed=$false
try{Focus-OwnedCaptureWindow $window 17 {throw 'Owned capture foreground unavailable'}}catch{$failed=$true}
if(-not $failed){throw 'Failed native foreground must prevent capture'}
$selection=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Select-OwnedCaptureWindow'},$true)
if($null -eq $selection){throw 'Product capture selection missing'}
. ([scriptblock]::Create($selection.Extent.Text))
function CaptureFixture($handle,$class) { [pscustomobject]@{Current=[pscustomobject]@{NativeWindowHandle=$handle;ClassName=$class}} }
$main=CaptureFixture 42 'Tauri Window'
$helper=CaptureFixture 43 'Tao Thread Event Target'
$sic=CaptureFixture 44 ('com.devbox.v08.workspace.i'+('a'*64)+'-sic')
$inventory=@([pscustomobject]@{handle=42;className='Tauri Window';visible=$true;topLevel=$true},[pscustomobject]@{handle=43;className='Tao Thread Event Target';visible=$true;topLevel=$true},[pscustomobject]@{handle=44;className=$sic.Current.ClassName;visible=$true;topLevel=$true})
if((Select-OwnedCaptureWindow @($main,$helper,$sic) $inventory 'workspace').Current.NativeWindowHandle -ne 42){throw 'Known product helpers must not block actual main capture'}
foreach($extra in @([pscustomobject]@{handle=45;className='Tauri Window';visible=$true;topLevel=$true},[pscustomobject]@{handle=45;className='#32770';visible=$true;topLevel=$true},[pscustomobject]@{handle=45;className='foreign';visible=$true;topLevel=$true})) {
 $rejected=$false
 try{Select-OwnedCaptureWindow @($main,$helper,$sic) @($inventory+$extra) 'workspace' | Out-Null}catch{$rejected=$true}
 if(-not $rejected){throw 'Second visible product/modal root must reject capture'}
}
$rejected=$false
try{Select-OwnedCaptureWindow @($main,$helper,$sic) $inventory '' | Out-Null}catch{$rejected=$true}
if(-not $rejected){throw 'Generic installer capture must retain unique-root guard'}
Write-Output 'Owned installer nonfocusable UIA capture boundary: PASS (no native input)'

# Pure tray evidence projection; never enumerate the local desktop or send input.
$trayTokens=$null;$trayErrors=$null
$trayAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'windows-agent-tray-ui.ps1'),[ref]$trayTokens,[ref]$trayErrors)
if($trayErrors.Count){throw 'Tray diagnostic parse failed'}
$projection=$trayAst.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Convert-TrayObservation'},$true)
if($null -eq $projection){throw 'Tray evidence projection missing'}
. ([scriptblock]::Create($projection.Extent.Text))
$info=[pscustomobject]@{ProcessId=17;Name='private document title';AutomationId='private-path';ClassName='ToolbarWindow32';ControlType=[pscustomobject]@{Id=50000};IsOffscreen=$false;IsEnabled=$true;NativeWindowHandle=42}
$projected=Convert-TrayObservation $info 2
if($null -ne $projected.name -or $null -ne $projected.automationId -or -not $projected.namePresent -or $projected.depth -ne 2 -or $projected.processId -ne 17){throw 'Tray projection must omit arbitrary names/IDs'}
$info.Name='Show hidden icons';$info.AutomationId='SystemTray.OverflowButton'
$projected=Convert-TrayObservation $info 3
if($projected.name -cne $info.Name -or $projected.automationId -cne $info.AutomationId){throw 'Fixed tray identity must remain observable'}

$info.AutomationId='1500'
if((Convert-TrayObservation $info 1).automationId -cne '1500'){throw 'Bounded numerical native tray ID must remain visible'}
$info.AutomationId='123456789'
if($null -ne (Convert-TrayObservation $info 1).automationId){throw 'Oversized numerical tray ID must be omitted'}

foreach($functionName in @('Test-TrayOverflowInfo','Select-TrayOverflow')) {
 $definition=$trayAst.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $functionName},$true)
 . ([scriptblock]::Create($definition.Extent.Text))
}
$info.ClassName='SystemTray.NormalButton';$info.AutomationId='SystemTrayIcon'
foreach($label in @(' Show Hidden Icons ','HIDDEN ICON MENU',' 숨겨진 아이콘 표시 ')) {
 $info.Name=$label
 if(-not (Test-TrayOverflowInfo $info)){throw 'Normalized known overflow label must match'}
}
$info.Name='IME'
if(Test-TrayOverflowInfo $info){throw 'Same-class same-ID IME must not match'}
$info.Name='Show hidden icons';$info.AutomationId='Other'
if(Test-TrayOverflowInfo $info){throw 'Wrong modern automation ID must reject'}
$info.AutomationId='SystemTrayIcon'
$candidate=[pscustomobject]@{Current=$info}
foreach($candidates in @(@(),@($candidate,$candidate))) {
 $rejected=$false
 try{Select-TrayOverflow $candidates | Out-Null}catch{$rejected=$true}
 if(-not $rejected){throw 'Missing or ambiguous overflow candidate must reject'}
}
if((Select-TrayOverflow @($candidate)).Current -ne $info){throw 'Unique known overflow candidate required'}
$info.Name='English system label'
if((Convert-TrayObservation $info 3).name -cne $info.Name){throw 'Modern system tray label must remain observable'}
$info.ClassName='Taskbar.TaskListButtonAutomationPeer'
if($null -ne (Convert-TrayObservation $info 3).name){throw 'Application taskbar label must remain omitted'}

$rootPredicate=$trayAst.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-TrayRootClass'},$true)
. ([scriptblock]::Create($rootPredicate.Extent.Text))
if(-not (Test-TrayRootClass 'TopLevelWindowForOverflowXamlIsland')){throw 'Observed modern Explorer overflow root must be recognized'}
foreach($foreign in @('Tauri Window','foreign','toplevelwindowforoverflowxamlisland')) {
 if(Test-TrayRootClass $foreign){throw 'Unknown or changed root class must reject'}
}
