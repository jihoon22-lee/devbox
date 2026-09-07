[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
  throw 'Performance fixtures require disposable GitHub-hosted Windows'
}
. "$PSScriptRoot/windows-installer-helpers.ps1"
$baseline = Read-Json "$PSScriptRoot/product-foundation-baseline.json"
$root = Join-Path $env:GITHUB_WORKSPACE 'performance-baseline'
if (Test-Path -LiteralPath $root) { Fail 'performance scratch already exists' }
$assets = Join-Path $root 'assets'
[IO.Directory]::CreateDirectory($assets) | Out-Null
$sourceRoot = Join-Path $env:GITHUB_WORKSPACE 'baseline-source'
$sourceCommit = & git -C $sourceRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $sourceCommit -cne $baseline.commit) { Fail 'baseline source checkout mismatch' }
$config = Join-Path $sourceRoot '.github/scripts/windows-packaged-smoke-config.json'
$tagRef = (& gh api "repos/jihoon22-lee/devbox/git/ref/tags/$($baseline.tag)" | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0) { Fail 'baseline tag lookup failed' }
$tagObject = $tagRef.object
for ($depth = 0; $tagObject.type -eq 'tag' -and $depth -lt 3; $depth += 1) {
  $tag = (& gh api "repos/jihoon22-lee/devbox/git/tags/$($tagObject.sha)" | ConvertFrom-Json)
  if ($LASTEXITCODE -ne 0) { Fail 'baseline annotated tag lookup failed' }
  $tagObject = $tag.object
}
if ($tagObject.type -ne 'commit' -or $tagObject.sha -cne $baseline.commit) { Fail 'baseline tag identity mismatch' }
& gh release download $baseline.tag --repo jihoon22-lee/devbox --dir $assets
if ($LASTEXITCODE -ne 0) { Fail 'baseline asset download failed' }
if ((Sha256 (Join-Path $assets 'release-manifest.json')) -cne $baseline.manifestSha256) { Fail 'baseline manifest digest mismatch' }
$release = (& gh release view $baseline.tag --repo jihoon22-lee/devbox --json tagName,isDraft,isPrerelease,assets | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0) { Fail 'baseline release metadata lookup failed' }
$metadata = [ordered]@{
  tagName = $release.tagName; targetCommit = $baseline.commit; isDraft = [bool]$release.isDraft; isPrerelease = [bool]$release.isPrerelease
  assets = @($release.assets | ForEach-Object { [ordered]@{ name = $_.name; size = [int64]$_.size; digest = $_.digest } })
}
$metadataPath = Join-Path $root 'release-metadata.json'
Write-Report $metadata $metadataPath
$verification = Join-Path $root 'verification.json'
& python "$PSScriptRoot/verify-downloaded-release.py" --assets $assets --release $metadataPath --config $config --tag $baseline.tag --commit $baseline.commit --draft false --prerelease false | Set-Content -LiteralPath $verification -Encoding utf8
if ($LASTEXITCODE -ne 0) { Fail 'independent baseline asset verification failed' }
& node "$PSScriptRoot/windows-packaged-smoke.mjs" --config $config --verification $verification --assets $assets --output (Join-Path $root 'runtime.json') --runtime (Join-Path $root 'runtime') --tag $baseline.tag --commit $baseline.commit --performance "$PSScriptRoot/product-foundation-performance.json"
if ($LASTEXITCODE -ne 0) { Fail 'baseline execution or measurement budget failed; inspect private evidence' }
