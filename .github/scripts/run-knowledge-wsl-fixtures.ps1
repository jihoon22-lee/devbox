param(
  [Parameter(Mandatory=$true)][string]$FixtureDirectory,
  [Parameter(Mandatory=$true)][string]$WslRoot,
  [Parameter(Mandatory=$true)][string]$Owner,
  [Parameter(Mandatory=$true)][string]$ExpectedSource,
  [Parameter(Mandatory=$true)][string]$Report
)
$ErrorActionPreference = 'Stop'
if ($ExpectedSource -notmatch '^[a-f0-9]{40}$' -or $Owner -notmatch '^[a-f0-9-]{36}$') { throw 'Invalid fixture identity' }
if ($WslRoot -notmatch '^\\\\wsl\.localhost\\([a-zA-Z0-9._-]+)\\tmp\\devbox-knowledge-wsl2-fixture-[a-f0-9-]{36}$' -or -not $WslRoot.EndsWith($Owner)) { throw 'An explicitly owned WSL UNC folder is required' }
$distro = $Matches[1]
$listing = ((& wsl.exe --list --verbose) -join "`n").Replace("`0", '')
if ($LASTEXITCODE -ne 0 -or $listing -notmatch ('(?m)^\s*\*?\s*' + [regex]::Escape($distro) + '\s+.+?\s+2\s*$')) { throw 'The selected fixture distro is not confirmed as WSL2' }
$marker = Join-Path $WslRoot 'fixture-owner.txt'
if ((Get-Content -LiteralPath $marker -Raw) -ne $Owner) { throw 'Fixture folder owner mismatch' }
$manifest = Get-Content -LiteralPath (Join-Path $FixtureDirectory 'manifest.json') -Raw | ConvertFrom-Json
if ($manifest.schemaVersion -ne 1 -or $manifest.source -ne $ExpectedSource -or $manifest.fixtures.Count -ne 3) { throw 'Fixture source manifest mismatch' }
$expectedFiles = @('knowledge_base_lib.exe', 'everything_plus_lib.exe', 'devbox_knowledge_lib.exe')
if (@(Compare-Object ($expectedFiles | Sort-Object) ($manifest.fixtures.file | Sort-Object)).Count) { throw 'Unexpected fixture executables' }
$evidence = @{ source = $ExpectedSource; artifactRun = $manifest.runId; environment = 'local Windows with actual WSL2 UNC filesystem'; osVersion = [Environment]::OSVersion.Version.ToString(); boundary = 'Owned synthetic folders and in-memory index DB only; no product/legacy profile, distro shutdown or VM suspend'; result = 'failed'; tests = @() }
$failed = $false
foreach ($fixture in $manifest.fixtures) {
  $record = @{ name = $fixture.file; result = 'failed' }
  $process = $null
  $started = $false
  try {
    if ($fixture.filter -ne 'native_wsl_owned_fixture') { throw 'Unexpected test filter' }
    $file = Join-Path $FixtureDirectory $fixture.file
    if ((Get-Item -LiteralPath $file).Length -ne $fixture.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $fixture.sha256) { throw 'Fixture executable digest mismatch' }
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $file
    $start.Arguments = 'native_wsl_owned_fixture --ignored --test-threads=1 --nocapture'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($key in @($start.EnvironmentVariables.Keys)) {
      if ($key -match 'TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY') { $start.EnvironmentVariables.Remove($key) }
    }
    $start.EnvironmentVariables['DEVBOX_WSL_FIXTURE_ROOT'] = $WslRoot
    $start.EnvironmentVariables['DEVBOX_WSL_FIXTURE_OWNER'] = $Owner
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    $watch = [Diagnostics.Stopwatch]::StartNew()
    if (-not $process.Start()) { throw 'Fixture process did not start' }
    $started = $true
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(180000)) { $process.Kill(); $process.WaitForExit(); throw 'Fixture process timed out' }
    $out = $stdout.GetAwaiter().GetResult()
    $err = $stderr.GetAwaiter().GetResult()
    $record.elapsedMs = $watch.ElapsedMilliseconds
    $record.exitCode = $process.ExitCode
    $record.output = $out.Substring(0, [Math]::Min($out.Length, 6000))
    $record.stderr = $err.Substring(0, [Math]::Min($err.Length, 3000))
    if ($process.ExitCode -ne 0 -or $out -notmatch 'test result: ok\. 1 passed; 0 failed; 0 ignored;') { throw 'Native WSL fixture did not pass its one required test' }
    $record.result = 'pass'
  } catch {
    $record.error = $_.Exception.Message
    $failed = $true
  } finally {
    if ($process) { if ($started -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }; $process.Dispose() }
    $evidence.tests += $record
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 -LiteralPath $Report
  }
}
if (-not $failed) { $evidence.result = 'pass' }
$evidence | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 -LiteralPath $Report
if ($failed) { throw 'One or more owned native WSL fixtures failed; see the evidence report' }
'All three Windows native WSL fixture tests passed.'
