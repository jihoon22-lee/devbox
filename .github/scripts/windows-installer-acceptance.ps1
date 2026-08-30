[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Config,
  [Parameter(Mandatory = $true)][string]$BaselineAssets,
  [Parameter(Mandatory = $true)][string]$BaselineMetadata,
  [Parameter(Mandatory = $true)][string]$CandidateAssets,
  [Parameter(Mandatory = $true)][string]$CandidateMetadata,
  [Parameter(Mandatory = $true)][string]$CandidateTag,
  [Parameter(Mandatory = $true)][string]$CandidateCommit,
  [Parameter(Mandatory = $true)][string]$Output,
  [Parameter(Mandatory = $true)][string]$ScratchRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
  throw [System.InvalidOperationException]::new($Message)
}

function Full-Path([string]$Path) {
  return [IO.Path]::GetFullPath($Path)
}

function Assert-Descendant([string]$Candidate, [string]$Root, [string]$Label) {
  $candidatePath = (Full-Path $Candidate).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $rootPath = (Full-Path $Root).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $prefix = $rootPath + [IO.Path]::DirectorySeparatorChar
  if (-not $candidatePath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    Fail "$Label must be inside the run-owned scratch root"
  }
}

function Assert-Plain-Existing-Path([string]$Path, [string]$Label) {
  $current = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  while ($null -ne $current) {
    if (($current.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      Fail "$Label contains a reparse point"
    }
    $parent = Split-Path -Parent $current.FullName
    if ([string]::IsNullOrWhiteSpace($parent) -or $parent -eq $current.FullName) { break }
    $current = Get-Item -LiteralPath $parent -Force -ErrorAction Stop
  }
}

function Sha256([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Public-Error([object]$ErrorValue) {
  $message = if ($ErrorValue -is [System.Management.Automation.ErrorRecord]) {
    $ErrorValue.Exception.Message
  } elseif ($ErrorValue -is [Exception]) {
    $ErrorValue.Message
  } else {
    [string]$ErrorValue
  }
  $message = $message -replace '(?i)[a-z]:\\[^\r\n]+', '<path>'
  if ($message.Length -gt 400) { return $message.Substring(0, 400) }
  return $message
}

function Write-Report([System.Collections.IDictionary]$Report, [string]$Path) {
  $json = $Report | ConvertTo-Json -Depth 100
  [IO.File]::WriteAllText($Path, "$json`n", [Text.UTF8Encoding]::new($false))
}

function Read-Json([string]$Path) {
  return Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json -Depth 100
}

function Assert-Safe-Leaf([string]$Name) {
  if ([string]::IsNullOrWhiteSpace($Name) -or $Name -ne [IO.Path]::GetFileName($Name)) {
    Fail 'release manifest contains an unsafe asset name'
  }
}

function Verify-Release(
  [string]$Assets,
  [string]$MetadataPath,
  [string]$ExpectedTag,
  [string]$ExpectedCommit,
  [int]$ExpectedApps,
  [bool]$ExpectedPrerelease,
  [object[]]$ConfiguredApps
) {
  $metadata = Read-Json $MetadataPath
  $manifestPath = Join-Path $Assets 'release-manifest.json'
  $manifest = Read-Json $manifestPath
  if ($metadata.tagName -cne $ExpectedTag -or $metadata.targetCommit -cne $ExpectedCommit) {
    Fail 'release metadata identity mismatch'
  }
  if ([bool]$metadata.isDraft -or [bool]$metadata.isPrerelease -ne $ExpectedPrerelease) {
    Fail 'release publication state mismatch'
  }
  if ($manifest.schemaVersion -ne 1 -or $manifest.releaseTag -cne $ExpectedTag) {
    Fail 'release manifest envelope mismatch'
  }
  $manifestApps = @($manifest.apps)
  if ($manifestApps.Count -ne $ExpectedApps) { Fail 'release manifest app count mismatch' }
  $configuredIds = @($ConfiguredApps | ForEach-Object { $_.id })
  $manifestIds = @($manifestApps | ForEach-Object { $_.id })
  if (@(Compare-Object $configuredIds $manifestIds -CaseSensitive).Count -ne 0) {
    Fail 'release manifest app identities mismatch'
  }

  $expected = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
  $byId = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
  foreach ($app in $manifestApps) {
    if ($app.id -notmatch '^[a-z0-9-]+$' -or $app.version -notmatch '^\d+\.\d+\.\d+$') {
      Fail 'release manifest contains an invalid app identity or version'
    }
    foreach ($kind in @('portable', 'installer')) {
      $asset = $app.$kind
      Assert-Safe-Leaf $asset.name
      if ([long]$asset.size -le 0 -or $asset.sha256 -notmatch '^[0-9a-f]{64}$') {
        Fail 'release manifest contains an invalid asset identity'
      }
      if ($expected.ContainsKey($asset.name)) { Fail 'release manifest contains duplicate assets' }
      $expected[$asset.name] = $asset
    }
    if ($app.portable.name -cne "$($app.id).exe") { Fail 'portable asset name mismatch' }
    if ($app.installer.name -cne "$($app.id)_$($app.version)_x64-setup.exe") {
      Fail 'installer asset name mismatch'
    }
    $byId[$app.id] = $app
  }

  $notices = $null
  $hasNotices = $manifest.PSObject.Properties.Name -contains 'notices'
  if (($ExpectedApps -eq 15) -ne $hasNotices) { Fail 'release notices contract mismatch' }
  if ($hasNotices) {
    $notices = $manifest.notices
    Assert-Safe-Leaf $notices.name
    if ($notices.name -cne 'THIRD_PARTY_NOTICES.md') { Fail 'notices asset name mismatch' }
    if ([long]$notices.size -le 0 -or $notices.sha256 -notmatch '^[0-9a-f]{64}$') {
      Fail 'release notices identity is invalid'
    }
    $expected[$notices.name] = $notices
  }
  $expected['release-manifest.json'] = [pscustomobject]@{
    name = 'release-manifest.json'
    size = (Get-Item -LiteralPath $manifestPath).Length
    sha256 = (Sha256 $manifestPath)
  }

  $downloaded = @(Get-ChildItem -LiteralPath $Assets -File)
  if ($downloaded.Count -ne $expected.Count) { Fail 'downloaded release asset count mismatch' }
  if (@(Compare-Object @($expected.Keys) @($downloaded.Name) -CaseSensitive).Count -ne 0) {
    Fail 'downloaded release asset names mismatch'
  }
  $remoteAssets = @($metadata.assets)
  if ($remoteAssets.Count -ne $expected.Count) { Fail 'remote release asset count mismatch' }
  $remote = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
  foreach ($asset in $remoteAssets) {
    if ($remote.ContainsKey($asset.name)) { Fail 'remote release contains duplicate assets' }
    $remote[$asset.name] = $asset
  }

  foreach ($name in $expected.Keys) {
    $path = Join-Path $Assets $name
    $item = Get-Item -LiteralPath $path
    $digest = Sha256 $path
    $declared = $expected[$name]
    if ($item.Length -ne [long]$declared.size -or $digest -cne $declared.sha256) {
      Fail 'downloaded release asset digest mismatch'
    }
    if (-not $remote.ContainsKey($name)) { Fail 'remote release asset is missing' }
    if ([long]$remote[$name].size -ne $item.Length -or $remote[$name].digest -cne "sha256:$digest") {
      Fail 'remote release asset identity mismatch'
    }
  }

  return [pscustomobject]@{
    tag = $ExpectedTag
    commit = $ExpectedCommit
    assets = $expected.Count
    apps = $manifestApps.Count
    manifestSha256 = Sha256 $manifestPath
    metadataSha256 = Sha256 $MetadataPath
    byId = $byId
    notices = $notices
  }
}

$registryRoots = @(
  'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'Registry::HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'Registry::HKEY_LOCAL_MACHINE\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)

function Get-Optional-Property([object]$Object, [string]$Name) {
  if ($null -eq $Object) { return '' }
  $properties = @($Object.PSObject.Properties.Match($Name))
  if ($properties.Count -eq 0 -or $null -eq $properties[0].Value) { return '' }
  return [string]$properties[0].Value
}

function Get-Uninstall-Entries {
  $entries = @()
  foreach ($root in $registryRoots) {
    if (-not (Test-Path -LiteralPath $root)) { continue }
    foreach ($key in @(Get-ChildItem -LiteralPath $root -ErrorAction Stop)) {
      $value = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction Stop
      $displayName = Get-Optional-Property $value 'DisplayName'
      if ([string]::IsNullOrWhiteSpace($displayName)) { continue }
      $entries += [pscustomobject]@{
        ProviderPath = $key.PSPath
        RegistryName = $key.Name
        DisplayName = $displayName
        DisplayVersion = Get-Optional-Property $value 'DisplayVersion'
        Publisher = Get-Optional-Property $value 'Publisher'
        DisplayIcon = Get-Optional-Property $value 'DisplayIcon'
        InstallLocation = Get-Optional-Property $value 'InstallLocation'
        UninstallString = Get-Optional-Property $value 'UninstallString'
      }
    }
  }
  return $entries
}

function Find-App-Entry([object]$App) {
  $matches = @(Get-Uninstall-Entries | Where-Object {
    $_.DisplayName -ieq $App.productName -or $_.DisplayName -ieq $App.id
  })
  if ($matches.Count -eq 0) { return $null }
  if ($matches.Count -ne 1) { Fail 'installer created ambiguous uninstall entries' }
  return $matches[0]
}

function Wait-App-Entry([object]$App, [bool]$Present) {
  for ($attempt = 0; $attempt -lt 40; $attempt += 1) {
    $entry = Find-App-Entry $App
    if ($Present -and $null -ne $entry) { return $entry }
    if (-not $Present -and $null -eq $entry) { return $null }
    Start-Sleep -Milliseconds 500
  }
  Fail 'installer registry state did not converge'
}

function Parse-Uninstaller([string]$Value) {
  if ([string]::IsNullOrWhiteSpace($Value)) { Fail 'uninstall command is missing' }
  $expanded = [Environment]::ExpandEnvironmentVariables($Value.Trim())
  if ($expanded -match '^"([^"]+\.exe)"') { return $Matches[1] }
  if ($expanded -match '^([^\s]+\.exe)') { return $Matches[1] }
  Fail 'uninstall command is malformed'
}

function Parse-Display-Icon([string]$Value) {
  if ([string]::IsNullOrWhiteSpace($Value)) { Fail 'display icon is missing' }
  $expanded = [Environment]::ExpandEnvironmentVariables($Value.Trim())
  if ($expanded -match '^"([^"]+\.exe)"(?:,\d+)?$') { return $Matches[1] }
  if ($expanded -match '^(.+\.exe)(?:,\d+)?$') { return $Matches[1] }
  Fail 'display icon is malformed'
}

function Allowed-Install-Root([string]$Path) {
  $full = Full-Path $Path
  foreach ($root in @($env:LOCALAPPDATA, $env:RUNNER_TEMP)) {
    if ([string]::IsNullOrWhiteSpace($root)) { continue }
    $prefix = (Full-Path $root).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { return $true }
  }
  return $false
}

function Get-App-Shortcut-Count([string]$Binary) {
  $roots = @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    [Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory)
  )
  $count = 0
  $shell = New-Object -ComObject WScript.Shell
  try {
    foreach ($root in $roots) {
      if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
      foreach ($shortcut in @(Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.lnk')) {
        try {
          $target = $shell.CreateShortcut($shortcut.FullName).TargetPath
          if (-not [string]::IsNullOrWhiteSpace($target) -and (Full-Path $target) -eq (Full-Path $Binary)) {
            $count += 1
          }
        } catch {}
      }
    }
  } finally {
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
  }
  return $count
}

function Get-Potential-Shortcut-Count([object]$App) {
  $roots = @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    [Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory)
  )
  $names = @($App.productName, $App.id)
  $count = 0
  foreach ($root in $roots) {
    if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
    foreach ($shortcut in @(Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.lnk')) {
      if ($names -contains $shortcut.BaseName) { $count += 1 }
    }
  }
  return $count
}

function Resolve-Install-State([object]$App, [object]$Release, [string]$Assets) {
  $entry = Find-App-Entry $App
  if ($null -eq $entry) { Fail 'installed app is missing its uninstall entry' }
  $uninstaller = Parse-Uninstaller $entry.UninstallString
  if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { Fail 'uninstaller is missing' }
  if ([string]::IsNullOrWhiteSpace($entry.InstallLocation)) { Fail 'install location metadata is missing' }
  $installLocation = [Environment]::ExpandEnvironmentVariables($entry.InstallLocation.Trim()).Trim('"')
  $installDir = Full-Path $installLocation
  if (-not (Allowed-Install-Root $installDir)) { Fail 'installer escaped the allowed current-user roots' }
  if ((Full-Path (Split-Path -Parent $uninstaller)) -ne $installDir) {
    Fail 'uninstaller is outside the declared install location'
  }
  if ((Full-Path $uninstaller) -ne (Full-Path (Join-Path $installDir 'uninstall.exe'))) {
    Fail 'uninstaller name or location is not canonical'
  }
  Assert-Plain-Existing-Path $installDir 'install directory'
  Assert-Plain-Existing-Path $uninstaller 'uninstaller path'
  $binary = Join-Path $installDir $App.binaryName
  if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
    $candidates = @(Get-ChildItem -LiteralPath $installDir -Recurse -File -Filter $App.binaryName)
    if ($candidates.Count -ne 1) { Fail 'installed executable is missing or ambiguous' }
    $binary = $candidates[0].FullName
  }
  Assert-Plain-Existing-Path $binary 'installed executable path'
  $displayIcon = Parse-Display-Icon $entry.DisplayIcon
  if ((Full-Path $displayIcon) -ne (Full-Path $binary)) { Fail 'display icon does not target the installed executable' }
  $manifestApp = $Release.byId[$App.id]
  $binarySha = Sha256 $binary
  if ($binarySha -ne $manifestApp.portable.sha256) { Fail 'installed executable digest mismatch' }
  if ($entry.DisplayVersion -ne $manifestApp.version) { Fail 'uninstall display version mismatch' }
  $fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($binary).FileVersion
  if ([string]::IsNullOrWhiteSpace($fileVersion) -or -not $fileVersion.StartsWith($manifestApp.version)) {
    Fail 'installed executable version mismatch'
  }
  $shortcutCount = Get-App-Shortcut-Count $binary
  if ($shortcutCount -lt 1) { Fail 'installed application shortcut is missing' }

  $noticeSha = $null
  if ($null -ne $Release.notices) {
    $notices = @(Get-ChildItem -LiteralPath $installDir -Recurse -File -Filter 'THIRD_PARTY_NOTICES.md')
    if ($notices.Count -ne 1) { Fail 'installed third-party notices are missing or ambiguous' }
    $noticeSha = Sha256 $notices[0].FullName
    if ($noticeSha -ne $Release.notices.sha256) { Fail 'installed third-party notices digest mismatch' }
  }

  return [pscustomobject]@{
    AppId = $App.id
    ProviderPath = $entry.ProviderPath
    RegistryName = $entry.RegistryName
    InstallDir = $installDir
    Binary = $binary
    Uninstaller = $uninstaller
    Version = $manifestApp.version
    BinarySha256 = $binarySha
    FileVersion = $fileVersion
    ShortcutCount = $shortcutCount
    NoticeSha256 = $noticeSha
    PublisherPresent = -not [string]::IsNullOrWhiteSpace($entry.Publisher)
  }
}

function Invoke-Owned-Process([string]$File, [string]$Arguments, [int]$TimeoutSeconds = 180) {
  if (-not (Test-Path -LiteralPath $File -PathType Leaf)) { Fail 'owned executable is missing' }
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $File
  $start.Arguments = $Arguments
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $process = [Diagnostics.Process]::new()
  $process.StartInfo = $start
  if (-not $process.Start()) { Fail 'owned process did not start' }
  if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    try { Stop-Process -Id $process.Id -Force -ErrorAction Stop } catch {}
    Fail 'owned process timed out'
  }
  if ($process.ExitCode -ne 0) { Fail "owned process failed with exit code $($process.ExitCode)" }
  return $process.ExitCode
}

$script:ownedInstalls = @{}
$script:observedInstallDirs = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
$script:observedRegistryKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

function Install-App(
  [object]$App,
  [object]$Release,
  [string]$Assets,
  [string]$Mode,
  [string]$CustomDirectory = ''
) {
  $manifestApp = $Release.byId[$App.id]
  $installer = Join-Path $Assets $manifestApp.installer.name
  $arguments = switch ($Mode) {
    'install' { '/S' }
    'update' { '/S /UPDATE' }
    'custom' {
      if ([string]::IsNullOrWhiteSpace($CustomDirectory)) { Fail 'custom install path is missing' }
      Assert-Descendant $CustomDirectory $ScratchRoot 'custom install path'
      "/S /D=$CustomDirectory"
    }
    default { Fail 'unknown installer mode' }
  }
  $signature = Get-AuthenticodeSignature -LiteralPath $installer
  Invoke-Owned-Process $installer $arguments | Out-Null
  Wait-App-Entry $App $true | Out-Null
  $state = Resolve-Install-State $App $Release $Assets
  if ($Mode -eq 'custom' -and (Full-Path $state.InstallDir) -ne (Full-Path $CustomDirectory)) {
    Fail 'custom install directory was not honored'
  }
  if ($Mode -ne 'custom' -and (Full-Path $state.InstallDir) -ne (Full-Path (Join-Path $env:LOCALAPPDATA $App.productName))) {
    Fail 'default current-user install directory mismatch'
  }
  $script:ownedInstalls[$App.id] = $state
  [void]$script:observedInstallDirs.Add((Full-Path $state.InstallDir))
  [void]$script:observedRegistryKeys.Add($state.ProviderPath)
  return [ordered]@{
    phase = $Mode
    version = $state.Version
    binarySha256 = $state.BinarySha256
    fileVersion = $state.FileVersion
    shortcutCount = $state.ShortcutCount
    noticesSha256 = $state.NoticeSha256
    publisherPresent = $state.PublisherPresent
    installRoot = if ((Full-Path $state.InstallDir).StartsWith((Full-Path $env:RUNNER_TEMP), [StringComparison]::OrdinalIgnoreCase)) { 'runner-temp' } else { 'local-app-data' }
    authenticode = [string]$signature.Status
  }
}

function Uninstall-App([object]$App, [object]$State) {
  if (-not (Allowed-Install-Root $State.InstallDir)) { Fail 'refusing unsafe uninstall path' }
  $arguments = "/S _?=$($State.InstallDir)"
  Invoke-Owned-Process $State.Uninstaller $arguments | Out-Null
  Wait-App-Entry $App $false | Out-Null
  if (Test-Path -LiteralPath $State.ProviderPath) { Fail 'uninstall left its exact registry key' }
  if (Test-Path -LiteralPath $State.Binary -PathType Leaf) { Fail 'uninstall left the application executable' }
  if ((Get-App-Shortcut-Count $State.Binary) -ne 0) { Fail 'uninstall left an application shortcut' }
  for ($attempt = 0; $attempt -lt 40 -and (Test-Path -LiteralPath $State.InstallDir); $attempt += 1) {
    Start-Sleep -Milliseconds 500
  }
  if (Test-Path -LiteralPath $State.InstallDir) { Fail 'uninstall left its install directory' }
  [void]$script:ownedInstalls.Remove($App.id)
  return [ordered]@{ phase = 'uninstall'; registryRemoved = $true; executableRemoved = $true }
}

function New-Markers([object]$App, [string]$RunId) {
  $markers = @()
  $identifiers = @($App.identifier) + @($App.legacyIdentifiers)
  foreach ($identifier in $identifiers) {
    if ($identifier -notmatch '^com\.(devbox|workbench)\.[a-z0-9]+$') { Fail 'unsafe app-data identifier' }
    $directory = Join-Path $env:LOCALAPPDATA $identifier
    $directoryExisted = Test-Path -LiteralPath $directory -PathType Container
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $path = Join-Path $directory ".devbox-installer-acceptance-$RunId.json"
    if (Test-Path -LiteralPath $path) { Fail 'acceptance marker collision' }
    $body = [ordered]@{ schemaVersion = 1; runId = $RunId; app = $App.id; identifier = $identifier }
    [IO.File]::WriteAllText($path, (($body | ConvertTo-Json -Compress) + "`n"), [Text.UTF8Encoding]::new($false))
    $markers += [pscustomobject]@{
      Identifier = $identifier
      Path = $path
      Directory = $directory
      DirectoryExisted = $directoryExisted
      Sha256 = Sha256 $path
    }
  }
  return $markers
}

function Assert-Markers([object[]]$Markers) {
  foreach ($marker in $Markers) {
    if (-not (Test-Path -LiteralPath $marker.Path -PathType Leaf) -or (Sha256 $marker.Path) -ne $marker.Sha256) {
      Fail 'app-data acceptance marker was not preserved'
    }
  }
}

function Remove-Markers([object[]]$Markers) {
  foreach ($marker in $Markers) {
    if (Test-Path -LiteralPath $marker.Path -PathType Leaf) { Remove-Item -LiteralPath $marker.Path -Force }
    if (-not $marker.DirectoryExisted -and (Test-Path -LiteralPath $marker.Directory -PathType Container)) {
      if (@(Get-ChildItem -LiteralPath $marker.Directory -Force).Count -eq 0) {
        Remove-Item -LiteralPath $marker.Directory -Force
      }
    }
  }
}

$report = [ordered]@{
  schemaVersion = 1
  status = 'RUNNING'
  candidateTag = $CandidateTag
  candidateCommit = $CandidateCommit
  startedAt = [DateTime]::UtcNow.ToString('o')
  completedAt = $null
  host = [ordered]@{
    runnerImage = $env:ImageOS
    os = [Environment]::OSVersion.VersionString
    powershell = $PSVersionTable.PSVersion.ToString()
  }
  releases = $null
  apps = @()
  cleanup = [ordered]@{
    uninstallResidue = 0
    registryKeyResidue = 0
    installDirectoryResidue = 0
    markerResidue = 0
    appDataDirectoryResidue = 0
    integrationDirectoryResidue = 0
    failures = @()
  }
  scope = 'installer artifact and lifecycle subset of W4-B'
  limitations = @(
    'the apps are not launched, so real v0.4.1 data-schema migration is not covered',
    'marker preservation proves installer non-deletion but is not a substitute for application data migration',
    'locked-file and ACL-denied fault injection are not covered by this initial lifecycle matrix',
    'real low-disk injection requires a dedicated bounded volume and is not covered by this hosted-runner matrix',
    'per-machine UAC behavior is not covered by the current-user hosted-runner matrix'
  )
  failures = @()
}

$allMarkers = @()
$exitCode = 0
$outputSafe = $false
try {
  if ($env:GITHUB_ACTIONS -ne 'true' -or [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    Fail 'installer acceptance is restricted to a GitHub-hosted disposable runner'
  }
  if ($CandidateTag -notmatch '^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$' -or $CandidateCommit -notmatch '^[0-9a-f]{40}$') {
    Fail 'candidate release identity is invalid'
  }
  foreach ($path in @($Config, $BaselineAssets, $BaselineMetadata, $CandidateAssets, $CandidateMetadata, $ScratchRoot)) {
    if (-not (Test-Path -LiteralPath $path)) { Fail 'required acceptance input is missing' }
  }
  if (Test-Path -LiteralPath $Output) { Fail 'refusing to overwrite acceptance evidence' }
  Assert-Descendant $ScratchRoot $env:RUNNER_TEMP 'scratch root'
  Assert-Descendant $Output $ScratchRoot 'acceptance output'
  $outputSafe = $true

  $configuration = Read-Json $Config
  if ($configuration.schemaVersion -ne 1 -or @($configuration.apps).Count -ne 15) {
    Fail 'installer acceptance config is invalid'
  }
  $baselineApps = @($configuration.apps | Where-Object { [bool]$_.baseline })
  if ($baselineApps.Count -ne 15 -or $baselineApps.Count -ne @($configuration.apps).Count) {
    Fail 'baseline app count mismatch'
  }
  if ($configuration.baseline.tag -notmatch '^v\d+\.\d+\.\d+$' -or $configuration.baseline.commit -notmatch '^[0-9a-f]{40}$') {
    Fail 'baseline release identity is invalid'
  }

  $protectedNames = @($configuration.apps | ForEach-Object {
    @($_.binaryName, "$($_.productName).exe")
  } | Select-Object -Unique)
  $preexistingProcesses = @(Get-CimInstance Win32_Process | Where-Object { $protectedNames -contains $_.Name })
  if ($preexistingProcesses.Count -ne 0) { Fail 'pre-existing Devbox process detected' }
  $sharedDataRoot = Join-Path $env:LOCALAPPDATA 'devbox'
  if (Test-Path -LiteralPath $sharedDataRoot) { Fail 'pre-existing Devbox integration data detected' }
  foreach ($app in $configuration.apps) {
    if ($null -ne (Find-App-Entry $app)) { Fail 'pre-existing Devbox installation detected' }
    if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $app.productName)) {
      Fail 'pre-existing Devbox install directory detected'
    }
    if ((Get-Potential-Shortcut-Count $app) -ne 0) { Fail 'pre-existing Devbox shortcut detected' }
    foreach ($identifier in @($app.identifier) + @($app.legacyIdentifiers)) {
      if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $identifier)) {
        Fail 'pre-existing Devbox app-data directory detected'
      }
    }
  }

  $baselineRelease = Verify-Release $BaselineAssets $BaselineMetadata $configuration.baseline.tag $configuration.baseline.commit $baselineApps.Count $false $baselineApps
  $candidateIsPrerelease = $CandidateTag.Contains('-')
  $candidateRelease = Verify-Release $CandidateAssets $CandidateMetadata $CandidateTag $CandidateCommit 15 $candidateIsPrerelease @($configuration.apps)
  $report.releases = [ordered]@{
    baseline = [ordered]@{ tag = $baselineRelease.tag; commit = $baselineRelease.commit; assets = $baselineRelease.assets; manifestSha256 = $baselineRelease.manifestSha256; metadataSha256 = $baselineRelease.metadataSha256 }
    candidate = [ordered]@{ tag = $candidateRelease.tag; commit = $candidateRelease.commit; assets = $candidateRelease.assets; manifestSha256 = $candidateRelease.manifestSha256; metadataSha256 = $candidateRelease.metadataSha256 }
  }

  $runId = "$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
  foreach ($app in $configuration.apps) {
    $appResult = [ordered]@{ id = $app.id; baseline = [bool]$app.baseline; status = 'RUNNING'; phases = @(); markers = @(); failure = $null }
    try {
      if ([bool]$app.baseline) {
        $appResult.phases += Install-App $app $baselineRelease $BaselineAssets 'install'
        $markers = @(New-Markers $app $runId)
        $allMarkers += $markers
        $appResult.markers = @($markers | ForEach-Object { [ordered]@{ identifier = $_.Identifier; sha256 = $_.Sha256 } })

        $appResult.phases += Install-App $app $candidateRelease $CandidateAssets 'update'
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers

        $appResult.phases += Install-App $app $candidateRelease $CandidateAssets 'install'
        Assert-Markers $markers
        $appResult.phases += Install-App $app $baselineRelease $BaselineAssets 'update'
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers
      } else {
        $customDir = Join-Path $ScratchRoot "custom-install\$($app.id)"
        $appResult.phases += Install-App $app $candidateRelease $CandidateAssets 'custom' $customDir
        $markers = @(New-Markers $app $runId)
        $allMarkers += $markers
        $appResult.markers = @($markers | ForEach-Object { [ordered]@{ identifier = $_.Identifier; sha256 = $_.Sha256 } })
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers

        $appResult.phases += Install-App $app $candidateRelease $CandidateAssets 'install'
        Assert-Markers $markers
        $state = $script:ownedInstalls[$app.id]
        $appResult.phases += Uninstall-App $app $state
        Assert-Markers $markers
      }
      Remove-Markers $markers
      $allMarkers = @($allMarkers | Where-Object { $_.Path -notin $markers.Path })
      $appResult.status = 'PASS'
    } catch {
      $appResult.status = 'FAIL'
      $appResult.failure = Public-Error $_
      throw
    } finally {
      $report.apps += $appResult
      if ($outputSafe) { Write-Report $report $Output }
    }
  }
  $report.status = 'PASS'
} catch {
  $report.status = 'FAIL'
  $report.failures += Public-Error $_
  $exitCode = 1
} finally {
  foreach ($appId in @($script:ownedInstalls.Keys)) {
    try {
      $app = @($configuration.apps | Where-Object { $_.id -eq $appId })[0]
      $state = $script:ownedInstalls[$appId]
      Uninstall-App $app $state | Out-Null
    } catch {
      $report.cleanup.failures += "uninstall cleanup failed: $(Public-Error $_)"
    }
  }
  foreach ($marker in @($allMarkers)) {
    try { Remove-Markers @($marker) } catch { $report.cleanup.failures += "marker cleanup failed: $(Public-Error $_)" }
  }
  try {
    if (Get-Variable configuration -ErrorAction SilentlyContinue) {
      foreach ($app in $configuration.apps) {
        if ($null -ne (Find-App-Entry $app)) { $report.cleanup.uninstallResidue += 1 }
      }
    }
    if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'devbox')) {
      $report.cleanup.integrationDirectoryResidue = 1
    }
    foreach ($marker in @($allMarkers)) {
      if (Test-Path -LiteralPath $marker.Path) { $report.cleanup.markerResidue += 1 }
    }
    foreach ($directory in $script:observedInstallDirs) {
      if (Test-Path -LiteralPath $directory) { $report.cleanup.installDirectoryResidue += 1 }
    }
    foreach ($registryKey in $script:observedRegistryKeys) {
      if (Test-Path -LiteralPath $registryKey) { $report.cleanup.registryKeyResidue += 1 }
    }
    if (Get-Variable configuration -ErrorAction SilentlyContinue) {
      foreach ($identifier in @($configuration.apps | ForEach-Object { @($_.identifier) + @($_.legacyIdentifiers) }) | Select-Object -Unique) {
        if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA $identifier)) {
          $report.cleanup.appDataDirectoryResidue += 1
        }
      }
    }
  } catch {
    $report.cleanup.failures += "cleanup read-back failed: $(Public-Error $_)"
  }
  if (
    $report.cleanup.uninstallResidue -ne 0 -or
    $report.cleanup.registryKeyResidue -ne 0 -or
    $report.cleanup.installDirectoryResidue -ne 0 -or
    $report.cleanup.markerResidue -ne 0 -or
    $report.cleanup.appDataDirectoryResidue -ne 0 -or
    $report.cleanup.integrationDirectoryResidue -ne 0 -or
    $report.cleanup.failures.Count -ne 0
  ) {
    $report.status = 'FAIL'
    $exitCode = 1
  }
  $report.completedAt = [DateTime]::UtcNow.ToString('o')
  if ($outputSafe) { Write-Report $report $Output }
}

exit $exitCode
