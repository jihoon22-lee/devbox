[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$StagingRoot,
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-RepositoryChild([string]$RepositoryRoot, [string]$RelativePath, [string]$Label) {
  if ([IO.Path]::IsPathRooted($RelativePath)) { throw "$Label must be repository-relative" }
  $resolved = [IO.Path]::GetFullPath((Join-Path $RepositoryRoot $RelativePath))
  $prefix = $RepositoryRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$Label must remain inside the repository root"
  }
  return $resolved
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$stagingPath = Resolve-RepositoryChild $repositoryRoot $StagingRoot 'staging root'
$outputPath = Resolve-RepositoryChild $repositoryRoot $OutputRoot 'output root'
if (-not (Test-Path -LiteralPath $stagingPath -PathType Container)) { throw 'staging root is missing' }
if (Test-Path -LiteralPath $outputPath) { throw 'flat output root must not already exist' }
New-Item -ItemType Directory -Path $outputPath | Out-Null

$catalog = Get-Content -LiteralPath (Join-Path $repositoryRoot 'apps/catalog.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$apps = @($catalog.apps | Where-Object { $_.release -eq $true })
if ($apps.Count -ne 15) { throw 'release catalog must contain exactly 15 apps' }

foreach ($entry in $apps) {
  $appId = [string]$entry.id
  if ($appId -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$') { throw 'release catalog contains an unsafe app id' }
  $portable = @(Get-ChildItem -LiteralPath (Join-Path $stagingPath "$appId/portable") -File -Filter '*.exe')
  $installer = @(Get-ChildItem -LiteralPath (Join-Path $stagingPath "$appId/installer") -File -Filter '*.exe')
  if ($portable.Count -ne 1 -or $portable[0].Name -cne "$appId.exe") {
    throw "portable staging contract mismatch: $appId"
  }
  if ($installer.Count -ne 1 -or $installer[0].Name -notmatch "^$([regex]::Escape($appId))_\d+\.\d+\.\d+_x64-setup\.exe$") {
    throw "installer staging contract mismatch: $appId"
  }
  Copy-Item -LiteralPath $portable[0].FullName -Destination (Join-Path $outputPath $portable[0].Name)
  Copy-Item -LiteralPath $installer[0].FullName -Destination (Join-Path $outputPath $installer[0].Name)
}

foreach ($name in @('THIRD_PARTY_NOTICES.md', 'release-manifest.json')) {
  $source = Join-Path $stagingPath $name
  if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "staging asset is missing: $name" }
  Copy-Item -LiteralPath $source -Destination (Join-Path $outputPath $name)
}

$entries = @(Get-ChildItem -LiteralPath $outputPath -Force)
$files = @($entries | Where-Object { -not $_.PSIsContainer -and ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0 })
if ($entries.Count -ne 32 -or $files.Count -ne 32) {
  throw "flat package set must contain exactly 32 regular files (found $($files.Count) of $($entries.Count) entries)"
}
Write-Host "Flattened exactly 32 candidate assets in $outputPath" -ForegroundColor Green
