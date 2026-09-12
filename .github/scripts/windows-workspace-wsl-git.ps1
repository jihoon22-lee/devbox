$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
  throw 'Disposable GitHub runner required'
}
$state = Get-Content -LiteralPath (Join-Path $env:RUNNER_TEMP 'devbox-knowledge-wsl-owner.json') -Raw | ConvertFrom-Json
if ($state.runId -ne $env:GITHUB_RUN_ID -or $state.name -notmatch '^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$' -or $state.name -ne $env:DEVBOX_KNOWLEDGE_WSL_DISTRO) {
  throw 'Only the current run-owned WSL fixture may receive test tools'
}
# Explicit disposable-fixture provisioning, never an application installation
# path. Package signatures and dependency resolution remain Ubuntu's apt policy.
& wsl.exe --distribution $state.name --user root --cd / --exec /usr/bin/apt-get update
if ($LASTEXITCODE -ne 0) { throw 'Could not refresh owned fixture package metadata' }
& wsl.exe --distribution $state.name --user root --cd / --exec /usr/bin/env DEBIAN_FRONTEND=noninteractive /usr/bin/apt-get install -y --no-install-recommends git python3
if ($LASTEXITCODE -ne 0) { throw 'Could not provision Git in the owned fixture' }
$version = & wsl.exe --distribution $state.name --user root --cd / --exec /usr/bin/git --version
if ($LASTEXITCODE -ne 0) { throw 'Owned fixture Git is unavailable' }
@{ runId = $state.runId; distribution = $state.name; git = ($version -join "`n").Trim(); boundary = 'Explicit disposable fixture only; no application auto-install' } |
  ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $env:GITHUB_WORKSPACE 'product-foundation-evidence/workspace-wsl-git-fixture.json')
