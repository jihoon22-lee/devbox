param([string]$ScriptDirectory=$PSScriptRoot)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'The owned GUI process regression requires Windows PowerShell 7.' }
$fixture=Join-Path ([IO.Path]::GetTempPath()) ('devbox-owned-process-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
try {
  $source=Join-Path $fixture 'Fixture.cs'
  $image=Join-Path $fixture 'Owned GUI fixture.exe'
  $marker=Join-Path $fixture 'argument proof.txt'
  @'
using System;
using System.IO;
using System.Threading;
public class OwnedGuiFixture {
 public static int Main(string[] args) {
  Thread.Sleep(Int32.Parse(args[0]));
  if (args[1] != "-") File.WriteAllLines(args[1], args);
  return Int32.Parse(args[2]);
 }
}
'@ | Set-Content -LiteralPath $source -Encoding utf8
  $compiler=Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
  $compile=Start-Process -FilePath $compiler -ArgumentList ('/nologo /target:winexe /out:"'+$image+'" "'+$source+'"') -PassThru
  if (-not $compile.WaitForExit(30000) -or $compile.ExitCode -ne 0) { throw 'Owned GUI fixture compilation failed.' }
  $scriptPath=Join-Path $ScriptDirectory 'windows-user-flow-install.ps1'
  $tokens=$null;$errors=$null
  $ast=[Management.Automation.Language.Parser]::ParseFile($scriptPath,[ref]$tokens,[ref]$errors)
  if ($errors.Count) { throw 'Provisioning script has parse errors.' }
  $function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Run-Owned'},$true)
  if ($null -eq $function) { throw 'Run-Owned was not found.' }
  $uninstallFunction=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-OwnedUninstall'},$true)
  if ($null -eq $uninstallFunction) { throw 'Wait-OwnedUninstall was not found.' }
  . ([scriptblock]::Create($uninstallFunction.Extent.Text))
  $failedUninstall=Start-Process -FilePath $image -ArgumentList '0 - 7' -PassThru
  try {
    $rejected=$false
    try { Wait-OwnedUninstall $failedUninstall } catch {
      if ($_.Exception.Message -notmatch 'uninstall failed; preserve fixture') { throw }
      $rejected=$true
    }
    if (-not $rejected) { throw 'Nonzero uninstall exit was accepted.' }
  } finally { $failedUninstall.Dispose() }
  $successfulUninstall=Start-Process -FilePath $image -ArgumentList '0 - 0' -PassThru
  try { Wait-OwnedUninstall $successfulUninstall } finally { $successfulUninstall.Dispose() }
  # Extract only the function; never bypass or invoke the hosted installation guard.
  . ([scriptblock]::Create($function.Extent.Text))
  Remove-Variable LASTEXITCODE -ErrorAction SilentlyContinue
  $arguments=@('250',$marker,'0','path with spaces\','embedded "quote"','','trailing slash \\')
  Run-Owned $image $arguments
  if (-not (Test-Path -LiteralPath $marker)) { throw 'Run-Owned returned before the GUI process completed.' }
  $actual=[IO.File]::ReadAllLines($marker)
  if ($actual.Length -ne $arguments.Length) { throw 'GUI argument count changed.' }
  for ($i=0;$i -lt $arguments.Length;$i++) {
    if ($actual[$i] -cne $arguments[$i]) { throw "GUI argument $i changed." }
  }
  Push-Location $fixture
  try { Run-Owned 'cmd.exe' @('/c','exit','0') } finally { Pop-Location }
  $global:LASTEXITCODE=47
  Run-Owned $image @('0','-','0')
  $failed=$false
  try { Run-Owned $image @('0','-','23') } catch {
    if ($_.Exception.Message -notmatch 'provisioning operation failed') { throw }
    $failed=$true
  }
  if (-not $failed) { throw 'Nonzero GUI exit was accepted.' }
  $timedOut=$false
  try { Run-Owned $image @('700','-','0') 50 } catch {
    if ($_.Exception.Message -notmatch 'timed out') { throw }
    $timedOut=$true
  }
  if (-not $timedOut) { throw 'GUI wait was not bounded.' }
  # Timeout preserves the process and fixture. Wait for this harmless fixture to exit naturally.
  Start-Sleep -Milliseconds 800
  Write-Output 'Run-Owned GUI wait, exact arguments, fresh exit code, failure and timeout: PASS'
} finally {
  Start-Sleep -Milliseconds 1500
  Remove-Item -LiteralPath $fixture -Recurse -Force
}
