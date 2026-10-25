param([string]$Scripts = (Split-Path $PSScriptRoot -Parent), [switch]$PackageOnly)
$ErrorActionPreference = 'Stop'
function Reject([scriptblock]$Action, [switch]$Package) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (!$rejected) { throw 'Expected operation failure' }
    if ($Package) { Assert-PackageRestored }
}
if (!$PackageOnly) {
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
}
# Package retains its own tree, fake commands and environment restoration.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devbox-package-test-' + [guid]::NewGuid().ToString('N'))
$previous = @{}
foreach ($name in @('CARGO_TARGET_DIR', 'GITHUB_ACTIONS', 'CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'DEVBOX_APP_CHANNEL', 'TAURI_CONFIG')) { $previous[$name] = [Environment]::GetEnvironmentVariable($name) }
$packageCases = 0
function Assert-PackageRestored {
    foreach ($name in $packageExpected.Keys) {
        if ([Environment]::GetEnvironmentVariable($name) -cne $packageExpected[$name]) { throw "Environment not restored: $name" }
    }
    if ((Get-Location).Path -cne $packageCwd) { throw 'Working directory not restored' }
    foreach ($path in $global:DevboxTestconfigs) { if (Test-Path -LiteralPath $path) { throw 'Invocation config leaked' } }
}
function Package-Case([string]$Name, [scriptblock]$Action) {
    $global:DevboxTestcalls = @(); $global:DevboxTestchecks = @(); $global:DevboxTestconfigs = @(); $global:DevboxTestfailure = ''
    Get-ChildItem -LiteralPath $fixture -Filter receipt.json -Recurse -ErrorAction SilentlyContinue | Remove-Item
    & $Action
    Assert-PackageRestored
    $script:packageCases++
    Write-Host "PACKAGE PASS: $Name"
}
function Counts([int]$Install, [int]$Frontend, [int]$Native, [int]$Tauri, [int]$Checks) {
    $actual = @(@($global:DevboxTestcalls -eq 'pnpm install --frozen-lockfile').Count, @($global:DevboxTestcalls -eq 'pnpm --filter app build').Count, @($global:DevboxTestcalls -like 'cargo *').Count, @($global:DevboxTestcalls -like 'pnpm --filter app tauri *').Count, $global:DevboxTestchecks.Count)
    if (($actual -join ',') -ne (@($Install, $Frontend, $Native, $Tauri, $Checks) -join ',')) { throw "Unexpected install/frontend/native/Tauri/self-check counts: $actual" }
}
function global:pnpm {
    $global:DevboxTestcalls += "pnpm $args"
    $stage = if ($args[0] -eq 'install') { 'install' } elseif ($args -contains 'tauri') { "tauri:$env:DEVBOX_APP_CHANNEL" } else { 'frontend' }
    if ($args -contains 'tauri') {
        if ($env:CARGO_TARGET_DIR -cne (Join-Path $fixture "app\src-tauri\target\$env:DEVBOX_APP_CHANNEL")) { throw 'Wrong channel target' }
        if (@($global:DevboxTestcalls -like 'cargo *').Count -and @($global:DevboxTestcalls -like 'pnpm --filter app tauri *').Count -eq 1 -and $global:DevboxTestnativeConfig -cne $env:TAURI_CONFIG) { throw 'Native verification used different channel config' }
        $config = $env:TAURI_CONFIG | ConvertFrom-Json
        if ($env:DEVBOX_APP_CHANNEL -ne 'prod' -and ($config.identifier -cne "fixture.$env:DEVBOX_APP_CHANNEL" -or $config.productName -cne "Fixture $env:DEVBOX_APP_CHANNEL" -or $config.plugins.'deep-link'.desktop.schemes[0] -cne "fixture-$env:DEVBOX_APP_CHANNEL")) { throw 'Channel identity changed' }
        if ($args -contains 'build') {
            if (@($global:DevboxTestcalls -eq 'pnpm --filter app build').Count -ne 1) { throw 'Tauri before successful frontend preparation' }
            $path = $args[[array]::IndexOf($args, '--config') + 1]
            $global:DevboxTestconfigs += $path
            if (![IO.Path]::IsPathRooted($path) -or (Get-Content -LiteralPath $path -Raw) -cne $env:TAURI_CONFIG -or $config.build.beforeBuildCommand -cne '') { throw 'Invalid invocation config override' }
            if ($env:DEVBOX_APP_CHANNEL -ne 'prod' -and ($config.build.beforeDevCommand -cne 'fixture-dev-hook' -or $config.build.frontendDist -cne '../fixture-dist')) { throw 'Unrelated channel build fields changed' }
        } elseif ($env:DEVBOX_APP_CHANNEL -ne 'prod' -and ($config.build.beforeDevCommand -cne 'fixture-dev-hook' -or $config.build.PSObject.Properties['beforeBuildCommand'])) { throw 'Dev hook/config changed' }
    }
    $global:LASTEXITCODE = if ($global:DevboxTestfailure -eq $stage) { 7 } else { 0 }
}
function global:cargo {
    $global:DevboxTestcalls += "cargo $args"
    $global:DevboxTestnativeConfig = $env:TAURI_CONFIG
    if ($env:CARGO_BUILD_JOBS -ne '2' -or $env:RUST_TEST_THREADS -ne '2') { throw 'Native resource limits missing' }
    $global:LASTEXITCODE = if ($global:DevboxTestfailure -eq 'cargo') { 8 } else { 0 }
}
function global:Start-Process {
    param($FilePath, $ArgumentList, [switch]$PassThru)
    if ($FilePath -ne (Join-Path $fixture "app\src-tauri\target\$env:DEVBOX_APP_CHANNEL\release\Devbox.exe") -or ($ArgumentList -join ' ') -ne "--self-check --channel $env:DEVBOX_APP_CHANNEL") { throw 'Wrong self-check invocation' }
    $global:DevboxTestchecks += $env:DEVBOX_APP_CHANNEL
    $process = New-Object PSObject -Property @{ ExitCode = $(if ($global:DevboxTestfailure -eq 'self-check') { 3 } else { 0 }); Handle = 1 }
    $process | Add-Member ScriptMethod WaitForExit { param($Timeout) if ($Timeout -ne 60000) { throw 'Wrong timeout' }; return $global:DevboxTestfailure -ne 'timeout' }
    $process | Add-Member ScriptMethod Refresh {}
    $process | Add-Member ScriptMethod Kill { $global:DevboxTestkilled = $true }
    return $process
}
try {
    $resources = New-Item -ItemType Directory -Path (Join-Path $fixture 'app\src-tauri\resources') -Force
    $release = New-Item -ItemType Directory -Path (Join-Path $fixture 'app\src-tauri\target\prod\release\bundle\nsis') -Force
    New-Item -ItemType Directory (Join-Path $fixture 'scripts') | Out-Null
    Copy-Item -LiteralPath (Join-Path $Scripts 'win-package.ps1') -Destination (Join-Path $fixture 'scripts\win-package.ps1')
    $copy = Join-Path $fixture 'scripts\win-package.ps1'
    $marker = Join-Path $fixture '.devbox-win-owner.json'
    @{ schema = 1; target = $fixture } | ConvertTo-Json | Set-Content $marker
    $env:CARGO_TARGET_DIR = 'fixture-original-target'; $env:DEVBOX_APP_CHANNEL = 'previous-channel'; $env:TAURI_CONFIG = 'previous-config'
    $env:CARGO_BUILD_JOBS = '7'; $env:RUST_TEST_THREADS = '9'; $env:GITHUB_ACTIONS = ''
    $packageExpected = @{}
    foreach ($name in $previous.Keys) { $packageExpected[$name] = [Environment]::GetEnvironmentVariable($name) }
    $packageCwd = (Get-Location).Path
    Package-Case 'missing resources rejects before tools' { Reject { & $copy } -Package; Counts 0 0 0 0 0 }
    foreach ($name in @('devbox-linux', 'devbox-version.txt')) {
        Set-Content (Join-Path $resources $name) 'owned fixture'
        Copy-Item (Join-Path $resources $name) (Join-Path $fixture "app\src-tauri\target\prod\release\$name")
    }
    Set-Content (Join-Path $fixture 'app\src-tauri\target\prod\release\Devbox.exe') 'never executed'
    Set-Content (Join-Path $release 'Devbox-setup.exe') 'never executed'
    Set-Content (Join-Path $fixture 'app\package.json') '{"version":"fixture"}'
    foreach ($channel in @('accept', 'dev')) {
        Copy-Item (Join-Path $fixture 'app\src-tauri\target\prod') (Join-Path $fixture "app\src-tauri\target\$channel") -Recurse
        @{ identifier = "fixture.$channel"; productName = "Fixture $channel"; plugins = @{ 'deep-link' = @{ desktop = @{ schemes = @("fixture-$channel") } } }; build = @{ beforeDevCommand = 'fixture-dev-hook'; frontendDist = '../fixture-dist' } } | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $fixture "app\src-tauri\tauri.$channel.conf.json")
    }
    foreach ($channel in @('prod', 'accept', 'dev')) {
        Package-Case "single verified $channel" {
            & $copy -Verify -Channel $channel
            Counts 1 1 5 1 1
            if ((Get-Content (Join-Path $fixture "app\src-tauri\target\$channel\release\bundle\nsis\receipt.json") -Raw | ConvertFrom-Json).channel -ne $channel) { throw 'Wrong receipt channel' }
            if (!($global:DevboxTestcalls -contains 'cargo test -p devbox-win --locked') -or !($global:DevboxTestcalls -contains 'cargo clippy -p devbox-win --all-targets --locked -- -D warnings')) { throw 'Windows notifications crate verification missing' }
        }
    }
    Package-Case 'ordered prod accept prepares once' {
        & $copy -Verify -Channel @('prod', 'accept')
        Counts 1 1 5 2 2
        if (($global:DevboxTestchecks -join ',') -ne 'prod,accept') { throw 'Channel order changed' }
        foreach ($channel in @('prod', 'accept')) {
            $receipt = Get-Content (Join-Path $fixture "app\src-tauri\target\$channel\release\bundle\nsis\receipt.json") -Raw | ConvertFrom-Json
            if ($receipt.channel -ne $channel -or $receipt.version -ne 'fixture' -or $receipt.installers.'Devbox-setup.exe' -cne (Get-FileHash (Join-Path $release 'Devbox-setup.exe')).Hash.ToLowerInvariant()) { throw 'Receipt identity/hash lost' }
        }
    }
    Package-Case 'reverse channel order without verify' { & $copy -Channel @('accept', 'prod'); Counts 1 1 0 2 2; if (($global:DevboxTestchecks -join ',') -ne 'accept,prod') { throw 'Channel order changed' } }
    Package-Case 'dev default preserves beforeDevCommand' { & $copy -Mode dev; Counts 1 0 0 1 0 }
    Package-Case 'dev explicit prod retains base hook' { & $copy -Mode dev -Channel prod; Counts 1 0 0 1 0 }
    Package-Case 'null channels reject' { Reject { & $copy -Channel $null } -Package; Counts 0 0 0 0 0 }
    Package-Case 'dev verify retains frontend validation' { & $copy -Mode dev -Verify; Counts 1 1 5 1 0 }
    foreach ($invalid in @(@(), @('prod', 'prod'), @('prod', 'Prod'), @('bad'), @(''))) {
        Package-Case "invalid channels count=$($invalid.Count) [$($invalid -join ',')]" { Reject { & $copy -Channel $invalid } -Package; Counts 0 0 0 0 0 }
    }
    Package-Case 'dev multiple channels rejects' { Reject { & $copy -Mode dev -Channel @('prod', 'accept') } -Package; Counts 0 0 0 0 0 }
    foreach ($failure in @('install', 'frontend', 'cargo', 'tauri:prod', 'tauri:accept')) {
        Package-Case "failure $failure propagates" {
            $global:DevboxTestfailure = $failure
            Reject { & $copy -Verify -Channel @('prod', 'accept') } -Package
            switch ($failure) {
                install { Counts 1 0 0 0 0 }
                frontend { Counts 1 1 0 0 0 }
                cargo { Counts 1 1 1 0 0 }
                'tauri:prod' { Counts 1 1 5 1 0 }
                'tauri:accept' { Counts 1 1 5 2 1 }
            }
            if ($failure -eq 'tauri:accept' -and !(Test-Path (Join-Path $release 'receipt.json'))) { throw 'Earlier channel receipt lost' }
            if (Test-Path (Join-Path $fixture 'app\src-tauri\target\accept\release\bundle\nsis\receipt.json')) { throw 'Failed batch published later receipt' }
        }
    }
    foreach ($failure in @('self-check', 'timeout')) {
        Package-Case "failure $failure stops later channel" {
            $global:DevboxTestfailure = $failure; $global:DevboxTestkilled = $false
            Reject { & $copy -Channel @('prod', 'accept') } -Package
            Counts 1 1 0 1 1
            if ($failure -eq 'timeout' -and !$global:DevboxTestkilled) { throw 'Timed-out process not killed' }
        }
    }
    Package-Case 'built resource hash mismatch rejects before self-check' {
        $binary = Join-Path $fixture 'app\src-tauri\target\prod\release\devbox-linux'
        Set-Content $binary 'different'
        try { Reject { & $copy } -Package; Counts 1 1 0 1 0 } finally { Copy-Item (Join-Path $resources 'devbox-linux') $binary -Force }
    }
    Package-Case 'failed config directory creation preserves unowned sentinel' {
        $global:DevboxTestcollision = $null
        function global:New-Item {
            param($ItemType, $Path)
            $global:DevboxTestcollision = $Path
            Microsoft.PowerShell.Management\New-Item -ItemType $ItemType -Path $Path | Out-Null
            Set-Content -LiteralPath (Join-Path $Path 'sentinel') 'other owner'
            throw 'Injected directory creation failure'
        }
        try {
            Reject { & $copy } -Package
            Counts 1 1 0 0 0
            if (!(Test-Path (Join-Path $global:DevboxTestcollision 'sentinel')) -or (Get-Content (Join-Path $global:DevboxTestcollision 'sentinel')) -ne 'other owner') { throw 'Failed creation removed unowned data' }
        } finally {
            Remove-Item Function:\New-Item
            if (Test-Path -LiteralPath $global:DevboxTestcollision) { Remove-Item -LiteralPath $global:DevboxTestcollision -Recurse -Force }
        }
    }
    Package-Case 'missing executable rejects before self-check' {
        $exe = Join-Path $fixture 'app\src-tauri\target\prod\release\Devbox.exe'
        Remove-Item $exe
        try { Reject { & $copy } -Package; Counts 1 1 0 1 0 } finally { Set-Content $exe 'never executed' }
    }
    Package-Case 'missing installer rejects receipt' {
        Remove-Item (Join-Path $release 'Devbox-setup.exe')
        Reject { & $copy } -Package
        Counts 1 1 0 1 1
        if (Test-Path (Join-Path $release 'receipt.json')) { throw 'Missing installer produced receipt' }
    }
    Package-Case 'unowned tree rejects before tools' { Remove-Item -LiteralPath $marker -Force; Reject { & $copy } -Package; Counts 0 0 0 0 0 }
    Write-Host "Package orchestration: $packageCases cases passed (fake tools; no native build)"
} finally {
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name]) }
    Remove-Item Function:\pnpm, Function:\cargo, Function:\Start-Process
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
if ($PackageOnly) { return }
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
