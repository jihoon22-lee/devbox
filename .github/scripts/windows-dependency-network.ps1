# Never invoke locally. This owns one image-scoped rule on a disposable hosted runner.
param(
  [Parameter(Mandatory=$true)][ValidateSet('Add','Remove')][string]$Action,
  [Parameter(Mandatory=$true)][string]$InstallRoot,
  [Parameter(Mandatory=$true)][string]$Executable,
  [Parameter(Mandatory=$true)][int]$OwnerProcessId,
  [Parameter(Mandatory=$true)][string]$ExpectedStart,
  [Parameter(Mandatory=$true)][string]$ExpectedDigest,
  [Parameter(Mandatory=$true)][string]$RuleName
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$failureStage='host-guard'
trap {
  $command='unavailable'
  $required=@('Split-Path','Test-Path','Join-Path','ConvertFrom-Json','Get-Item','Get-Content','Get-FileHash','Get-Process','Get-NetFirewallRule','Get-NetFirewallProfile','Get-NetFirewallApplicationFilter','Get-NetFirewallPortFilter','New-NetFirewallRule','Remove-NetFirewallRule','Get-OwnedRule','Assert-RuleScope','Import-Module','Get-Command')
  if($_.Exception -is [System.Management.Automation.CommandNotFoundException]) {
    $missing=$_.Exception.CommandName -replace '^(NetSecurity|Microsoft[.]PowerShell[.](Utility|Management))\\',''
    if($missing -cin $required){$command=$missing}
  }
  [Console]::Error.WriteLine("DependencyFirewallFailure stage=$failureStage command=$command")
  break # Retain the original terminating error after the fixed diagnostic line.
}
function Initialize-DependencyFirewallCommands {
  # Resolve every non-core dependency from this Windows PowerShell installation;
  # a pwsh parent's PSModulePath must not select an incompatible Utility module.
  foreach($module in @('Microsoft.PowerShell.Management','Microsoft.PowerShell.Utility','NetSecurity')) {
    Import-Module ([IO.Path]::Combine($PSHOME,'Modules',$module,$module+'.psd1')) -ErrorAction Stop
  }
}

if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
  $env:RUNNER_OS -cne 'Windows' -or $env:GITHUB_REPOSITORY -cne 'jihoon22-lee/devbox' -or
  $env:GITHUB_RUN_ID -notmatch '^\d+$'){throw 'Dependency firewall fixture requires disposable GitHub-hosted Windows; never spoof CI'}
$failureStage='command-resolution'
Initialize-DependencyFirewallCommands
$failureStage='ownership-guard'
if($RuleName -cnotmatch '^DevboxFixture-Dependencies-[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$' -or
  $ExpectedDigest -cnotmatch '^[a-f0-9]{64}$' -or $OwnerProcessId -le 0 -or
  $ExpectedStart -cnotmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$'){throw 'Invalid exact firewall ownership'}
$root=[IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$image=[IO.Path]::GetFullPath($Executable)
$temp=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')+'\'
$parent=Microsoft.PowerShell.Management\Split-Path -Parent $root
if((Microsoft.PowerShell.Management\Split-Path -Leaf $root) -cne 'Suite UI Fixture' -or
  (Microsoft.PowerShell.Management\Split-Path -Leaf $parent) -cnotmatch '^devbox-suite-delivery-[a-f0-9]{32}$' -or
  -not $root.StartsWith($temp,[StringComparison]::OrdinalIgnoreCase) -or
  -not $image.StartsWith($root+'\generations\',[StringComparison]::OrdinalIgnoreCase) -or
  $image.Substring($root.Length) -cnotmatch '^\\generations\\[^\\]+\\products\\workspace\\devbox-workspace\.exe$'){throw 'Firewall image outside owned installation'}
$group='Devbox owned dependency fixture'
$description="run=$($env:GITHUB_RUN_ID);pid=$OwnerProcessId;started=$ExpectedStart;sha256=$ExpectedDigest;image=$image"
function Get-OwnedRule {
  $rules=@(NetSecurity\Get-NetFirewallRule -PolicyStore PersistentStore -Name $RuleName -ErrorAction SilentlyContinue)
  if($rules.Count -gt 1){throw 'Ambiguous owned firewall rule'}
  if($rules.Count -eq 0){return $null}
  $rule=$rules[0]
  if($rule.Name -cne $RuleName -or $rule.DisplayName -cne $RuleName -or $rule.Group -cne $group -or
    $rule.Description -cne $description){throw 'Firewall rule ownership changed'}
  return $rule
}
function Assert-RuleScope($rule) {
  $program=@($rule | NetSecurity\Get-NetFirewallApplicationFilter)
  $ports=@($rule | NetSecurity\Get-NetFirewallPortFilter)
  if($rule.Direction -ne 'Outbound' -or $rule.Action -ne 'Block' -or $rule.Enabled -ne 'True' -or
    $rule.Profile -ne 'Any' -or $program.Count -ne 1 -or $ports.Count -ne 1 -or
    $program[0].Program -ine $image -or $ports[0].Protocol -notin @('TCP','6') -or
    "$($ports[0].RemotePort)" -cne '443' -or "$($ports[0].LocalPort)" -cne 'Any'){throw 'Owned firewall scope changed'}
}
if($Action -eq 'Remove') {
  $failureStage='remove-owned-rule'
  $rule=Get-OwnedRule
  if($null -eq $rule){throw 'Owned firewall rule missing before cleanup'}
  Assert-RuleScope $rule
  $rule | NetSecurity\Remove-NetFirewallRule
  if($null -ne (Get-OwnedRule)){throw 'Owned firewall rule removal unconfirmed'}
  exit 0
}
$failureStage='image-identity'
# Validate immutable image and current PID again immediately before changing policy.
if(-not (Microsoft.PowerShell.Management\Test-Path -LiteralPath $image -PathType Leaf)){throw 'Owned image missing'}
$node=Microsoft.PowerShell.Management\Get-Item -LiteralPath $image
while($null -ne $node -and $node.FullName.Length -ge $parent.Length){
  if(($node.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'Owned firewall path contains a link'}
  $node=if($node -is [IO.DirectoryInfo]){$node.Parent}else{$node.Directory}
}
$manifest=Microsoft.PowerShell.Management\Get-Content -LiteralPath (Microsoft.PowerShell.Management\Join-Path $root 'devbox-installation.json') -Raw | Microsoft.PowerShell.Utility\ConvertFrom-Json
$members=@($manifest.members | Where-Object {$_.product -ceq 'workspace'})
if($members.Count -ne 1 -or $members[0].executable -cne "generations/$($manifest.generation)/products/workspace/devbox-workspace.exe" -or
  [IO.Path]::GetFullPath((Microsoft.PowerShell.Management\Join-Path $root $members[0].executable)) -ine $image -or
  $members[0].sha256 -cne $ExpectedDigest -or (Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedDigest){throw 'Owned Workspace digest changed'}
$process=Microsoft.PowerShell.Management\Get-Process -Id $OwnerProcessId
if($process.Path -ine $image -or $process.StartTime.ToUniversalTime().ToString('o') -cne $ExpectedStart){throw 'Owned Workspace PID identity changed'}
if(@(NetSecurity\Get-NetFirewallRule -PolicyStore PersistentStore -Name $RuleName -ErrorAction SilentlyContinue).Count -ne 0){throw 'Refusing to replace an existing firewall rule'}
$failureStage='effective-profile'
# Require enabled effective profiles; never enable/reconfigure an existing profile.
$profiles=@(NetSecurity\Get-NetFirewallProfile -PolicyStore ActiveStore)
if($profiles.Count -eq 0 -or @($profiles | Where-Object {$_.Enabled -ne $true -or $_.AllowLocalFirewallRules -eq $false}).Count -ne 0){throw 'Effective firewall profile does not admit the owned rule'}
$failureStage='create-rule'
try {
  NetSecurity\New-NetFirewallRule -PolicyStore PersistentStore -Name $RuleName -DisplayName $RuleName -Group $group -Description $description -Program $image -Direction Outbound -Action Block -Enabled True -Profile Any -Protocol TCP -RemotePort 443 | Out-Null
  $rule=Get-OwnedRule
  if($null -eq $rule){throw 'Owned firewall creation unconfirmed'}
  Assert-RuleScope $rule
  $failureStage='verify-effective-rule'
  $effective=@(NetSecurity\Get-NetFirewallRule -PolicyStore ActiveStore -Name $RuleName)
  if($effective.Count -ne 1 -or $effective[0].Description -cne $description){throw 'Owned firewall rule not effective'}
  Assert-RuleScope $effective[0]
} catch {
  $original=$_
  # Add refused collisions before this try. Remove only our exact marker if a
  # partial creation occurred; no wildcard or policy-wide cleanup is permitted.
  $created=Get-OwnedRule
  if($null -ne $created){$created | NetSecurity\Remove-NetFirewallRule}
  throw $original
}
