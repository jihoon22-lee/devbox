$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Private fixture export requires a hosted runner' }
$names = @('knowledge_base_lib', 'everything_plus_lib', 'devbox_knowledge_lib')
$artifacts = @{}
& cargo test -p knowledge-base -p everything-plus -p devbox-knowledge --lib --no-run --message-format=json | ForEach-Object {
  $message = $_ | ConvertFrom-Json
  if ($message.reason -eq 'compiler-artifact' -and $message.profile.test -and $message.executable -and $names -contains $message.target.name) {
    $artifacts[$message.target.name] = $message.executable
  }
}
if ($LASTEXITCODE -ne 0) { throw 'Windows native fixture compilation failed' }
if ($artifacts.Count -ne $names.Count) { throw 'Expected three native fixture executables' }
$destination = Join-Path $env:GITHUB_WORKSPACE 'knowledge-wsl-fixtures'
New-Item -ItemType Directory $destination | Out-Null
$records = @()
foreach ($name in $names) {
  $file = "$name.exe"
  $target = Join-Path $destination $file
  Copy-Item -LiteralPath $artifacts[$name] -Destination $target
  $records += @{ file = $file; sha256 = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant(); bytes = (Get-Item -LiteralPath $target).Length; filter = 'native_wsl_owned_fixture' }
}
@{ schemaVersion = 1; source = $env:GITHUB_SHA; runId = $env:GITHUB_RUN_ID; kind = 'Windows unit-test executables; not product/release candidates'; fixtures = $records } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $destination 'manifest.json')
