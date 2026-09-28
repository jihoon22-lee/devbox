import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { ownedCdpSelection } from "./windows-cdp-host.mjs";
const execute = promisify(execFile);
const modules = new Set([
  "ntdll",
  "kernel32",
  "kernelbase",
  "user32",
  "win32u",
  "combase",
  "ole32",
  "oleaut32",
  "rpcrt4",
  "uiautomationcore",
  "dwmapi",
  "imm32",
  "msctf",
  "comctl32",
  "textinputframework",
  "uxtheme",
  "coremessaging",
  "wintypes",
  "inputhost",
  "msedge",
  "msedgewebview2",
  "embeddedbrowserwebview",
  "webview2loader",
  "devbox-api-studio",
  "windows-cdp-stacks",
  "other",
]);
function project(value) {
  if (
    ![
      "observed",
      "denied",
      "identity_changed",
      "capture_failed",
      "thread_unavailable",
      "symbols_unavailable",
      "walk_failed",
    ].includes(value?.state) ||
    !Array.isArray(value.frames) ||
    value.frames.length > 32
  )
    throw new Error("invalid stack");
  const frames = value.frames.map(({ module, symbol, offset, symbolKind, displacement }) => {
    if (
      !modules.has(module) ||
      !["none", "export", "pdb"].includes(symbolKind) ||
      (symbol === null
        ? symbolKind !== "none" || displacement !== null
        : symbolKind === "none" ||
          !Number.isSafeInteger(displacement) ||
          displacement < 0 ||
          displacement > 0xffffffff) ||
      !(symbol === null || (typeof symbol === "string" && /^[A-Za-z0-9_?$@:<>, ()&*~!+.\-]{1,160}$/.test(symbol))) ||
      !Number.isSafeInteger(offset) ||
      offset < 0 ||
      offset > 0xffffffff
    )
      throw new Error("invalid frame");
    return { module, symbol, offset, symbolKind, displacement };
  });
  return { state: value.state, frames };
}
// Opt-in hosted diagnostic: code locations only, no dump or target mutation.
export async function observeWindowsCdpStacks(
  identity,
  port,
  { platform = process.platform, helper = process.env.DEVBOX_CDP_STACK_HELPER, run = execute } = {},
) {
  if (platform !== "win32") return { state: "unsupported" };
  if (!helper) return { state: "unavailable" };
  let selection;
  try {
    selection = ownedCdpSelection(identity, port);
  } catch {
    return { state: "invalid_owner" };
  }
  const script = `${selection}
if ($listener -ne 'owned') { @{state='listener_unavailable'} | ConvertTo-Json -Compress; exit 0 }
$listenerIds = @($ports | Select-Object -ExpandProperty OwningProcess -Unique)
if ($listenerIds.Count -ne 1) { @{state='listener_unavailable'} | ConvertTo-Json -Compress; exit 0 }
$listenerPid = [int]$listenerIds[0]
$browser = if ($listenerPid -eq $ownedPid) { $owner } else { @($rows | Where-Object { $_.ProcessId -eq $listenerPid })[0] }
if (-not $browser) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$appProcess = [System.Diagnostics.Process]::GetProcessById($ownedPid)
$appLease = $appProcess.Handle
$browserProcess = [System.Diagnostics.Process]::GetProcessById($listenerPid)
$browserLease = $browserProcess.Handle
$precision = 'yyyy-MM-ddTHH:mm:ss.ffffffZ'
if ($appProcess.StartTime.ToUniversalTime().ToString($precision) -ne $owner.CreationDate.ToUniversalTime().ToString($precision) -or $browserProcess.StartTime.ToUniversalTime().ToString($precision) -ne $browser.CreationDate.ToUniversalTime().ToString($precision)) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DevboxStackWindow {
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
}
'@
[uint32]$windowPid = 0
$uiThread = [DevboxStackWindow]::GetWindowThreadProcessId($appProcess.MainWindowHandle, [ref]$windowPid)
if ($uiThread -eq 0 -or $windowPid -ne $ownedPid) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$tool = '${helper.replaceAll("'", "''")}'
$app = & $tool $ownedPid $appProcess.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() $uiThread | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'stack probe failed' }
$child = & $tool $listenerPid $browserProcess.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() 0 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'stack probe failed' }
@{state='observed';app=$app;browser=$child} | ConvertTo-Json -Depth 6 -Compress
`;
  try {
    const { stdout } = await run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
      timeout: 30000,
      maxBuffer: 65536,
      windowsHide: true,
      encoding: "utf8",
    });
    if (typeof stdout !== "string" || Buffer.byteLength(stdout) > 65536) return { state: "invalid_response" };
    let value;
    try {
      value = JSON.parse(stdout.replace(/^\uFEFF/, "").trim());
      if (["listener_unavailable", "identity_changed", "product_exited"].includes(value?.state))
        return { state: value.state };
      if (value?.state !== "observed") return { state: "invalid_response" };
      return { state: "observed", app: project(value.app), browser: project(value.browser) };
    } catch {
      return { state: "invalid_response" };
    }
  } catch {
    return { state: "probe_failed" };
  }
}
