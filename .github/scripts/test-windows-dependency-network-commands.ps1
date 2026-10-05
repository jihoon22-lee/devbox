# Read-only regression: imports/command discovery and a synthetic missing command.
# Never invokes the hosted entrypoint or a firewall policy cmdlet.
$ErrorActionPreference='Stop'
$source=[IO.Path]::Combine($PSScriptRoot,'windows-dependency-network.ps1')
$tokens=$null; $errors=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count -ne 0){throw 'Dependency helper ParseFile failed'}
$definitions=@($ast.FindAll({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Initialize-DependencyFirewallCommands'},$true))
if($definitions.Count -ne 1){throw 'Expected exact read-only initializer'}
$commands=@($definitions[0].FindAll({param($node) $node -is [System.Management.Automation.Language.CommandAst]},$true))
foreach($command in $commands){
  if($command.GetCommandName() -cnotin @('Import-Module')){throw 'Initializer cannot execute a policy command'}
}
# Model unavailable automatic module discovery, without changing PSModulePath or
# any OS configuration. The explicit inbox import must still resolve commands.
$PSModuleAutoLoadingPreference='None'
. ([ScriptBlock]::Create($definitions[0].Extent.Text))
Initialize-DependencyFirewallCommands
# Discover every literal command in the entire helper, including cleanup. A
# fixed six-command checklist previously missed the Utility Get-FileHash call.
$localNames=@('Initialize-DependencyFirewallCommands','Get-OwnedRule','Assert-RuleScope')
$allCommands=@($ast.FindAll({param($node) $node -is [System.Management.Automation.Language.CommandAst]},$true))
foreach($entry in $allCommands) {
  $name=$entry.GetCommandName()
  if(-not $name){throw 'Dynamic helper command cannot be audited'}
  if($name -cin $localNames){continue}
  $resolved=@(Get-Command -Name $name -ErrorAction Stop)
  if($resolved.Count -ne 1){throw 'Helper command resolution must be unique'}
  $command=$resolved[0]
  if($name.Contains('\')) {
    if(-not $command.Module.Path.StartsWith($PSHOME+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Helper command must resolve to inbox module'}
  } elseif($command.ModuleName -cne 'Microsoft.PowerShell.Core') {
    throw 'Non-core helper command must be module-qualified'
  }
}
# Exercise the actual qualified hash against an independent .NET calculation on
# an existing system executable. This only reads bytes; no fixture/policy runs.
$image=[IO.Path]::Combine($PSHOME,'powershell.exe')
$actual=(Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant()
$sha=[Security.Cryptography.SHA256]::Create()
$stream=[IO.File]::OpenRead($image)
try {$expected=[BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant()}
finally {$stream.Dispose();$sha.Dispose()}
if($actual -cnotmatch '^[a-f0-9]{64}$' -or $actual -cne $expected){throw 'Actual image hash differs from independent SHA256'}
$trap=@($ast.FindAll({param($node) $node -is [System.Management.Automation.Language.TrapStatementAst]},$true))
if($trap.Count -ne 1){throw 'Expected one fixed failure diagnostic trap'}
$fixture=[ScriptBlock]::Create('$failureStage="create-rule"; '+$trap[0].Extent.Text+'; & "Get-OwnedRule"')
$observed=$false
try { & $fixture } catch {
  if($_.Exception -isnot [System.Management.Automation.CommandNotFoundException] -or $_.Exception.CommandName -cne 'Get-OwnedRule'){throw 'Original command resolution error was replaced'}
  $observed=$true
}
if(-not $observed){throw 'Expected synthetic missing command failure'}
[Console]::WriteLine('All helper commands, actual image hash and original failure preservation: PASS')
