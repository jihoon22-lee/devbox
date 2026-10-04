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
Write-Output 'Owned installer nonfocusable UIA capture boundary: PASS (no native input)'
