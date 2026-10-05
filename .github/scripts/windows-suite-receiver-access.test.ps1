param([string]$ScriptDirectory=$PSScriptRoot)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or $env:RUNNER_OS -cne 'Windows' -or $env:GITHUB_REPOSITORY -cne 'jihoon22-lee/devbox' -or $env:GITHUB_RUN_ID -notmatch '^\d+$'){throw 'Receiver fixture requires disposable hosted Windows; never spoof CI'}
foreach($module in @('Microsoft.PowerShell.Management','Microsoft.PowerShell.Utility','Microsoft.PowerShell.Security')){Import-Module ([IO.Path]::Combine($PSHOME,'Modules',$module,$module+'.psd1')) -ErrorAction Stop}
$scratch=Microsoft.PowerShell.Management\Join-Path $env:RUNNER_TEMP ('devbox-suite-delivery-'+[guid]::NewGuid().ToString('N'))
$root=Microsoft.PowerShell.Management\Join-Path $scratch 'Suite UI Fixture'
$relative='generations/g1/products/knowledge/devbox-knowledge.exe'
$image=Microsoft.PowerShell.Management\Join-Path $root $relative
$helper=Microsoft.PowerShell.Management\Join-Path $ScriptDirectory 'windows-suite-receiver-access.ps1'
$pin=$null
try {
  Microsoft.PowerShell.Management\New-Item -ItemType Directory -Path (Microsoft.PowerShell.Management\Split-Path -Parent $image) | Out-Null
  # A disposable console executable exercises Windows launch access without product UI.
  Microsoft.PowerShell.Management\Copy-Item -LiteralPath (Microsoft.PowerShell.Management\Join-Path $env:WINDIR 'System32\whoami.exe') -Destination $image
  $digest=(Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant()
  @{generation='g1';members=@(@{product='knowledge';executable=$relative;sha256=$digest})} | Microsoft.PowerShell.Utility\ConvertTo-Json -Depth 5 | Microsoft.PowerShell.Management\Set-Content -LiteralPath (Microsoft.PowerShell.Management\Join-Path $root 'devbox-installation.json')
  $sddl=[IO.File]::GetAccessControl($image,[Security.AccessControl.AccessControlSections]::Access).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)
  $pin=[IO.File]::Open($image,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
  & $helper -Root $root -Image $image -ExpectedDigest $digest -Action Deny
  $start=[Diagnostics.ProcessStartInfo]::new($image)
  $start.UseShellExecute=$false
  $start.CreateNoWindow=$true
  $start.RedirectStandardOutput=$true
  $denied=$false
  try {
    $child=[Diagnostics.Process]::Start($start)
    try {$child.WaitForExit(); throw 'Receiver unexpectedly executable'} finally {$child.Dispose()}
  } catch {
    $errorValue=$_.Exception
    while($null -ne $errorValue.InnerException){$errorValue=$errorValue.InnerException}
    if($errorValue -isnot [ComponentModel.Win32Exception] -or $errorValue.NativeErrorCode -ne 5){throw}
    $denied=$true
  }
  if(-not $denied -or (Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest){throw 'Execute-only access proof failed'}
  & $helper -Root $root -Image $image -ExpectedDigest $digest -Action Restore
  if([IO.File]::GetAccessControl($image,[Security.AccessControl.AccessControlSections]::Access).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access) -cne $sddl){throw 'Original access descriptor changed'}
  $child=[Diagnostics.Process]::Start($start)
  try {$child.WaitForExit(); if($child.ExitCode -ne 0){throw 'Restored receiver launch failed'}} finally {$child.Dispose()}
  # UserTemp commonly starts with AI. Exercise legacy D:/D:P and inherited
  # ACEs too: modern DACL setters can add AI or inherit extra parent ACEs.
  $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
  try {
    foreach($control in @('','AI','P','PAI')) {
      foreach($inheritance in @('','ID')) {
        $fixtureSddl='D:'+$control+'(A;'+$inheritance+';FA;;;'+$sid+')'
        [DevboxReceiverAccess]::Restore($image,$fixtureSddl)
        if([IO.File]::GetAccessControl($image,[Security.AccessControl.AccessControlSections]::Access).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access) -cne $fixtureSddl){throw 'Synthetic inheritance boundary not established'}
        & $helper -Root $root -Image $image -ExpectedDigest $digest -Action Deny
        & $helper -Root $root -Image $image -ExpectedDigest $digest -Action Restore
        if([IO.File]::GetAccessControl($image,[Security.AccessControl.AccessControlSections]::Access).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access) -cne $fixtureSddl){throw 'Inheritance variant not restored exactly'}
      }
    }
  } finally {
    if(Microsoft.PowerShell.Management\Test-Path -LiteralPath (Microsoft.PowerShell.Management\Join-Path $root '.handoff-receiver-access.json')){& $helper -Root $root -Image $image -ExpectedDigest $digest -Action Restore}
    [DevboxReceiverAccess]::Restore($image,$sddl)
  }
  Microsoft.PowerShell.Utility\Write-Output 'Pinned receiver: execute denied, read/digest preserved, exact ACL restored, launch recovered PASS'
} finally {
  if(Microsoft.PowerShell.Management\Test-Path -LiteralPath (Microsoft.PowerShell.Management\Join-Path $root '.handoff-receiver-access.json')){& $helper -Root $root -Image $image -ExpectedDigest $digest -Action Restore}
  if($null -ne $pin){$pin.Dispose()}
  if(Microsoft.PowerShell.Management\Test-Path -LiteralPath $scratch){Microsoft.PowerShell.Management\Remove-Item -LiteralPath $scratch -Recurse -Force}
}
