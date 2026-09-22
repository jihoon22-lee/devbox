param([Parameter(Mandatory=$true)][string]$Shards, [Parameter(Mandatory=$true)][string]$Output, [Parameter(Mandatory=$true)][string]$Tag, [Parameter(Mandatory=$true)][string]$Source, [switch]$AllowPrerelease)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Suite assembly requires disposable hosted Windows.' }
if ($Source -notmatch '^[a-f0-9]{40}$' -or (git rev-parse HEAD).Trim() -cne $Source) { throw 'Exact source and stable tag required.' }
$pattern = if ($AllowPrerelease) { '^v\d+\.\d+\.\d+-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*$' } else { '^v\d+\.\d+\.\d+$' }
if ($Tag -notmatch $pattern) { throw 'Release tag does not match the explicit channel.' }
$version = $Tag.Split('-')[0].Substring(1)
foreach ($product in @('workspace','api-studio','knowledge','control-center')) {
  if ((Get-Content -Raw "apps/devbox-$product/src-tauri/tauri.conf.json" | ConvertFrom-Json).version -cne $version) { throw 'Product version differs from Suite tag.' }
}
if (Test-Path -LiteralPath $Output) { throw 'New candidate output required.' }
New-Item -ItemType Directory -Path $Output | Out-Null
$payload = Join-Path $Output 'payload'
New-Item -ItemType Directory -Path $payload | Out-Null
$seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($shard in Get-ChildItem -LiteralPath $Shards -Directory) {
  if (($shard.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or @(Get-ChildItem -LiteralPath $shard.FullName -Recurse -Force | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count -ne 0) { throw 'Linked shard content.' }
  $receipt = Get-Content -Raw -LiteralPath (Join-Path $shard.FullName 'shard-source.json') | ConvertFrom-Json
  if ($receipt.schemaVersion -ne 1 -or $receipt.sourceSha -cne $Source -or $receipt.profile -cne 'release') { throw 'Shard provenance mismatch.' }
  $required = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($product in $receipt.products) {
    if ($product -cnotin @('workspace','api-studio','knowledge','control-center') -or -not $seen.Add($product)) { throw 'Missing/duplicate/unknown shard product.' }
    [void]$required.Add("$product/devbox-$product.exe")
    if ($product -in @('workspace','knowledge')) { [void]$required.Add("$product/resources/wsl/manifest.json"); [void]$required.Add("$product/resources/wsl/devbox-workspace-wsl") }
    if ($product -eq 'control-center') { [void]$required.Add('control-center/resources/suite/devbox-suite-bootstrap.exe') }
  }
  foreach ($file in $receipt.files) {
    if (-not $required.Remove($file.name)) { throw 'Duplicate/unexpected shard file.' }
    $input = Join-Path $shard.FullName $file.name
    $item = Get-Item -LiteralPath $input
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $item.Length -ne $file.size -or $item.Length -le 0 -or $item.Length -gt 1GB -or (Get-FileHash -LiteralPath $input -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.sha256) { throw 'Shard file identity mismatch.' }
    $destination = Join-Path $payload $file.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    if (Test-Path -LiteralPath $destination) { throw 'Overlapping shard output.' }
    Copy-Item -LiteralPath $input -Destination $destination
  }
  if ($required.Count -ne 0) { throw 'Incomplete shard files.' }
  $actual = @(Get-ChildItem -LiteralPath $shard.FullName -File -Recurse)
  if ($actual.Count -ne $receipt.files.Count + 1) { throw 'Undeclared shard content.' }
}
if ($seen.Count -ne 4) { throw 'Incomplete four-product candidate.' }
Copy-Item -LiteralPath 'THIRD_PARTY_NOTICES.md' -Destination $payload
$assembly = Join-Path $Output 'assembly'
python .github/scripts/build-suite-package.py portables $payload $assembly $version $Source
if ($LASTEXITCODE -ne 0) { throw 'Suite archive assembly failed.' }
$compiler = & "$PSScriptRoot/acquire-suite-nsis.ps1" -Destination (Join-Path $env:RUNNER_TEMP ('suite-nsis-' + [guid]::NewGuid().ToString('N')))
python .github/scripts/build-suite-installer.py $assembly "$payload/control-center/resources/suite/devbox-suite-bootstrap.exe" --makensis $compiler
if ($LASTEXITCODE -ne 0) { throw 'Suite installer assembly failed.' }
python .github/scripts/build-suite-package.py manifest $assembly "$assembly/release-manifest.json"
if ($LASTEXITCODE -ne 0) { throw 'Suite manifest assembly failed.' }
if ($AllowPrerelease) {
  $preview = Get-Content -Raw "$assembly/release-manifest.json" | ConvertFrom-Json
  $preview.releaseTag = $Tag
  $preview | ConvertTo-Json -Depth 30 | Set-Content -Encoding utf8 "$assembly/release-manifest.json"
}
$assets = Join-Path $Output 'assets'
New-Item -ItemType Directory -Path $assets | Out-Null
$manifest = Get-Content -Raw "$assembly/release-manifest.json" | ConvertFrom-Json
foreach ($name in @($manifest.products.portable.name) + @($manifest.setup.name,$manifest.notices.name,'release-manifest.json')) { Copy-Item -LiteralPath (Join-Path $assembly $name) -Destination $assets }
$evidence = Join-Path $Output 'evidence'
New-Item -ItemType Directory -Path $evidence | Out-Null
Copy-Item '.github/scripts/windows-packaged-smoke-config.json','apps/products.json','.github/scripts/suite-build-tools.json' $evidence
if ($AllowPrerelease) { return } # Never label a preview build as an accepted stable candidate.
python .github/scripts/build-candidate-metadata.py --assets $assets --tag $Tag --commit $Source --repository $env:GITHUB_REPOSITORY --workflow-run $env:GITHUB_RUN_ID --output "$evidence/candidate-metadata.json"
if ($LASTEXITCODE -ne 0) { throw 'Candidate provenance failed.' }
python .github/scripts/verify-downloaded-release.py --assets $assets --release "$evidence/candidate-metadata.json" --config "$evidence/windows-packaged-smoke-config.json" --tag $Tag --commit $Source --artifact-kind candidate --repository $env:GITHUB_REPOSITORY --workflow-run $env:GITHUB_RUN_ID > "$evidence/assembly.json"
if ($LASTEXITCODE -ne 0) { throw 'Independent candidate verification failed.' }
