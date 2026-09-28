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
# Fetch only public Windows symbol data. No executable or debugger is downloaded.
$symbols = Join-Path $build 'symbols'
New-Item -ItemType Directory -Path $symbols | Out-Null
foreach ($module in @('ntdll', 'user32', 'win32u', 'imm32', 'msctf')) {
  $index = & $exe --symbol-index $module | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0 -or $index.name -notmatch '^[A-Za-z0-9_.-]{1,160}\.pdb$' -or $index.key -notmatch '^[A-F0-9]{33,40}$') {
    throw 'Invalid public system symbol identity'
  }
  $client = [System.Net.Http.HttpClient]::new()
  $cancel = [System.Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(20))
  $response = $null; $inputStream = $null; $outputStream = $null
  $partial = Join-Path $symbols ($index.name + '.partial')
  try {
    $url = "https://msdl.microsoft.com/download/symbols/$($index.name)/$($index.key)/$($index.name)"
    $response = $client.GetAsync($url, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancel.Token).GetAwaiter().GetResult()
    if (-not $response.IsSuccessStatusCode -or $response.Content.Headers.ContentLength -gt 33554432) { throw 'Public symbols unavailable' }
    $inputStream = $response.Content.ReadAsStreamAsync($cancel.Token).GetAwaiter().GetResult()
    $outputStream = [System.IO.File]::Open($partial, [System.IO.FileMode]::CreateNew)
    $buffer = [byte[]]::new(65536)
    $total = 0
    while (($count = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancel.Token).GetAwaiter().GetResult()) -gt 0) {
      $total += $count
      if ($total -gt 33554432) { throw 'Public symbol size limit' }
      $outputStream.Write($buffer, 0, $count)
    }
    $outputStream.Dispose(); $outputStream = $null
    Move-Item -LiteralPath $partial -Destination (Join-Path $symbols $index.name)
    Write-Host "Public Windows symbols prepared: $module"
  } catch {
    Write-Host "Public Windows symbols unavailable: $module; retain explicit export-only evidence"
  } finally {
    if ($outputStream) { $outputStream.Dispose() }
    if ($inputStream) { $inputStream.Dispose() }
    if ($response) { $response.Dispose() }
    $cancel.Dispose(); $client.Dispose()
    if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial }
  }
}
"DEVBOX_CDP_STACK_SYMBOLS=$symbols" >> $env:GITHUB_ENV
