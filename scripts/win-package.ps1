# Called from an owned sync tree, or a disposable hosted runner checkout.
param([ValidateSet('dev', 'build')][string]$Mode = 'build', [switch]$Verify, [ValidateSet('prod', 'accept', 'dev')][string[]]$Channel = @($(if ($Mode -eq 'dev') { 'dev' } else { 'prod' })))
$ErrorActionPreference = 'Stop'
if (!$Channel.Count -or @($Channel | Sort-Object -Unique).Count -ne $Channel.Count -or ($Mode -eq 'dev' -and $Channel.Count -ne 1)) { throw 'Channels must be nonempty, unique, and single for dev mode' }
$Channel = @($Channel | ForEach-Object { $_.ToLowerInvariant() })
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).ProviderPath
if ($env:GITHUB_ACTIONS -ne 'true') {
    $marker = Join-Path $repo '.devbox-win-owner.json'
    if (!(Test-Path -LiteralPath $marker)) { throw 'Build requires an owned Windows sync tree' }
    $owner = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($owner.schema -ne 1 -or $owner.target -cne $repo) { throw 'Invalid Windows tree owner' }
}
function Run([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed ($LASTEXITCODE)" }
}
$saved = @{}
foreach ($name in @('CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'DEVBOX_APP_CHANNEL', 'TAURI_CONFIG')) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
$configDirectory = $null
Push-Location $repo
try {
    $env:CARGO_BUILD_JOBS = '2'
    $env:RUST_TEST_THREADS = '2'
    foreach ($name in @('devbox-linux', 'devbox-version.txt')) {
        $resource = Join-Path $repo "app\src-tauri\resources\$name"
        if (!(Test-Path -LiteralPath $resource -PathType Leaf) -or (Get-Item -LiteralPath $resource).Length -eq 0) { throw "Missing resource: $resource" }
    }
    for ($index = 0; $index -lt $Channel.Count; $index++) {
        $selected = $Channel[$index]
        $env:CARGO_TARGET_DIR = Join-Path $repo "app\src-tauri\target\$selected"
        $env:DEVBOX_APP_CHANNEL = $selected
        $env:TAURI_CONFIG = if ($selected -eq 'prod') { '{}' } else { Get-Content "app/src-tauri/tauri.$selected.conf.json" -Raw }
        if ($index -eq 0) {
            Run pnpm @('install', '--frozen-lockfile')
            if ($Mode -eq 'build' -or $Verify) { Run pnpm @('--filter', 'app', 'build') }
        }
        $configArgs = @(if ($selected -ne 'prod') { @('--config', "src-tauri/tauri.$selected.conf.json") })
        if ($Mode -eq 'build') {
            # Only this successful invocation may suppress Tauri's repeated frontend hook.
            if (!$configDirectory) {
                $candidate = Join-Path ([IO.Path]::GetTempPath()) ('devbox-package-config-' + [guid]::NewGuid().ToString('N'))
                $configDirectory = (New-Item -ItemType Directory $candidate).FullName
            }
            $override = $env:TAURI_CONFIG | ConvertFrom-Json
            if (!$override.build) { $override | Add-Member NoteProperty build ([pscustomobject]@{}) -Force }
            $override.build | Add-Member NoteProperty beforeBuildCommand '' -Force
            $env:TAURI_CONFIG = $override | ConvertTo-Json -Depth 100 -Compress
            $configPath = Join-Path $configDirectory "$selected.json"
            [IO.File]::WriteAllText($configPath, $env:TAURI_CONFIG)
            $configArgs = @('--config', $configPath)
        }
        if ($Verify -and $index -eq 0) {
            Run cargo @('fmt', '--manifest-path', 'app/src-tauri/Cargo.toml', '--check')
            Run cargo @('clippy', '--manifest-path', 'app/src-tauri/Cargo.toml', '--all-targets', '--locked', '--', '-D', 'warnings')
            Run cargo @('test', '-p', 'devbox-win', '--locked')
            Run cargo @('clippy', '-p', 'devbox-win', '--all-targets', '--locked', '--', '-D', 'warnings')
            Run cargo @('test', '--manifest-path', 'app/src-tauri/Cargo.toml', '--locked', '--lib')
        }
        Run pnpm (@('--filter', 'app', 'tauri', $Mode) + $configArgs + @('--', '--locked'))
        if ($Mode -eq 'build') {
            $release = Join-Path $env:CARGO_TARGET_DIR 'release'
            foreach ($name in @('Devbox.exe', 'devbox-linux', 'devbox-version.txt')) {
                $file = Join-Path $release $name
                if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing built file: $name" }
                if ($name -ne 'Devbox.exe' -and (Get-FileHash $file).Hash -ne (Get-FileHash "app/src-tauri/resources/$name").Hash) { throw "Built resource differs: $name" }
            }
            $process = Start-Process -FilePath (Join-Path $release 'Devbox.exe') -ArgumentList @('--self-check', '--channel', $selected) -PassThru
            $null = $process.Handle # Keep the process handle until ExitCode is collected (Windows PowerShell 5.1).
            if (!$process.WaitForExit(60000)) { $process.Kill(); throw 'Self-check timed out' }
            $process.Refresh()
            if ($process.ExitCode -ne 0) { throw "Self-check failed ($($process.ExitCode))" }
            $installers = @(Get-ChildItem -Path (Join-Path $release 'bundle\nsis\*.exe') -File)
            if (!$installers.Count) { throw 'NSIS installer missing' }
            $hashes = @{}
            foreach ($file in $installers) { $hashes[$file.Name] = (Get-FileHash $file.FullName).Hash.ToLowerInvariant() }
            @{ channel = $selected; source_sha = $env:SOURCE_SHA; run_id = $env:GITHUB_RUN_ID; repository = $env:GITHUB_REPOSITORY; workflow = '.github/workflows/windows.yml'; version = (Get-Content app/package.json -Raw | ConvertFrom-Json).version; installers = $hashes } | ConvertTo-Json | Set-Content (Join-Path $release 'bundle\nsis\receipt.json') -Encoding utf8
            $installers | ForEach-Object { Write-Host $_.FullName }
        }
    }
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    Pop-Location
    if ($configDirectory) { Remove-Item -LiteralPath $configDirectory -Recurse -Force }
}
