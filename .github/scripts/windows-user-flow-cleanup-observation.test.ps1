param([string]$Driver=(Join-Path $PSScriptRoot 'windows-user-flow-install.ps1'))
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($Driver,[ref]$tokens,[ref]$errors)
if($errors.Count -ne 0){throw 'Owned cleanup driver syntax invalid'}
$definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Write-OwnedCleanupObservation'},$true)
if($null -eq $definition){throw 'Cleanup observation function missing'}
. ([scriptblock]::Create($definition.Extent.Text))
$fixture=Join-Path ([IO.Path]::GetTempPath()) ('devbox-cleanup-observation-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
try {
  $record=Join-Path $fixture 'observation.json'
  foreach($name in @('suite-update.json','suite-data-restore.json','uninstall-plan.json','uninstall-complete.json')) {
    [IO.File]::WriteAllText((Join-Path $fixture $name),'synthetic')
  }
  Write-OwnedCleanupObservation $fixture $record 'wait-removal-receipt' 'running' 0
  $text=[IO.File]::ReadAllText($record)
  $value=$text | ConvertFrom-Json
  if($text.Contains($fixture) -or $value.stage -cne 'wait-removal-receipt' -or $value.status -cne 'running' -or $value.exitCode -ne 0 -or -not ($value.updatePending -and $value.restorePending -and $value.uninstallPending -and $value.removalReceipt)){throw 'Bounded running observation mismatch'}
  Write-OwnedCleanupObservation $fixture $record 'wait-uninstaller' 'failed' 7
  $value=[IO.File]::ReadAllText($record) | ConvertFrom-Json
  if($value.status -cne 'failed' -or $value.exitCode -ne 7){throw 'Failed cleanup observation mismatch'}
  Remove-Item -LiteralPath $fixture -Recurse -Force
  New-Item -ItemType Directory -Path $fixture | Out-Null
  Write-OwnedCleanupObservation $fixture $record 'remove-owned-installation' 'completed'
  $value=[IO.File]::ReadAllText($record) | ConvertFrom-Json
  if($value.status -cne 'completed' -or $null -ne $value.exitCode -or $value.updatePending -or $value.restorePending -or $value.uninstallPending -or $value.removalReceipt){throw 'Completed cleanup observation mismatch'}
  Write-Output 'Owned cleanup syntax and bounded running/failed/completed evidence: PASS'
} finally {
  Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
