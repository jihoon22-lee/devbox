[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$StagingRoot, [string[]]$AppIds = @())
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Product packages require Windows.' }
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if ([IO.Path]::IsPathRooted($StagingRoot)) { throw 'Staging must be repository-relative.' }
$staging = [IO.Path]::GetFullPath((Join-Path $repository $StagingRoot))
if (-not $staging.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $staging)) { throw 'New private repository staging required.' }
$catalog = Get-Content -Raw (Join-Path $repository 'apps/catalog.json') | ConvertFrom-Json
$products = @('devbox-workspace','devbox-api-studio','devbox-knowledge','devbox-control-center')
$actual = @($catalog.apps | Where-Object release | ForEach-Object id)
if ($actual.Count -ne 4 -or @($actual | Where-Object { $_ -notin $products }).Count -ne 0 -or @($actual | Sort-Object -Unique).Count -ne 4) { throw 'Four-product catalog required.' }
if ($AppIds.Count -eq 0) { $AppIds = $products }
if (@($AppIds | Sort-Object -Unique).Count -ne $AppIds.Count -or @($AppIds | Where-Object { $_ -notin $products }).Count -ne 0) { throw 'Invalid product shard.' }
$source = (git -C $repository rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $source -notmatch '^[a-f0-9]{40}$') { throw 'Exact source required.' }
New-Item -ItemType Directory -Path $staging | Out-Null
$receipt = [ordered]@{schemaVersion=1;sourceSha=$source;profile='release';products=@();files=@()}
Push-Location $repository
try {
  foreach ($id in $AppIds) {
    $definition = $catalog.apps | Where-Object id -CEQ $id
    if ($definition.appDir -cne "apps/$id" -or $definition.cargoPackage -cne $id) { throw 'Noncanonical product location.' }
    pnpm --filter "${id}^..." --if-present build
    if ($LASTEXITCODE -ne 0) { throw "Shared frontend build failed: $id" }
    pnpm --filter $id tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "Product build failed: $id" }
    $product = $id.Substring(7)
    $destination = Join-Path $staging $product
    New-Item -ItemType Directory -Path $destination | Out-Null
    Copy-Item -LiteralPath "target/release/$id.exe" -Destination $destination
    if ($product -eq 'workspace') {
      New-Item -ItemType Directory -Path "$destination/resources/wsl" -Force | Out-Null
      Copy-Item -LiteralPath 'apps/devbox-workspace/src-tauri/resources/wsl/manifest.json','apps/devbox-workspace/src-tauri/resources/wsl/devbox-workspace-wsl' -Destination "$destination/resources/wsl"
      # A private test executable is never part of the product archive.
      cargo build --locked --release -p devbox-editor-engine --bin fake-lsp-server
      if ($LASTEXITCODE -ne 0) { throw 'Private LSP fixture build failed.' }
    }
    if ($product -eq 'control-center') {
      cargo build --locked --release -p devbox-control-center --bin devbox-suite-bootstrap
      if ($LASTEXITCODE -ne 0) { throw 'Suite helper build failed.' }
      New-Item -ItemType Directory -Path "$destination/resources/suite" -Force | Out-Null
      Copy-Item -LiteralPath 'target/release/devbox-suite-bootstrap.exe' -Destination "$destination/resources/suite/"
    }
    $receipt.products += $product
    foreach ($file in Get-ChildItem -LiteralPath $destination -File -Recurse) {
      if (($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked build output.' }
      $receipt.files += @{name=[IO.Path]::GetRelativePath($staging,$file.FullName).Replace('\','/');size=$file.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
  }
  $receipt | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $staging 'shard-source.json')
} finally { Pop-Location }
