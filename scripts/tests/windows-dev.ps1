param([string]$Scripts = (Split-Path $PSScriptRoot -Parent))
$ErrorActionPreference = 'Stop'
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
        $rejected = $false
        try { & (Join-Path $source 'scripts\win-dev.ps1') -WinRoot $target } catch { $rejected = $true }
        if (!$rejected -or (Test-Path (Join-Path $target 'packaged'))) { throw "Failed to reject $case" }
        # Restore the sentinel for the next failure case; source sync cannot produce it.
        Set-Content (Join-Path $target 'packaged') 'sentinel'
    }
    Write-Host 'Windows dev orchestration tests passed (fake WSL/build commands)'
} finally {
    Remove-Item Function:\wsl.exe
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
