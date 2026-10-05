# Read-only regression: imports/command discovery and a synthetic missing command.
# Never invokes the hosted entrypoint or a firewall policy cmdlet.
$ErrorActionPreference='Stop'
$source=Join-Path $PSScriptRoot 'windows-dependency-network.ps1'
$tokens=$null; $errors=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count -ne 0){throw 'Dependency helper ParseFile failed'}
$definitions=@($ast.FindAll({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Initialize-DependencyFirewallCommands'},$true))
if($definitions.Count -ne 1){throw 'Expected exact read-only initializer'}
$commands=@($definitions[0].FindAll({param($node) $node -is [System.Management.Automation.Language.CommandAst]},$true))
foreach($command in $commands){
  if($command.GetCommandName() -cnotin @('Import-Module','Join-Path','Get-Command','Out-Null')){throw 'Initializer cannot execute a policy command'}
}
# Model unavailable automatic module discovery, without changing PSModulePath or
# any OS configuration. The explicit inbox import must still resolve commands.
$PSModuleAutoLoadingPreference='None'
. ([ScriptBlock]::Create($definitions[0].Extent.Text))
Initialize-DependencyFirewallCommands
foreach($name in @('Get-NetFirewallRule','Get-NetFirewallProfile','Get-NetFirewallApplicationFilter','Get-NetFirewallPortFilter','New-NetFirewallRule','Remove-NetFirewallRule')) {
  $command=Get-Command -Name ('NetSecurity\'+$name) -ErrorAction Stop
  if($command.Module.ModuleBase -ine ($PSHOME+'\Modules\NetSecurity')){throw 'Command must resolve to inbox NetSecurity'}
}
$trap=@($ast.FindAll({param($node) $node -is [System.Management.Automation.Language.TrapStatementAst]},$true))
if($trap.Count -ne 1){throw 'Expected one fixed failure diagnostic trap'}
$fixture=[ScriptBlock]::Create('$failureStage="create-rule"; '+$trap[0].Extent.Text+'; & "Get-OwnedRule"')
$observed=$false
try { & $fixture } catch {
  if($_.Exception -isnot [System.Management.Automation.CommandNotFoundException] -or $_.Exception.CommandName -cne 'Get-OwnedRule'){throw 'Original command resolution error was replaced'}
  $observed=$true
}
if(-not $observed){throw 'Expected synthetic missing command failure'}
[Console]::WriteLine('Read-only module resolution and original failure preservation: PASS')
