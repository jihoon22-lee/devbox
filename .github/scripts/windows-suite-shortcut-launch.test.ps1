$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$source=Join-Path $PSScriptRoot 'windows-suite-shortcut-launch.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Shortcut launcher parse failed'}
$failureFunction=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ShortcutFailureObservation'},$true)
if($null -eq $failureFunction){throw 'Shortcut failure projection missing'}
Invoke-Expression $failureFunction.Extent.Text
foreach($command in @('Get-FileHash','private-user-command')) {
 $failure=[Management.Automation.ErrorRecord]::new([Management.Automation.CommandNotFoundException]::new($command),'CommandNotFoundException',[Management.Automation.ErrorCategory]::ObjectNotFound,$null)
 $failure.Exception.CommandName=$command
 $observation=Get-ShortcutFailureObservation $failure 'retained-payload'
 if($observation.issue -cne 'shortcut_command_unavailable'){throw 'Exact missing-command issue required'}
 if($command -ceq 'Get-FileHash' -and $observation.command -cne $command){throw 'Allowlisted command should remain available'}
 if($command -ceq 'private-user-command' -and $null -ne $observation.command){throw 'Unknown command must be omitted'}
}
# Load only the physical identity reader and assertion; never execute ShellExecute.
$types=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Add-Type'},$true))
if($types.Count -ne 1){throw 'One native identity reader required'}
Invoke-Expression $types[0].Extent.Text
$functions=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-Identity'},$true))
if($functions.Count -ne 1){throw 'Identity assertion missing'}
Invoke-Expression $functions[0].Extent.Text
$directory=Join-Path ([IO.Path]::GetTempPath()) ('devbox-shortcut-identity-test-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($directory) | Out-Null
try {
 $file=Join-Path $directory 'owned.lnk'
 $copy=Join-Path $directory 'copied.lnk'
 [IO.File]::WriteAllText($file,'synthetic fixture; not an executable link')
 [IO.File]::Copy($file,$copy)
 $identity=[OwnedShortcutIdentity]::Read($file)
 Assert-Identity $file $identity
 $rejected=$false
 try{Assert-Identity $copy $identity}catch{$rejected=$true}
 if(-not $rejected){throw 'Same-byte foreign shortcut identity accepted'}
 $rejected=$false
 try{Assert-Identity $file @($identity[0])}catch{$rejected=$true}
 if(-not $rejected){throw 'Incomplete identity accepted'}
 Write-Output 'PASS: owned shortcut identity rejects copied replacement; launcher parsed without executing links'
} finally {
 Remove-Item -LiteralPath $directory -Recurse -Force
}
