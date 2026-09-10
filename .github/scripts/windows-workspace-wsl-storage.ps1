$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Owned GitHub fixture required' }
$state = Get-Content -LiteralPath (Join-Path $env:RUNNER_TEMP 'devbox-knowledge-wsl-owner.json') -Raw | ConvertFrom-Json
if ($state.runId -ne $env:GITHUB_RUN_ID -or $state.name -notmatch '^DevboxKnowledgeFixture-[0-9]+-[a-f0-9]{12}$') { throw 'Unexpected distro fixture' }
if ((Get-Content -LiteralPath (Join-Path $state.install 'devbox-fixture-owner.txt') -Raw) -ne $state.name) { throw 'Fixture owner changed' }
$keys = @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss' | Where-Object { $_.GetValue('DistributionName') -eq $state.name })
if ($keys.Count -ne 1) { throw 'Owned registration missing or ambiguous' }
$raw = [string]$keys[0].GetValue('BasePath')
$filesystemVersion = [uint32]$keys[0].GetValue('Version')
$flags = [uint32]$keys[0].GetValue('Flags')
$wslVersion = if (($flags -band 8) -ne 0) { 2 } else { 1 }
if ($wslVersion -ne 1) { throw 'The exclusively owned fixture must use WSL1' }
$base = if ($raw.StartsWith('\??\')) { $raw.Substring(4) } else { $raw }
$plain = if ($base.StartsWith('\\?\')) { $base.Substring(4) } else { $base }
$plain = [IO.Path]::GetFullPath($plain)
if (-not $plain.StartsWith([IO.Path]::GetFullPath($state.install).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Registration escaped the owned fixture' }
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class OwnedWslStorage {
  [StructLayout(LayoutKind.Sequential)] public struct Info {
    public uint Attributes; public uint CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
    public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
  }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern uint GetFileAttributesW(string path);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle file,out Info info);
  public static Dictionary<string,object> Inspect(string path) {
    var row=new Dictionary<string,object>(); row["path"]=path;
    uint attributes=GetFileAttributesW(path);row["attributes"]=attributes;
    if(attributes==0xffffffff){row["error"]=Marshal.GetLastWin32Error();return row;}
    row["reparse"]=(attributes&0x400)!=0;
    using(var file=CreateFileW(path,0x80,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
      if(file.IsInvalid){row["error"]=Marshal.GetLastWin32Error();return row;}
      Info info;
      if(!GetFileInformationByHandle(file,out info)){row["error"]=Marshal.GetLastWin32Error();return row;}
      row["handleAttributes"]=info.Attributes;row["volume"]=info.Volume;row["fileId"]=((ulong)info.IndexHigh<<32)|info.IndexLow;
    }
    return row;
  }
}
'@
$paths = [Collections.Generic.List[string]]::new()
$current = $base.TrimEnd('\')
while ($current) {
  $paths.Add($current)
  $parent = [IO.Path]::GetDirectoryName($current)
  if ($parent -eq $current) { break }
  $current = $parent
}
$paths.Reverse()
$rows = @()
foreach ($path in $paths) {
  $row = [OwnedWslStorage]::Inspect($path)
  $rows += $row
  if ($row.ContainsKey('error') -or $row['reparse']) { break }
}
@{ version = 1; filesystemVersion = $filesystemVersion; flags = $flags; wslVersion = $wslVersion; boundary = 'Only the nonce-owned distro backing path; metadata only'; basePath = $raw; ancestors = $rows } |
  ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $env:GITHUB_WORKSPACE 'product-foundation-evidence/workspace-wsl-storage.json')
