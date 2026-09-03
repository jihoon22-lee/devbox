[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateNotNullOrEmpty()]
  [string]$StagingRoot,

  [string[]]$AppIds = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-RepositoryChild([string]$RepositoryRoot, [string]$RelativePath, [string]$Label) {
  if ([IO.Path]::IsPathRooted($RelativePath)) {
    throw "$Label must be relative to the repository root"
  }
  $resolved = [IO.Path]::GetFullPath((Join-Path $RepositoryRoot $RelativePath))
  $prefix = $RepositoryRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$Label must remain inside the repository root"
  }
  return $resolved
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$stagingPath = Resolve-RepositoryChild $repositoryRoot $StagingRoot 'staging root'

if (Test-Path -LiteralPath $stagingPath) {
  $existing = @(Get-ChildItem -LiteralPath $stagingPath -Force)
  if ($existing.Count -ne 0) {
    throw 'staging root must be absent or empty'
  }
} else {
  New-Item -ItemType Directory -Path $stagingPath | Out-Null
}

Push-Location $repositoryRoot
try {
  $catalog = Get-Content -LiteralPath 'apps/catalog.json' -Raw -Encoding UTF8 | ConvertFrom-Json
  $releaseApps = @($catalog.apps | Where-Object { $_.release -eq $true })
  if ($releaseApps.Count -ne 15) {
    throw "release catalog must contain exactly 15 apps (found $($releaseApps.Count))"
  }

  $releaseById = @{}
  foreach ($entry in $releaseApps) {
    $appId = [string]$entry.id
    if ($appId -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$') {
      throw 'release catalog contains an unsafe app id'
    }
    $cargoPackage = [string]$entry.cargoPackage
    if ($cargoPackage -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$') {
      throw "release catalog contains an unsafe Cargo package: $appId"
    }
    $expectedAppDir = "apps/$appId"
    if ([string]$entry.appDir -cne $expectedAppDir) {
      throw "release catalog appDir mismatch: $appId"
    }
    if ($releaseById.ContainsKey($appId)) {
      throw "release catalog contains a duplicate app id: $appId"
    }
    $releaseById[$appId] = $entry
  }

  if ($AppIds.Count -eq 0) {
    $apps = $releaseApps
    $includeNotices = $true
  } else {
    if ($AppIds.Count -gt $releaseApps.Count) {
      throw 'requested app count exceeds the release catalog'
    }
    $requested = @{}
    $apps = @()
    foreach ($appId in $AppIds) {
      if ($appId -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$') {
        throw 'requested app list contains an unsafe app id'
      }
      if ($requested.ContainsKey($appId)) {
        throw "requested app list contains a duplicate: $appId"
      }
      if (-not $releaseById.ContainsKey($appId)) {
        throw "requested app is not in the release catalog: $appId"
      }
      $requested[$appId] = $true
      $apps += $releaseById[$appId]
    }
    $includeNotices = $false
  }

  foreach ($entry in $apps) {
    $appId = [string]$entry.id
    $cargoPackage = [string]$entry.cargoPackage
    $expectedAppDir = "apps/$appId"
    Write-Host "Building $appId" -ForegroundColor Cyan
    Push-Location $expectedAppDir
    try {
      & pnpm tauri build --bundles nsis
      if ($LASTEXITCODE -ne 0) {
        throw "build failed: $appId"
      }
    } finally {
      Pop-Location
    }

    $config = Get-Content -LiteralPath "$expectedAppDir/src-tauri/tauri.conf.json" -Raw -Encoding UTF8 | ConvertFrom-Json
    $productName = [string]$config.productName
    $version = [string]$config.version
    if (
      [string]::IsNullOrWhiteSpace($productName) -or
      $productName -ne [IO.Path]::GetFileName($productName) -or
      $productName -in @('.', '..') -or
      $version -notmatch '^\d+\.\d+\.\d+$'
    ) {
      throw "Tauri package identity is unsafe: $appId"
    }
    $portable = Join-Path $repositoryRoot "target/release/$cargoPackage.exe"
    $installer = Join-Path $repositoryRoot "target/release/bundle/nsis/${productName}_${version}_x64-setup.exe"
    if (-not (Test-Path -LiteralPath $portable -PathType Leaf)) {
      throw "portable output is missing: $appId"
    }
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
      throw "installer output is missing: $appId"
    }

    $portableOutput = Join-Path $stagingPath "$appId/portable"
    $installerOutput = Join-Path $stagingPath "$appId/installer"
    New-Item -ItemType Directory -Path $portableOutput, $installerOutput | Out-Null
    Move-Item -LiteralPath $portable -Destination (Join-Path $portableOutput "$appId.exe")
    Move-Item -LiteralPath $installer -Destination (Join-Path $installerOutput "${appId}_${version}_x64-setup.exe")
  }

  if ($includeNotices) {
    Copy-Item -LiteralPath 'THIRD_PARTY_NOTICES.md' -Destination (Join-Path $stagingPath 'THIRD_PARTY_NOTICES.md')
  }
} finally {
  Pop-Location
}

$noticeDescription = if ($includeNotices) { ' with notices' } else { '' }
Write-Host "Staged $($apps.Count) Windows app pairs$noticeDescription in $stagingPath" -ForegroundColor Green
