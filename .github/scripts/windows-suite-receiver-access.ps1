param([Parameter(Mandatory)][string]$Root,[Parameter(Mandatory)][string]$Image,[Parameter(Mandatory)][string]$ExpectedDigest,[Parameter(Mandatory)][ValidateSet('Deny','Restore')][string]$Action)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or $env:RUNNER_OS -cne 'Windows' -or $env:GITHUB_REPOSITORY -cne 'jihoon22-lee/devbox' -or $env:GITHUB_RUN_ID -notmatch '^\d+$'){throw 'Receiver fixture requires disposable hosted Windows; never spoof CI'}
foreach($module in @('Microsoft.PowerShell.Management','Microsoft.PowerShell.Utility','Microsoft.PowerShell.Security')){Import-Module ([IO.Path]::Combine($PSHOME,'Modules',$module,$module+'.psd1')) -ErrorAction Stop}
$rootPath=(Microsoft.PowerShell.Management\Resolve-Path -LiteralPath $Root).Path.TrimEnd('\')
$tempPath=(Microsoft.PowerShell.Management\Resolve-Path -LiteralPath $env:RUNNER_TEMP).Path.TrimEnd('\')
if(-not $rootPath.StartsWith($tempPath+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Receiver root outside runner temp'}
if((Microsoft.PowerShell.Management\Split-Path -Leaf $rootPath) -cne 'Suite UI Fixture' -or (Microsoft.PowerShell.Management\Split-Path -Leaf (Microsoft.PowerShell.Management\Split-Path -Parent $rootPath)) -notmatch '^devbox-suite-delivery-[a-f0-9]{32}$'){throw 'Unowned installation'}
$manifest=Microsoft.PowerShell.Management\Get-Content -LiteralPath (Microsoft.PowerShell.Management\Join-Path $rootPath 'devbox-installation.json') -Raw | Microsoft.PowerShell.Utility\ConvertFrom-Json
if($manifest.generation -cnotmatch '^[a-zA-Z0-9_-]+$'){throw 'Receiver generation invalid'}
$members=@($manifest.members | Where-Object {$_.product -ceq 'knowledge'})
if($members.Count -ne 1 -or $members[0].executable -cne ('generations/'+$manifest.generation+'/products/knowledge/devbox-knowledge.exe')){throw 'Receiver member invalid'}
$file=(Microsoft.PowerShell.Management\Resolve-Path -LiteralPath $Image).Path
if(-not [string]::Equals($file,[IO.Path]::GetFullPath((Microsoft.PowerShell.Management\Join-Path $rootPath $members[0].executable)),[StringComparison]::OrdinalIgnoreCase)){throw 'Receiver path mismatch'}
$item=Microsoft.PowerShell.Management\Get-Item -LiteralPath $file
if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Receiver file has reparse point'}
for($item=$item.Directory;$null -ne $item;$item=$item.Parent){if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Receiver path has reparse point'}}
if($ExpectedDigest -notmatch '^[a-f0-9]{64}$' -or $members[0].sha256 -cne $ExpectedDigest -or (Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedDigest){throw 'Receiver digest mismatch'}
$receipt=Microsoft.PowerShell.Management\Join-Path $rootPath '.handoff-receiver-access.json'
if((Microsoft.PowerShell.Management\Test-Path -LiteralPath $receipt) -and ((Microsoft.PowerShell.Management\Get-Item -LiteralPath $receipt).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Receiver receipt has reparse point'}
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
function Restore-ReceiverAccess($state) {
  if($state.image -cne $file -or $state.digest -cne $ExpectedDigest -or $state.sid -cne $sid.Value -or $state.runId -cne $env:GITHUB_RUN_ID){throw 'Receiver restoration ownership mismatch'}
  $acl=Microsoft.PowerShell.Security\Get-Acl -LiteralPath $file
  $acl.SetSecurityDescriptorSddlForm($state.sddl,[Security.AccessControl.AccessControlSections]::Access)
  Microsoft.PowerShell.Security\Set-Acl -LiteralPath $file -AclObject $acl
  if((Microsoft.PowerShell.Security\Get-Acl -LiteralPath $file).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access) -cne $state.sddl){throw 'Receiver ACL restoration mismatch'}
  if((Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedDigest){throw 'Restored receiver bytes changed'}
  Microsoft.PowerShell.Management\Remove-Item -LiteralPath $receipt
}
if($Action -ceq 'Restore') {
  Restore-ReceiverAccess (Microsoft.PowerShell.Management\Get-Content -LiteralPath $receipt -Raw | Microsoft.PowerShell.Utility\ConvertFrom-Json)
} else {
  if(Microsoft.PowerShell.Management\Test-Path -LiteralPath $receipt){throw 'Receiver restoration already pending'}
  $acl=Microsoft.PowerShell.Security\Get-Acl -LiteralPath $file
  $state=@{image=$file;digest=$ExpectedDigest;sid=$sid.Value;runId=$env:GITHUB_RUN_ID;sddl=$acl.GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)}
  $bytes=[Text.Encoding]::UTF8.GetBytes(($state | Microsoft.PowerShell.Utility\ConvertTo-Json -Compress))
  $stream=[IO.File]::Open($receipt,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
  try {$stream.Write($bytes,0,$bytes.Length)} finally {$stream.Dispose()}
  try {
    $rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,[Security.AccessControl.FileSystemRights]::ExecuteFile,[Security.AccessControl.AccessControlType]::Deny)
    $acl.AddAccessRule($rule)
    Microsoft.PowerShell.Security\Set-Acl -LiteralPath $file -AclObject $acl
    $denied=@((Microsoft.PowerShell.Security\Get-Acl -LiteralPath $file).GetAccessRules($true,$false,[Security.Principal.SecurityIdentifier]) | Where-Object {$_.IdentityReference.Value -ceq $sid.Value -and $_.AccessControlType -eq 'Deny' -and ($_.FileSystemRights -band [Security.AccessControl.FileSystemRights]::ExecuteFile)})
    if($denied.Count -ne 1){throw 'Receiver execute denial not observed'}
    if((Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedDigest){throw 'Receiver bytes changed'}
  } catch {
    $originalFailure=$_
    try {Restore-ReceiverAccess $state} catch {[Console]::Error.WriteLine('Receiver access rollback remains pending')}
    throw $originalFailure
  }
}
Microsoft.PowerShell.Utility\Write-Output ('Owned receiver access '+$Action+': PASS')
