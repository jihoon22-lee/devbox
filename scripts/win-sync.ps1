param(
    [string]$WinRoot = (Join-Path $env:LOCALAPPDATA ('Devbox\build-' + [guid]::NewGuid().ToString('N'))),
    [string]$SourceRoot = (Join-Path $PSScriptRoot '..')
)
$ErrorActionPreference = 'Stop'
$source = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $SourceRoot).ProviderPath).TrimEnd('\')
if ($WinRoot -notmatch '^[A-Za-z]:\\' -or $WinRoot.Substring(2).Contains(':')) { throw 'Destination must be an absolute local drive path' }
$target = [IO.Path]::GetFullPath($WinRoot).TrimEnd('\')
$forbidden = @([IO.Path]::GetPathRoot($target).TrimEnd('\'), $env:USERPROFILE, $env:LOCALAPPDATA, $env:APPDATA, $env:SystemRoot, $env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:ProgramData)
if ($forbidden -contains $target) { throw 'Refusing protected destination' }
if ($source -eq $target -or $source.StartsWith($target + '\', 'OrdinalIgnoreCase') -or $target.StartsWith($source + '\', 'OrdinalIgnoreCase')) { throw 'Source and destination overlap' }
function Assert-NoReparse([string]$Path) {
    $cursor = $Path
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse path rejected: $cursor" }
        }
        $cursor = Split-Path -Parent $cursor
    }
}
Assert-NoReparse $target
Assert-NoReparse $source
$markerName = '.devbox-win-owner.json'
$marker = Join-Path $target $markerName
if (Test-Path -LiteralPath $target) {
    if (!(Test-Path -LiteralPath $marker -PathType Leaf)) { throw 'Existing destination has no ownership marker; choose a new directory' }
    Assert-NoReparse $marker
    $owner = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($owner.schema -ne 1 -or $owner.source -cne $source -or $owner.target -cne $target) { throw 'Ownership marker does not match source and destination' }
    # Do not follow or mirror over junctions, even inside an owned tree.
    $pending = New-Object 'System.Collections.Generic.Queue[string]'
    $pending.Enqueue($target)
    while ($pending.Count) {
        foreach ($entry in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse entry rejected: $($entry.FullName)" }
            if ($entry.PSIsContainer -and $entry.Name -notin @('node_modules', 'target', 'dist', '.git', 'test-results', 'playwright-report')) { $pending.Enqueue($entry.FullName) }
        }
    }
} else {
    New-Item -ItemType Directory -Path $target | Out-Null
    @{ schema = 1; source = $source; target = $target } | ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding UTF8
}
& robocopy $source $target /MIR /XJ /XD node_modules target dist .git test-results playwright-report /XF $markerName .git /NFL /NDL /NJH /NJS /NP | Out-Host
if ($LASTEXITCODE -ge 8 -or $LASTEXITCODE -lt 0) { throw "robocopy failed ($LASTEXITCODE)" }
$global:LASTEXITCODE = 0
Write-Output $target
