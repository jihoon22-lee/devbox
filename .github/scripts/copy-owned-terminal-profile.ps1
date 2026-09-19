param([Parameter(Mandatory)][string]$Source,[Parameter(Mandatory)][string]$Target,[Parameter(Mandatory)][string]$Nonce)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Hosted fixture required' }
$sourceRoot = [IO.Path]::GetFullPath($Source).TrimEnd('\')
$targetRoot = [IO.Path]::GetFullPath($Target).TrimEnd('\')
if ([IO.Path]::GetFileName($sourceRoot) -notmatch '^com\.devbox\.v08\.workspace\.i[a-f0-9]{64}$') { throw 'Unexpected source owner' }
if ([IO.Path]::GetFileName($targetRoot) -ne 'com.devbox.wsldesktop') { throw 'Unexpected target owner' }
if ([IO.File]::ReadAllText((Join-Path $targetRoot '.fixture-owner')) -ne $Nonce) { throw 'Fixture ownership changed' }
foreach ($directory in @($sourceRoot,$targetRoot)) {
  if (([IO.File]::GetAttributes($directory) -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Fixture root is a link' }
}
$candidates = @('EBWebView\Default\Local Storage\leveldb','Default\Local Storage\leveldb')
$existing = @($candidates | Where-Object { [IO.Directory]::Exists((Join-Path $sourceRoot $_)) })
if ($existing.Count -ne 1) { throw 'Expected one owned localStorage store' }
$sourceStore = Join-Path $sourceRoot $existing[0]
$targetStore = Join-Path $targetRoot $existing[0]
$handles = [Collections.Generic.List[IO.FileStream]]::new()
$deadline = [DateTime]::UtcNow.AddSeconds(25)
$lock = $null
while (-not $lock -and [DateTime]::UtcNow -lt $deadline) {
  try { $lock = [IO.File]::Open((Join-Path $sourceStore 'LOCK'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None) }
  catch { Start-Sleep -Milliseconds 100 }
}
if (-not $lock) { throw 'Owned WebView store did not close' }
try {
  $files = @(Get-ChildItem -LiteralPath $sourceStore -Force)
  if ($files.Count -gt 512) { throw 'Fixture store file limit' }
  $entries = @()
  $total = 0L
  foreach ($file in $files) {
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Unexpected store entry' }
    if ($file.Name -eq 'LOCK') { continue }
    $stream = [IO.File]::Open($file.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
    $handles.Add($stream); $entries += ,@($file.Name,$stream)
    $total += $stream.Length
    if ($total -gt 268435456) { throw 'Fixture store byte limit' }
  }
  [IO.Directory]::CreateDirectory($targetStore) | Out-Null
  foreach ($entry in $entries) {
    $destination = [IO.File]::Open((Join-Path $targetStore $entry[0]),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try { $entry[1].CopyTo($destination); $destination.Flush($true) } finally { $destination.Dispose() }
  }
  $copiedLock = [IO.File]::Open((Join-Path $targetStore 'LOCK'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
  $copiedLock.Dispose()
} finally {
  foreach ($handle in $handles) { $handle.Dispose() }
  $lock.Dispose()
}
