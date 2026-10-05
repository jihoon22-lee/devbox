param([string]$Driver=(Join-Path $PSScriptRoot 'windows-owned-installer-fault.ps1'))
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
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
