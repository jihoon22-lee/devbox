param([string]$Script = (Join-Path $PSScriptRoot '..\win-package.ps1'))
$ErrorActionPreference = 'Stop'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devbox-package-test-' + [guid]::NewGuid().ToString('N'))
$previousTarget = $env:CARGO_TARGET_DIR
$previousCI = $env:GITHUB_ACTIONS
$global:DevboxTestfailure = ''
$global:DevboxTestexitCode = 0
$global:DevboxTestwait = $true
$global:DevboxTestcalls = @()
function global:pnpm {
    $global:DevboxTestcalls += "pnpm $args"
    $global:LASTEXITCODE = if ($global:DevboxTestfailure -eq 'pnpm') { 7 } else { 0 }
}
function global:cargo {
    $global:DevboxTestcalls += "cargo $args"
    $global:LASTEXITCODE = if ($global:DevboxTestfailure -eq 'cargo') { 8 } else { 0 }
}
function global:Start-Process {
    param($FilePath, $ArgumentList, [switch]$PassThru)
    if ($FilePath -ne (Join-Path $fixture 'app\src-tauri\target\release\Devbox.exe') -or $ArgumentList -ne '--self-check') { throw 'Wrong self-check invocation' }
    $process = New-Object PSObject -Property @{ ExitCode = $global:DevboxTestexitCode; Handle = 1 }
    $process | Add-Member ScriptMethod WaitForExit { param($Timeout) return $global:DevboxTestwait }
    $process | Add-Member ScriptMethod Refresh {}
    $process | Add-Member ScriptMethod Kill {}
    return $process
}
function Reject([scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (!$rejected) { throw 'Expected build failure' }
    if ($env:CARGO_TARGET_DIR -ne 'fixture-original-target') { throw 'Target environment not restored' }
}
try {
    $resources = New-Item -ItemType Directory -Path (Join-Path $fixture 'app\src-tauri\resources') -Force
    $release = New-Item -ItemType Directory -Path (Join-Path $fixture 'app\src-tauri\target\release\bundle\nsis') -Force
    New-Item -ItemType Directory (Join-Path $fixture 'scripts') | Out-Null
    Copy-Item -LiteralPath $Script -Destination (Join-Path $fixture 'scripts\win-package.ps1')
    $copy = Join-Path $fixture 'scripts\win-package.ps1'
    @{ schema = 1; target = $fixture } | ConvertTo-Json | Set-Content (Join-Path $fixture '.devbox-win-owner.json')
    $env:CARGO_TARGET_DIR = 'fixture-original-target'
    $env:GITHUB_ACTIONS = ''
    Reject { & $copy }
    foreach ($name in @('devbox-linux', 'devbox-version.txt')) {
        Set-Content (Join-Path $resources $name) 'owned fixture'
        Copy-Item (Join-Path $resources $name) (Join-Path $fixture "app\src-tauri\target\release\$name")
    }
    Set-Content (Join-Path $fixture 'app\src-tauri\target\release\Devbox.exe') 'never executed'
    Set-Content (Join-Path $release 'Devbox-setup.exe') 'never executed'
    & $copy -Verify
    if ($env:CARGO_TARGET_DIR -ne 'fixture-original-target' -or !($global:DevboxTestcalls -match 'test .*--locked') -or !($global:DevboxTestcalls -match 'tauri build -- --locked')) { throw 'Build contract failed' }
    if (!($global:DevboxTestcalls -contains 'cargo test -p devbox-win --locked') -or !($global:DevboxTestcalls -contains 'cargo clippy -p devbox-win --all-targets --locked -- -D warnings')) { throw 'Windows notifications crate verification missing' }
    foreach ($failure in @('pnpm', 'cargo')) { $global:DevboxTestfailure = $failure; Reject { & $copy -Verify } }
    $global:DevboxTestfailure = ''; $global:DevboxTestexitCode = 3
    Reject { & $copy }
    $global:DevboxTestexitCode = 0; $global:DevboxTestwait = $false
    Reject { & $copy }
    $global:DevboxTestwait = $true
    Remove-Item (Join-Path $release 'Devbox-setup.exe')
    Reject { & $copy }
    Write-Host 'Windows package orchestration tests passed (fake commands; no native build)'
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    $env:GITHUB_ACTIONS = $previousCI
    Remove-Item Function:\pnpm, Function:\cargo, Function:\Start-Process
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
