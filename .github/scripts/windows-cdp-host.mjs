import { execFile } from "node:child_process";
import { promisify } from "node:util";
const execute = promisify(execFile);
const bounded = (value, max) => Number.isSafeInteger(value) && value >= 0 && value <= max;
// Opt-in hosted comparison only; never replays a product command.
export async function focusWindowsCdpHost(identity, { platform = process.platform, run = execute } = {}) {
  if (platform !== "win32") return { state: "unsupported" };
  if (
    !bounded(identity?.Pid, 2147483647) ||
    identity.Pid === 0 ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{1,7}Z$/.test(identity.Created ?? "")
  )
    return { state: "invalid_owner" };
  const script = `
$ErrorActionPreference = 'Stop'
$ownedPid = ${identity.Pid}
$expected = '${identity.Created}'
$owner = Get-CimInstance Win32_Process -Filter "ProcessId=$ownedPid"
if (-not $owner) { @{state='product_exited'} | ConvertTo-Json -Compress; exit 0 }
if ($owner.CreationDate.ToUniversalTime().ToString('o') -ne $expected) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$ownedProcess = [System.Diagnostics.Process]::GetProcessById($ownedPid)
$lease = $ownedProcess.Handle
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DevboxCdpForeground {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
  [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr window, int command);
}
'@
$window = $ownedProcess.MainWindowHandle
if ($window -eq [IntPtr]::Zero) { @{state='no_window'} | ConvertTo-Json -Compress; exit 0 }
[uint32]$windowPid = 0
[void][DevboxCdpForeground]::GetWindowThreadProcessId($window, [ref]$windowPid)
$again = Get-CimInstance Win32_Process -Filter "ProcessId=$ownedPid"
if (-not $again -or $windowPid -ne $ownedPid -or $again.CreationDate.ToUniversalTime().ToString('o') -ne $expected) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$wasForeground = [DevboxCdpForeground]::GetForegroundWindow() -eq $window
$wasMinimized = [DevboxCdpForeground]::IsIconic($window)
if ($wasMinimized) { [void][DevboxCdpForeground]::ShowWindowAsync($window, 9) }
[void][DevboxCdpForeground]::SetForegroundWindow($window)
Start-Sleep -Milliseconds 250
[void][DevboxCdpForeground]::GetWindowThreadProcessId($window, [ref]$windowPid)
if ($ownedProcess.HasExited -or $windowPid -ne $ownedPid) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$state = if ([DevboxCdpForeground]::GetForegroundWindow() -eq $window) { 'focused' } else { 'not_focused' }
@{state=$state;wasForeground=[bool]$wasForeground;wasMinimized=[bool]$wasMinimized} | ConvertTo-Json -Compress
`;
  try {
    const { stdout } = await run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
      timeout: 10000,
      maxBuffer: 16384,
      windowsHide: true,
      encoding: "utf8",
    });
    if (typeof stdout !== "string" || Buffer.byteLength(stdout) > 16384) return { state: "invalid_response" };
    const value = JSON.parse(stdout.replace(/^\uFEFF/, "").trim());
    if (["product_exited", "identity_changed", "no_window"].includes(value?.state)) return { state: value.state };
    if (
      !["focused", "not_focused"].includes(value?.state) ||
      typeof value.wasForeground !== "boolean" ||
      typeof value.wasMinimized !== "boolean"
    )
      return { state: "invalid_response" };
    return { state: value.state, wasForeground: value.wasForeground, wasMinimized: value.wasMinimized };
  } catch {
    return { state: "probe_failed" };
  }
}

// Read only the retained product identity and its WebView descendants after failure.
export async function observeWindowsCdpHost(identity, port, { platform = process.platform, run = execute } = {}) {
  if (platform !== "win32") return { state: "unsupported" };
  if (
    !bounded(identity?.Pid, 2147483647) ||
    identity.Pid === 0 ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{1,7}Z$/.test(identity.Created ?? "") ||
    !bounded(port, 65535) ||
    port === 0
  )
    return { state: "invalid_owner" };
  // Every interpolated value is an integer or a closed UTC timestamp, never a path.
  const script = `
$ErrorActionPreference = 'Stop'
$ownedPid = ${identity.Pid}
$expected = '${identity.Created}'
$owner = Get-CimInstance Win32_Process -Filter "ProcessId=$ownedPid"
if (-not $owner) { @{state='product_exited'} | ConvertTo-Json -Compress; exit 0 }
if ($owner.CreationDate.ToUniversalTime().ToString('o') -ne $expected) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$ids = New-Object 'System.Collections.Generic.HashSet[int]'
[void]$ids.Add($ownedPid)
$rows = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'")
for ($scan = 0; $scan -lt 128; $scan++) {
  $grew = $false
  foreach ($row in $rows) {
    if ($row.CreationDate -ge $owner.CreationDate -and $ids.Contains([int]$row.ParentProcessId)) {
      if ($ids.Add([int]$row.ProcessId)) { $grew = $true }
    }
  }
  if ($ids.Count -gt 129) { throw 'probe_limit' }
  if (-not $grew) { break }
}
$listener = 'query_failed'
try {
  $ports = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { $_.LocalPort -eq ${port} })
  if ($ports.Count -eq 0) { $listener = 'absent' }
  elseif (@($ports | Where-Object { -not $ids.Contains([int]$_.OwningProcess) }).Count -eq 0) { $listener = 'owned' }
  else { $listener = 'other' }
} catch { $listener = 'query_failed' }
$ownedProcess = [System.Diagnostics.Process]::GetProcessById($ownedPid)
$windowPresent = $ownedProcess.MainWindowHandle -ne [IntPtr]::Zero
$responding = if ($windowPresent) { [bool]$ownedProcess.Responding } else { $null }
$memory = Get-CimInstance Win32_OperatingSystem
$again = Get-CimInstance Win32_Process -Filter "ProcessId=$ownedPid"
if (-not $again) { @{state='product_exited'} | ConvertTo-Json -Compress; exit 0 }
if ($again.CreationDate.ToUniversalTime().ToString('o') -ne $expected) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
@{
  state='observed'; ownerWindowPresent=[bool]$windowPresent; ownerResponding=$responding
  webviewProcesses=($ids.Count - 1); listener=$listener
  freeMemoryMiB=[Math]::Floor($memory.FreePhysicalMemory/1024)
  totalMemoryMiB=[Math]::Floor($memory.TotalVisibleMemorySize/1024)
} | ConvertTo-Json -Compress
`;
  try {
    const { stdout } = await run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
      timeout: 10000,
      maxBuffer: 16384,
      windowsHide: true,
      encoding: "utf8",
    });
    if (typeof stdout !== "string" || Buffer.byteLength(stdout) > 16384) return { state: "invalid_response" };
    const value = JSON.parse(stdout.replace(/^\uFEFF/, "").trim());
    if (["product_exited", "identity_changed"].includes(value?.state)) return { state: value.state };
    if (
      value?.state !== "observed" ||
      typeof value.ownerWindowPresent !== "boolean" ||
      (value.ownerWindowPresent ? typeof value.ownerResponding !== "boolean" : value.ownerResponding !== null) ||
      !bounded(value.webviewProcesses, 128) ||
      !["owned", "other", "absent", "query_failed"].includes(value.listener) ||
      !bounded(value.freeMemoryMiB, 16777216) ||
      !bounded(value.totalMemoryMiB, 16777216) ||
      value.totalMemoryMiB === 0 ||
      value.freeMemoryMiB > value.totalMemoryMiB
    )
      return { state: "invalid_response" };
    const { ownerWindowPresent, ownerResponding, webviewProcesses, listener, freeMemoryMiB, totalMemoryMiB } = value;
    return {
      state: "observed",
      ownerWindowPresent,
      ownerResponding,
      webviewProcesses,
      listener,
      freeMemoryMiB,
      totalMemoryMiB,
    };
  } catch {
    return { state: "probe_failed" };
  }
}
