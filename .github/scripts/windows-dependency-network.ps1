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
if($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
  $env:RUNNER_OS -cne 'Windows' -or $env:GITHUB_REPOSITORY -cne 'jihoon22-lee/devbox' -or
  $env:GITHUB_RUN_ID -notmatch '^\d+$'){throw 'Dependency firewall fixture requires disposable GitHub-hosted Windows; never spoof CI'}
if($RuleName -cnotmatch '^DevboxFixture-Dependencies-[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$' -or
  $ExpectedDigest -cnotmatch '^[a-f0-9]{64}$' -or $OwnerProcessId -le 0 -or
  $ExpectedStart -cnotmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$'){throw 'Invalid exact firewall ownership'}
$root=[IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$image=[IO.Path]::GetFullPath($Executable)
$temp=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')+'\'
$parent=Split-Path -Parent $root
if((Split-Path -Leaf $root) -cne 'Suite UI Fixture' -or
  (Split-Path -Leaf $parent) -cnotmatch '^devbox-suite-delivery-[a-f0-9]{32}$' -or
  -not $root.StartsWith($temp,[StringComparison]::OrdinalIgnoreCase) -or
  -not $image.StartsWith($root+'\generations\',[StringComparison]::OrdinalIgnoreCase) -or
  $image.Substring($root.Length) -cnotmatch '^\\generations\\[^\\]+\\products\\workspace\\devbox-workspace\.exe$'){throw 'Firewall image outside owned installation'}
$group='Devbox owned dependency fixture'
$description="run=$($env:GITHUB_RUN_ID);pid=$OwnerProcessId;started=$ExpectedStart;sha256=$ExpectedDigest;image=$image"
function Get-OwnedRule {
  $rules=@(Get-NetFirewallRule -PolicyStore PersistentStore -Name $RuleName -ErrorAction SilentlyContinue)
  if($rules.Count -gt 1){throw 'Ambiguous owned firewall rule'}
  if($rules.Count -eq 0){return $null}
  $rule=$rules[0]
  if($rule.Name -cne $RuleName -or $rule.DisplayName -cne $RuleName -or $rule.Group -cne $group -or
    $rule.Description -cne $description){throw 'Firewall rule ownership changed'}
  return $rule
}
function Assert-RuleScope($rule) {
  $program=@($rule | Get-NetFirewallApplicationFilter)
  $ports=@($rule | Get-NetFirewallPortFilter)
  if($rule.Direction -ne 'Outbound' -or $rule.Action -ne 'Block' -or $rule.Enabled -ne 'True' -or
    $rule.Profile -ne 'Any' -or $program.Count -ne 1 -or $ports.Count -ne 1 -or
    $program[0].Program -ine $image -or $ports[0].Protocol -notin @('TCP','6') -or
    "$($ports[0].RemotePort)" -cne '443' -or "$($ports[0].LocalPort)" -cne 'Any'){throw 'Owned firewall scope changed'}
}
if($Action -eq 'Remove') {
  $rule=Get-OwnedRule
  if($null -eq $rule){throw 'Owned firewall rule missing before cleanup'}
  Assert-RuleScope $rule
  $rule | Remove-NetFirewallRule
  if($null -ne (Get-OwnedRule)){throw 'Owned firewall rule removal unconfirmed'}
  exit 0
}
# Validate immutable image and current PID again immediately before changing policy.
if(-not (Test-Path -LiteralPath $image -PathType Leaf)){throw 'Owned image missing'}
$node=Get-Item -LiteralPath $image
while($null -ne $node -and $node.FullName.Length -ge $parent.Length){
  if(($node.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'Owned firewall path contains a link'}
  $node=if($node -is [IO.DirectoryInfo]){$node.Parent}else{$node.Directory}
}
$manifest=Get-Content -LiteralPath (Join-Path $root 'devbox-installation.json') -Raw | ConvertFrom-Json
$members=@($manifest.members | Where-Object {$_.product -ceq 'workspace'})
if($members.Count -ne 1 -or $members[0].executable -cne "generations/$($manifest.generation)/products/workspace/devbox-workspace.exe" -or
  [IO.Path]::GetFullPath((Join-Path $root $members[0].executable)) -ine $image -or
  $members[0].sha256 -cne $ExpectedDigest -or (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedDigest){throw 'Owned Workspace digest changed'}
$process=Get-Process -Id $OwnerProcessId
if($process.Path -ine $image -or $process.StartTime.ToUniversalTime().ToString('o') -cne $ExpectedStart){throw 'Owned Workspace PID identity changed'}
if(@(Get-NetFirewallRule -PolicyStore PersistentStore -Name $RuleName -ErrorAction SilentlyContinue).Count -ne 0){throw 'Refusing to replace an existing firewall rule'}
# Require enabled effective profiles; never enable/reconfigure an existing profile.
$profiles=@(Get-NetFirewallProfile -PolicyStore ActiveStore)
if($profiles.Count -eq 0 -or @($profiles | Where-Object {$_.Enabled -ne $true -or $_.AllowLocalFirewallRules -eq $false}).Count -ne 0){throw 'Effective firewall profile does not admit the owned rule'}
try {
  New-NetFirewallRule -PolicyStore PersistentStore -Name $RuleName -DisplayName $RuleName -Group $group -Description $description -Program $image -Direction Outbound -Action Block -Enabled True -Profile Any -Protocol TCP -RemotePort 443 | Out-Null
  $rule=Get-OwnedRule
  if($null -eq $rule){throw 'Owned firewall creation unconfirmed'}
  Assert-RuleScope $rule
  $effective=@(Get-NetFirewallRule -PolicyStore ActiveStore -Name $RuleName)
  if($effective.Count -ne 1 -or $effective[0].Description -cne $description){throw 'Owned firewall rule not effective'}
  Assert-RuleScope $effective[0]
} catch {
  $original=$_
  # Add refused collisions before this try. Remove only our exact marker if a
  # partial creation occurred; no wildcard or policy-wide cleanup is permitted.
  $created=Get-OwnedRule
  if($null -ne $created){$created | Remove-NetFirewallRule}
  throw $original
}
