param([string]$Script = (Join-Path $PSScriptRoot '..\win-sync.ps1'))
$ErrorActionPreference = 'Stop'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devbox-sync-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $fixture | Out-Null
function Reject([scriptblock]$Action) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (!$rejected) { throw 'Expected unsafe sync to fail' }
}
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
