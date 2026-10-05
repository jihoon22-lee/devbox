param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][ValidateSet('workspace','api-studio','knowledge','control-center')][string]$Product)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
function Get-ShortcutFailureObservation($Failure,[ValidateSet('ownership','activation','physical-identity','retained-payload','link-target','registered-link-launch')][string]$Stage) {
 $knownCommands=@('Assert-Unlinked','Assert-Identity','Get-Content','Get-Item','Get-FileHash','Split-Path','Join-Path','Add-Type','New-Object','ConvertFrom-Json','ConvertTo-Json','Where-Object')
 $missing=$null
 if($Failure.Exception -is [Management.Automation.CommandNotFoundException]) {
  $property=$Failure.Exception.PSObject.Properties['CommandName']
  if($null -ne $property -and $knownCommands -ccontains [string]$property.Value){$missing=[string]$property.Value}
 }
 $line=0
 if($null -ne $Failure.InvocationInfo){$line=$Failure.InvocationInfo.ScriptLineNumber}
 @{schemaVersion=1;issue=$(if($Failure.Exception -is [Management.Automation.CommandNotFoundException]){'shortcut_command_unavailable'}else{'shortcut_launch_failed'});stage=$Stage;command=$missing;line=$(if($line -ge 1 -and $line -le 2000){$line}else{$null})}
}
$shortcutStage='ownership'
try {
if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted'){throw 'Owned hosted shortcut fixture required'}
$rootPath=[IO.Path]::GetFullPath($Root).TrimEnd('\')
$parent=Split-Path -Parent $rootPath
if((Split-Path -Leaf $rootPath) -cne 'Suite UI Fixture' -or (Split-Path -Leaf $parent) -notmatch '^devbox-suite-delivery-[a-f0-9]{32}$' -or
 (Split-Path -Parent $parent) -ine [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')){throw 'Unowned shortcut fixture'}
function Assert-Unlinked([string]$Path) {
 $current=[IO.Path]::GetFullPath($Path)
 while($current){
  if((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked shortcut input'}
  $current=Split-Path -Parent $current
 }
}
Assert-Unlinked $rootPath
$owner=Get-Content -LiteralPath (Join-Path $parent 'user-flow-owner.json') -Raw | ConvertFrom-Json
$registration=Get-Content -LiteralPath (Join-Path $rootPath 'suite-registration.json') -Raw | ConvertFrom-Json
$key=[string]$registration.installationKey
if($key -notmatch '^[a-f0-9]{64}$' -or $owner.root -ine $rootPath -or $owner.installationKey -cne $key -or $registration.schemaVersion -ne 1){throw 'Shortcut fixture ownership changed'}
$shortcutStage='activation'
$activation=Get-Content -LiteralPath (Join-Path $rootPath 'devbox-activation.json') -Raw | ConvertFrom-Json
if($activation.phase -cne 'committed'){throw 'Committed shortcut launch required'}
$directory=Join-Path ([Environment]::GetFolderPath('Programs')) ('Devbox Suite ('+$key.Substring(0,12)+')')
if($registration.shortcutDirectory -ine $directory){throw 'Foreign shortcut directory'}
Assert-Unlinked $directory
$labels=@{'workspace'='Workspace';'api-studio'='API Studio';'knowledge'='Knowledge';'control-center'='Control Center'}
$name=$labels[$Product]+'.lnk'
$link=Join-Path $directory $name
Assert-Unlinked $link
$shortcutStage='physical-identity'
$plan=$registration.shortcutPlan
$entries=@($plan.files | Where-Object {$_.relative -ceq $name})
if($plan.schemaVersion -ne 1 -or $plan.installationKey -cne $key -or $entries.Count -ne 1){throw 'Invalid shortcut plan'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class OwnedShortcutIdentity {
 [StructLayout(LayoutKind.Sequential)] public struct Info {
  public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
  public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
 }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle file,out Info info);
 public static ulong[] Read(string path) {
  using(var file=CreateFileW(path,0x80,3,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   Info info;
   if(file.IsInvalid || !GetFileInformationByHandle(file,out info) || (info.Attributes&0x400)!=0) throw new InvalidOperationException("Shortcut identity unavailable");
   return new ulong[]{info.Volume,((ulong)info.IndexHigh<<32)|info.IndexLow};
  }
 }
}
'@
function Assert-Identity([string]$Path,$Expected) {
 $actual=[OwnedShortcutIdentity]::Read($Path)
 if($Expected.Count -ne 2 -or $actual[0] -ne [System.UInt64]$Expected[0] -or $actual[1] -ne [System.UInt64]$Expected[1]){throw 'Shortcut physical identity changed'}
}
Assert-Identity $rootPath $registration.rootIdentity
Assert-Identity $directory $registration.shortcutIdentity
Assert-Identity $directory $plan.rootIdentity
Assert-Identity $link $entries[0].identity
if((Get-Item -LiteralPath $link).Length -ne $entries[0].bytes -or (Get-FileHash -LiteralPath $link -Algorithm SHA256).Hash -ine $entries[0].sha256){throw 'Shortcut plan bytes changed'}
$shortcutStage='retained-payload'
$revision=[string]$registration.payloadRevision
if($revision -notmatch '^[a-f0-9]{64}$'){throw 'Shortcut revision invalid'}
$cached=Join-Path (Join-Path $rootPath 'setup') $revision
$payload=Join-Path $cached 'suite-payload.json'
$helper=Join-Path $cached 'devbox-suite-bootstrap.exe'
Assert-Unlinked $payload
Assert-Unlinked $helper
if((Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash -ine $revision){throw 'Shortcut payload changed'}
$package=Get-Content -LiteralPath $payload -Raw | ConvertFrom-Json
$asset=@($package.products | Where-Object {$_.id -ceq 'control-center'})[0].files | Where-Object {$_.name -ceq 'resources/suite/devbox-suite-bootstrap.exe'}
if((Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash -ine $asset.sha256){throw 'Shortcut helper changed'}
$shortcutStage='link-target'
$shell=New-Object -ComObject WScript.Shell
$shortcut=$null
try {
 $shortcut=$shell.CreateShortcut($link)
 $arguments='--open-'+$Product+' "'+$rootPath+'" "'+$payload+'"'
 if($shortcut.TargetPath -ine $helper -or $shortcut.Arguments -cne $arguments){throw 'Registered shortcut target changed'}
 $shortcutStage='registered-link-launch'
 # Execute the registered .lnk itself. Never substitute its parsed target.
 $start=[Diagnostics.ProcessStartInfo]::new()
 $start.FileName=$link
 $start.UseShellExecute=$true
 $process=[Diagnostics.Process]::Start($start)
 if($null -ne $process){$process.Dispose()}
 @{schemaVersion=1;product=$Product;shortcutVerified=$true;launchedRegisteredLink=$true} | ConvertTo-Json -Compress
} finally {
 if($null -ne $shortcut){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut)}
 [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
}

} catch {
 # Fixed source-level evidence only; preserve the original exception and never
 # serialize arbitrary command names, link targets, arguments or user paths.
 try {[Console]::Out.WriteLine((Get-ShortcutFailureObservation $_ $shortcutStage | ConvertTo-Json -Compress))}catch{}
 throw
}
