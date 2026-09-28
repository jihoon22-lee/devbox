$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
  throw 'Stack probe compilation is restricted to disposable hosted Windows acceptance'
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $visualStudio) { throw 'Existing MSVC toolchain unavailable' }
$vcvars = Join-Path $visualStudio 'VC/Auxiliary/Build/vcvars64.bat'
$source = Join-Path $PSScriptRoot 'windows-cdp-stacks.cpp'
$build = Join-Path $env:RUNNER_TEMP ('cdp-stack-probe-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $build | Out-Null
$exe = Join-Path $build 'windows-cdp-stacks.exe'
Push-Location $build
try {
  & cmd.exe /d /s /c "`"$vcvars`" && cl.exe /nologo /W4 /WX /EHsc /std:c++17 /Zi `"$source`" /Fe:`"$exe`" /link /DEBUG:FULL"
  if ($LASTEXITCODE -ne 0) { throw 'Stack probe compilation failed' }
  & $exe --self-test
  if ($LASTEXITCODE -ne 0) { throw 'Owned snapshot stack self-test failed' }
} finally {
  Pop-Location
}
"DEVBOX_CDP_STACK_HELPER=$exe" >> $env:GITHUB_ENV
