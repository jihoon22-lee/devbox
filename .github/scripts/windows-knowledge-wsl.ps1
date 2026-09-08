param([switch]$Cleanup)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
  throw 'This fixture may register a distro only on a disposable GitHub-hosted runner'
}
$evidenceDir = Join-Path $env:GITHUB_WORKSPACE 'product-foundation-evidence'
New-Item -ItemType Directory -Force $evidenceDir | Out-Null
$stateFile = Join-Path $env:RUNNER_TEMP 'devbox-knowledge-wsl-owner.json'
if ($Cleanup) {
  if (-not (Test-Path -LiteralPath $stateFile)) { exit 0 }
  $state = Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
  if ($state.name -notmatch '^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$' -or $state.runId -ne $env:GITHUB_RUN_ID) { throw 'Unexpected fixture owner' }
  $marker = Join-Path $state.install 'devbox-fixture-owner.txt'
  if ((Get-Content -LiteralPath $marker -Raw) -ne $state.name) { throw 'Fixture directory owner changed' }
  $registered = ((& wsl.exe --list --quiet) -join "`n").Replace("`0", '').Split("`n") | ForEach-Object { $_.Trim() }
  if ($registered -contains $state.name) {
    & wsl.exe --terminate $state.name
    & wsl.exe --unregister $state.name
    if ($LASTEXITCODE -ne 0) { throw 'Owned fixture distro cleanup failed' }
  }
  @{ ownedDistroUnregistered = $true; version = 1; runId = $state.runId } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $evidenceDir 'knowledge-wsl-cleanup.json')
  Remove-Item -LiteralPath $stateFile
  exit 0
}
if (Test-Path -LiteralPath $stateFile) { throw 'A fixture owner record already exists' }
# Canonical's fixed 24.04 LTS image and published SHA256SUMS. This image is used
# only as an isolated filesystem fixture; no package installation or networking
# is part of the test. Never turn an unavailable WSL host into a skipped PASS.
$url = 'https://cloud-images.ubuntu.com/wsl/releases/noble/current/ubuntu-noble-wsl-amd64-24.04lts.rootfs.tar.gz'
$expected = '2a790896740b14d637dbdc583cce1ba081ac53b9e9cdb46dc09a2f73abbd9934'
$name = 'DevboxKnowledgeFixture-' + $env:GITHUB_RUN_ID + '-' + ([guid]::NewGuid().ToString('N').Substring(0, 12))
$install = Join-Path $env:RUNNER_TEMP $name
New-Item -ItemType Directory $install | Out-Null
[IO.File]::WriteAllText((Join-Path $install 'devbox-fixture-owner.txt'), $name)
@{ name = $name; install = $install; runId = $env:GITHUB_RUN_ID } | ConvertTo-Json | Set-Content -Encoding utf8 $stateFile
$archive = Join-Path $install 'rootfs.tar.gz'
Invoke-WebRequest -Uri $url -OutFile $archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'WSL rootfs digest mismatch' }
& wsl.exe --import $name (Join-Path $install 'distribution') $archive --version 1
if ($LASTEXITCODE -ne 0) { throw 'Could not import the dedicated WSL1 fixture' }
& wsl.exe --distribution $name --user root --exec /bin/mkdir -p /home/devbox-fixture
if ($LASTEXITCODE -ne 0) { throw 'Could not prepare the dedicated WSL fixture directory' }
& wsl.exe --distribution $name --user root --exec /bin/uname -a
if ($LASTEXITCODE -ne 0) { throw 'The WSL fixture did not execute' }
"DEVBOX_KNOWLEDGE_WSL_DISTRO=$name" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
@{ distribution = $name; version = 1; rootfsUrl = $url; rootfsSha256 = $expected; result = 'prepared'; boundary = 'Actual WSL1 UNC filesystem; not WSL2 VM/suspend acceptance' } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $evidenceDir 'knowledge-wsl-host.json')
