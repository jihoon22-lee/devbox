param([Parameter(Mandatory=$true)][string]$Retained, [Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Private rebuild requires disposable hosted Windows.' }
$receipt = Get-Content -Raw "$Retained/suite-fixture-source.json" | ConvertFrom-Json
$payload = Get-Content -Raw "$Retained/suite-payload.json" | ConvertFrom-Json
if ($receipt.sourceSha -notmatch '^[a-f0-9]{40}$' -or $receipt.sourceSha -ne $payload.sourceSha) { throw 'Retained source mismatch' }
git merge-base --is-ancestor $receipt.sourceSha HEAD
if ($LASTEXITCODE -ne 0) { throw 'Retained source must be an ancestor of this private rebuild' }
$changes = @(git diff --name-only $receipt.sourceSha HEAD)
if ($LASTEXITCODE -ne 0) { throw 'Cannot compare retained product inputs' }
$privateNative = @(
  'apps/devbox-control-center/src-tauri/src/platform/legacy_installer.rs',
  'apps/devbox-control-center/src-tauri/src/tools_host.rs',
  'apps/devbox-control-center/src-tauri/src/legacy_cleanup.rs'
)
foreach ($change in $changes) {
  if ($change -notmatch '^(\.github/|docs/|workthrough/|apps/devbox-control-center/src/)' -and $change -notin $privateNative -and $change -notin @('apps/v0.8-feature-parity.json','apps/v0.8-data-inventory.json','apps/devbox-control-center/README.md')) {
    throw "Changed input requires rebuilding the full Suite: $change"
  }
}
$current = (git rev-parse HEAD).Trim()
$version = (Get-Content -Raw apps/devbox-control-center/src-tauri/tauri.conf.json | ConvertFrom-Json).version
if ($version -ne $payload.suiteVersion) { throw 'Retained product version mismatch' }
if ((Get-FileHash THIRD_PARTY_NOTICES.md -Algorithm SHA256).Hash.ToLowerInvariant() -ne $payload.notices.sha256) { throw 'Retained dependency notices changed' }
$raw = Join-Path $env:RUNNER_TEMP 'suite-control-center-rebuild-input'
if ((Test-Path -LiteralPath $raw) -or (Test-Path -LiteralPath $Output)) { throw 'Private rebuild destination already exists' }
New-Item -ItemType Directory -Path $raw | Out-Null
Copy-Item THIRD_PARTY_NOTICES.md "$raw/"
$productSources = [ordered]@{}
Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($product in @('workspace','api-studio','knowledge')) {
  $definition = @($payload.products | Where-Object id -CEQ $product)
  if ($definition.Count -ne 1) { throw 'Retained product missing or duplicated' }
  $definition = $definition[0]
  $name = "devbox-${product}_${version}_x64.zip"
  if ($definition.portable.name -cne $name) { throw 'Unexpected retained archive name' }
  $archive = Join-Path $Retained $name
  if ((Get-Item -LiteralPath $archive).Length -ne $definition.portable.size -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $definition.portable.sha256) { throw 'Retained archive changed' }
  $names = @("devbox-$product.exe")
  if ($product -eq 'workspace') { $names += @('resources/wsl/manifest.json','resources/wsl/devbox-workspace-wsl') }
  $zip = [IO.Compression.ZipFile]::OpenRead($archive)
  try {
    foreach ($name in $names) {
      $declared = @($definition.files | Where-Object name -CEQ $name)
      $entries = @($zip.Entries | Where-Object FullName -CEQ $name)
      if ($declared.Count -ne 1 -or $entries.Count -ne 1 -or $entries[0].Length -ne $declared[0].size -or $declared[0].size -le 0 -or $declared[0].size -gt 1GB) { throw 'Invalid retained file declaration' }
      # Only these fixed file names are materialized; archive paths never become destinations.
      $destination = Join-Path "$raw/$product" $name
      New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
      $entryStream = $entries[0].Open()
      $destinationStream = [IO.File]::Open($destination,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write)
      try {
        $buffer = New-Object byte[] 65536
        [long]$copied = 0
        while (($count = $entryStream.Read($buffer,0,$buffer.Length)) -gt 0) {
          $copied += $count
          if ($copied -gt $declared[0].size) { throw 'Retained file exceeds declared length' }
          $destinationStream.Write($buffer,0,$count)
        }
        if ($copied -ne $declared[0].size) { throw 'Retained file length mismatch' }
      } finally { $destinationStream.Dispose(); $entryStream.Dispose() }
      if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $declared[0].sha256) { throw 'Retained file digest mismatch' }
    }
  } finally { $zip.Dispose() }
  $prior = if ($receipt.productSources) { $receipt.productSources.PSObject.Properties[$product].Value } else { $receipt.sourceSha }
  if ($prior -notmatch '^[a-f0-9]{40}$') { throw 'Retained product source missing' }
  $productSources[$product] = $prior
}
pnpm --filter 'devbox-control-center^...' --if-present build
if ($LASTEXITCODE -ne 0) { throw 'Control Center shared frontend build failed' }
pnpm --filter devbox-control-center tauri build --debug --bundles nsis
if ($LASTEXITCODE -ne 0) { throw 'Control Center rebuild failed' }
cargo build -p devbox-control-center --bin devbox-suite-bootstrap
if ($LASTEXITCODE -ne 0) { throw 'Control Center helper rebuild failed' }
New-Item -ItemType Directory -Path "$raw/control-center/resources/suite" -Force | Out-Null
Copy-Item target/debug/devbox-control-center.exe "$raw/control-center/"
Copy-Item target/debug/devbox-suite-bootstrap.exe "$raw/control-center/resources/suite/"
python .github/scripts/build-suite-package.py portables $raw $Output $version $current
if ($LASTEXITCODE -ne 0) { throw 'Private Suite reassembly failed' }
$compiler = Join-Path $env:LOCALAPPDATA 'tauri/NSIS/makensis.exe'
python .github/scripts/build-suite-installer.py $Output "$raw/control-center/resources/suite/devbox-suite-bootstrap.exe" --makensis $compiler
if ($LASTEXITCODE -ne 0) { throw 'Private Suite installer rebuild failed' }
Copy-Item target/debug/devbox-suite-bootstrap.exe "$Output/"
$productSources['control-center'] = $current
@{ sourceSha=$current; runHeadSha=$current; repository=$env:GITHUB_REPOSITORY; runId=$env:GITHUB_RUN_ID; retainedFromRun=$receipt.runId; productSources=$productSources; scope='private-control-center-rebuild' } | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 "$Output/suite-fixture-source.json"
