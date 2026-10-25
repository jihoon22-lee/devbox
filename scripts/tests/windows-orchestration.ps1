param([string]$Scripts = (Split-Path $PSScriptRoot -Parent))
$ErrorActionPreference = 'Stop'
function Reject([scriptblock]$Action, [switch]$Package) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (!$rejected) { throw 'Expected operation failure' }
    if ($Package -and ($env:CARGO_TARGET_DIR -ne 'fixture-original-target' -or $env:DEVBOX_APP_CHANNEL -ne 'previous-channel' -or $env:TAURI_CONFIG -ne 'previous-config')) { throw 'Target environment not restored' }
}
# Sync retains its own GUID tree and junction cleanup.
$Script = Join-Path $Scripts 'win-sync.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devbox-sync-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $fixture | Out-Null
try {
    $source = New-Item -ItemType Directory (Join-Path $fixture 'source')
    Set-Content (Join-Path $source 'sample.txt') 'first'
    Set-Content (Join-Path $source '.git') 'gitdir: /owned/fixture'
    New-Item -ItemType Directory (Join-Path $source 'node_modules') | Out-Null
    Set-Content (Join-Path $source 'node_modules\linux-only') 'fixture'
    $target = Join-Path $fixture 'target'
    $unowned = New-Item -ItemType Directory (Join-Path $fixture 'unowned')
    Reject { & $Script -SourceRoot $source -WinRoot $unowned }
    Set-Content (Join-Path $unowned 'preserve.txt') 'user data'
    Reject { & $Script -SourceRoot $source -WinRoot $unowned }
    if ((Get-Content (Join-Path $unowned 'preserve.txt')) -ne 'user data') { throw 'Unowned data changed' }
    Reject { & $Script -SourceRoot $source -WinRoot $source }
    Reject { & $Script -SourceRoot $source -WinRoot (Join-Path $source 'child') }
    Reject { & $Script -SourceRoot $source -WinRoot $env:USERPROFILE }
    Reject { & $Script -SourceRoot $source -WinRoot ([IO.Path]::GetPathRoot($fixture)) }
    Reject { & $Script -SourceRoot $source -WinRoot '\\localhost\C$\unsafe' }
    & $Script -SourceRoot $source -WinRoot $target
    if ((Test-Path (Join-Path $target '.git')) -or (Test-Path (Join-Path $target 'node_modules'))) { throw 'Platform-specific files copied' }
    New-Item -ItemType Directory (Join-Path $target 'node_modules') | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $target 'node_modules\owned-link') -Target $unowned | Out-Null
    Set-Content (Join-Path $target 'stale.txt') 'old'
    Set-Content (Join-Path $source 'sample.txt') 'second'
    & $Script -SourceRoot $source -WinRoot $target
    if ((Test-Path (Join-Path $target 'stale.txt')) -or (Get-Content (Join-Path $target 'sample.txt')) -ne 'second') { throw 'Mirror failed' }
    if (!(Test-Path (Join-Path $target 'node_modules\owned-link\preserve.txt'))) { throw 'Excluded dependencies changed' }
    [IO.Directory]::Delete((Join-Path $target 'node_modules\owned-link'))
    $other = New-Item -ItemType Directory (Join-Path $fixture 'other')
    Reject { & $Script -SourceRoot $other -WinRoot $target }
    $junction = Join-Path $fixture 'junction'
    New-Item -ItemType Junction -Path $junction -Target $unowned | Out-Null
    Reject { & $Script -SourceRoot $source -WinRoot (Join-Path $junction 'child') }
    [IO.Directory]::Delete($junction)
    function global:robocopy { $global:LASTEXITCODE = 16 }
    Reject { & $Script -SourceRoot $source -WinRoot $target }
    Remove-Item Function:\robocopy
    Write-Host 'Windows sync ownership tests passed'
} finally {
    if (Test-Path (Join-Path $fixture 'target\node_modules\owned-link')) { [IO.Directory]::Delete((Join-Path $fixture 'target\node_modules\owned-link')) }
    if (Test-Path (Join-Path $fixture 'junction')) { [IO.Directory]::Delete((Join-Path $fixture 'junction')) }
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
# Package retains its own tree, fake commands and environment restoration.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devbox-package-test-' + [guid]::NewGuid().ToString('N'))
$previous = @{}
foreach ($name in @('CARGO_TARGET_DIR', 'GITHUB_ACTIONS', 'CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'DEVBOX_APP_CHANNEL', 'TAURI_CONFIG')) { $previous[$name] = [Environment]::GetEnvironmentVariable($name) }
$global:DevboxTestfailure = ''
$global:DevboxTestexitCode = 0
$global:DevboxTestwait = $true
$global:DevboxTestcalls = @()
function global:pnpm {
    $global:DevboxTestcalls += "pnpm $args"
    if ($args -contains 'tauri' -and $env:TAURI_CONFIG -ne $(if ($env:DEVBOX_APP_CHANNEL -eq 'prod') { '{}' } else { '{"fixture":true}' })) { throw 'Wrong channel config' }
    $global:LASTEXITCODE = if ($global:DevboxTestfailure -eq 'pnpm') { 7 } else { 0 }
}
function global:cargo {
    $global:DevboxTestcalls += "cargo $args"
    $global:LASTEXITCODE = if ($global:DevboxTestfailure -eq 'cargo') { 8 } else { 0 }
}
function global:Start-Process {
    param($FilePath, $ArgumentList, [switch]$PassThru)
    if ($FilePath -ne (Join-Path $fixture "app\src-tauri\target\$env:DEVBOX_APP_CHANNEL\release\Devbox.exe") -or ($ArgumentList -join ' ') -ne "--self-check --channel $env:DEVBOX_APP_CHANNEL") { throw 'Wrong self-check invocation' }
    $process = New-Object PSObject -Property @{ ExitCode = $global:DevboxTestexitCode; Handle = 1 }
    $process | Add-Member ScriptMethod WaitForExit { param($Timeout) return $global:DevboxTestwait }
    $process | Add-Member ScriptMethod Refresh {}
    $process | Add-Member ScriptMethod Kill {}
    return $process
}
try {
    $resources = New-Item -ItemType Directory -Path (Join-Path $fixture 'app\src-tauri\resources') -Force
    $release = New-Item -ItemType Directory -Path (Join-Path $fixture 'app\src-tauri\target\prod\release\bundle\nsis') -Force
    New-Item -ItemType Directory (Join-Path $fixture 'scripts') | Out-Null
    Copy-Item -LiteralPath (Join-Path $Scripts 'win-package.ps1') -Destination (Join-Path $fixture 'scripts\win-package.ps1')
    $copy = Join-Path $fixture 'scripts\win-package.ps1'
    @{ schema = 1; target = $fixture } | ConvertTo-Json | Set-Content (Join-Path $fixture '.devbox-win-owner.json')
    $env:CARGO_TARGET_DIR = 'fixture-original-target'
    $env:DEVBOX_APP_CHANNEL = 'previous-channel'
    $env:TAURI_CONFIG = 'previous-config'
    $env:GITHUB_ACTIONS = ''
    Reject { & $copy } -Package
    foreach ($name in @('devbox-linux', 'devbox-version.txt')) {
        Set-Content (Join-Path $resources $name) 'owned fixture'
        Copy-Item (Join-Path $resources $name) (Join-Path $fixture "app\src-tauri\target\prod\release\$name")
    }
    Set-Content (Join-Path $fixture 'app\src-tauri\target\prod\release\Devbox.exe') 'never executed'
    Set-Content (Join-Path $release 'Devbox-setup.exe') 'never executed'
    Set-Content (Join-Path $fixture 'app\package.json') '{"version":"fixture"}'
    foreach ($channel in @('accept', 'dev')) {
        Copy-Item (Join-Path $fixture 'app\src-tauri\target\prod') (Join-Path $fixture "app\src-tauri\target\$channel") -Recurse
        [IO.File]::WriteAllText((Join-Path $fixture "app\src-tauri\tauri.$channel.conf.json"), '{"fixture":true}')
    }
    foreach ($channel in @('prod', 'accept', 'dev')) {
        & $copy -Verify -Channel $channel
        if ((Get-Content (Join-Path $fixture "app\src-tauri\target\$channel\release\bundle\nsis\receipt.json") -Raw | ConvertFrom-Json).channel -ne $channel) { throw 'Wrong receipt channel' }
    }
    if ($env:CARGO_TARGET_DIR -ne 'fixture-original-target' -or !($global:DevboxTestcalls -match 'test .*--locked') -or !($global:DevboxTestcalls -match 'tauri build -- --locked')) { throw 'Build contract failed' }
    if (!($global:DevboxTestcalls -contains 'cargo test -p devbox-win --locked') -or !($global:DevboxTestcalls -contains 'cargo clippy -p devbox-win --all-targets --locked -- -D warnings')) { throw 'Windows notifications crate verification missing' }
    foreach ($failure in @('pnpm', 'cargo')) { $global:DevboxTestfailure = $failure; Reject { & $copy -Verify } -Package }
    $global:DevboxTestfailure = ''; $global:DevboxTestexitCode = 3
    Reject { & $copy } -Package
    $global:DevboxTestexitCode = 0; $global:DevboxTestwait = $false
    Reject { & $copy } -Package
    $global:DevboxTestwait = $true
    Remove-Item (Join-Path $release 'Devbox-setup.exe')
    Reject { & $copy } -Package
    Write-Host 'Windows package orchestration tests passed (fake commands; no native build)'
} finally {
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name]) }
    Remove-Item Function:\pnpm, Function:\cargo, Function:\Start-Process
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
# Dev retains its literal-quote path and independent fake WSL lifetime.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ("devbox-dev-test-'literal-" + [guid]::NewGuid().ToString('N'))
$global:DevboxTestVersion = '1.0.0-dev'
$global:DevboxTestFail = $false
$global:DevboxTestWslCalls = @()
function global:wsl.exe {
    $global:DevboxTestWslCalls += ,$args
    $global:LASTEXITCODE = 0
    if ($global:DevboxTestFail) { $global:LASTEXITCODE = 17; return }
    if ($args -contains 'wslpath') {
        if ($args -contains '-w') { return (Join-Path $fixture 'binary') }
        return "/fixture/with ' literal"
    }
    if ($args -contains '-c') {
        return (@{ target_directory = '/fixture/shared target'; packages = @(@{ name = 'devbox-cli'; version = $global:DevboxTestVersion }) } | ConvertTo-Json -Depth 4)
    }
}
try {
    $source = Join-Path $fixture 'source'
    New-Item -ItemType Directory (Join-Path $source 'scripts') -Force | Out-Null
    New-Item -ItemType Directory (Join-Path $source 'app') | Out-Null
    Set-Content (Join-Path $source 'app\package.json') '{"version":"1.0.0-dev"}'
    Set-Content (Join-Path $fixture 'binary') 'owned musl fixture'
    foreach ($name in @('win-sync.ps1', 'win-dev.ps1')) { Copy-Item (Join-Path $Scripts $name) (Join-Path $source 'scripts') }
    Set-Content (Join-Path $source 'scripts\win-package.ps1') 'param($Mode); Set-Content (Join-Path $PSScriptRoot "..\packaged") $Mode'
    $target = Join-Path $fixture 'target'
    & (Join-Path $source 'scripts\win-dev.ps1') -Mode build -WinRoot $target -Distro 'Fixture Distro'
    if ((Get-Content (Join-Path $target 'packaged')) -ne 'build') { throw 'Package helper not called' }
    $bytes = [IO.File]::ReadAllBytes((Join-Path $target 'app\src-tauri\resources\devbox-version.txt'))
    if ([Text.Encoding]::UTF8.GetString($bytes) -cne '1.0.0-dev') { throw 'Version must be BOM-free UTF8 without newline' }
    $buildCalls = @($global:DevboxTestWslCalls | Where-Object { $_ -contains 'scripts/build-linux.sh' })
    if ($buildCalls.Count -ne 1 -or $buildCalls[0][-1] -ne 'release' -or !($buildCalls[0] -contains "/fixture/with ' literal")) { throw 'WSL arguments changed or interpolated' }
    foreach ($case in @('version', 'exit')) {
        Remove-Item (Join-Path $target 'packaged')
        if ($case -eq 'version') { $global:DevboxTestVersion = 'bad' } else { $global:DevboxTestFail = $true }
        Reject { & (Join-Path $source 'scripts\win-dev.ps1') -WinRoot $target }
        if (Test-Path (Join-Path $target 'packaged')) { throw "Failed to reject $case" }
        # Restore the sentinel for the next failure case; source sync cannot produce it.
        Set-Content (Join-Path $target 'packaged') 'sentinel'
    }
    Write-Host 'Windows dev orchestration tests passed (fake WSL/build commands)'
} finally {
    Remove-Item Function:\wsl.exe
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
