param([string]$Driver=(Join-Path $PSScriptRoot 'windows-user-flow-install.ps1'))
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($Driver,[ref]$tokens,[ref]$errors)
if($errors.Count -ne 0){throw 'Owned cleanup driver syntax invalid'}
foreach($name in @('Start-OwnedCleanupUninstaller','Wait-OwnedUninstall','Write-OwnedCleanupObservation')) {
  $definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
  if($null -eq $definition){throw 'Owned cleanup function missing'}
  . ([scriptblock]::Create($definition.Extent.Text))
}
$fixture=Join-Path ([IO.Path]::GetTempPath()) ('devbox-cleanup-uninstall-'+[guid]::NewGuid().ToString('N'))
$root=Join-Path $fixture 'Suite UI Fixture'
New-Item -ItemType Directory -Path $root -Force | Out-Null
$process=$null
$selfCopy=$null
try {
  $image=Join-Path $root 'Uninstall.exe'
  Add-Type -OutputAssembly $image -OutputType WindowsApplication -TypeDefinition @'
using System;
using System.IO;
using System.Threading;
public class OwnedUninstallArgs {
 public static int Main(string[] args) {
  string command=Environment.CommandLine;
  string embedded=Path.GetDirectoryName(Environment.GetCommandLineArgs()[0]);
  if(args.Length == 1 && args[0] == "--self-copy-child") {
   Thread.Sleep(300); File.WriteAllText(Path.Combine(embedded,"self-copy-failure.txt"),"17"); return 17;
  }
  int offset=command.IndexOf("_?=",StringComparison.Ordinal);
  if(offset < 0) {
   System.Diagnostics.Process.Start(Environment.GetCommandLineArgs()[0],"--self-copy-child"); return 0;
  }
  if(args.Length < 2 || args[0] != "/S") return 19;
  string root=command.Substring(offset+3);
  Thread.Sleep(300);
  File.WriteAllLines(Path.Combine(root,"argument-proof.txt"),new string[]{"/S","_?="+root});
  return File.Exists(Path.Combine(root,"reject-uninstall.txt")) ? 17 : 0;
 }
}
'@
  $hash=(Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash
  $process=Start-OwnedCleanupUninstaller $root $fixture
  Wait-OwnedUninstall $process
  $lines=[IO.File]::ReadAllLines((Join-Path $root 'argument-proof.txt'))
  if($lines.Count -ne 2 -or $lines[0] -cne '/S' -or $lines[1] -cne ('_?='+$root)){throw 'Raw final NSIS path argument mismatch'}
  if($process.StartInfo.FileName -ceq $image -or (Get-FileHash -LiteralPath $process.StartInfo.FileName -Algorithm SHA256).Hash -cne $hash -or (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash -cne $hash){throw 'Exact owned scratch copy/source mismatch'}
  $selfCopy=Start-Process -FilePath $image -ArgumentList '/S' -PassThru
  if(-not $selfCopy.WaitForExit(10000) -or $selfCopy.ExitCode -ne 0){throw 'Self-copy fixture parent must report zero'}
  $deadline=[DateTime]::UtcNow.AddSeconds(10)
  while(-not (Test-Path -LiteralPath (Join-Path $root 'self-copy-failure.txt')) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 50}
  if([IO.File]::ReadAllText((Join-Path $root 'self-copy-failure.txt')) -cne '17'){throw 'Child failure must remain hidden by original self-copy parent'}
  [IO.File]::WriteAllText((Join-Path $root 'reject-uninstall.txt'),'synthetic failure')
  $process.Dispose()
  $process=Start-OwnedCleanupUninstaller $root $fixture
  $rejected=$false
  $actualExitCode=$null
  try{Wait-OwnedUninstall $process ([ref]$actualExitCode)}catch{$rejected=$true}
  if(-not $rejected -or $process.ExitCode -ne 17 -or $actualExitCode -ne 17){throw 'Actual copied uninstaller failure must be rejected and retained'}
  $evidence=Join-Path $fixture 'failed-observation.json'
  Write-OwnedCleanupObservation $root $evidence 'wait-uninstaller' 'failed' $actualExitCode
  $observation=[IO.File]::ReadAllText($evidence) | ConvertFrom-Json
  if($observation.exitCode -ne 17 -or $observation.status -cne 'failed' -or $observation.removalReceipt){throw 'Failed observation must preserve actual exit17 and absent receipt'}
  if(Test-Path -LiteralPath (Join-Path $root 'uninstall-complete.json')){throw 'Argument regression must not simulate a removal receipt'}
  Write-Output 'Owned uninstall scratch copy, SHA, raw final space-path and actual child exit: PASS'
} finally {
  if($process){$process.Dispose()}
  if($selfCopy){$selfCopy.Dispose()}
  Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
