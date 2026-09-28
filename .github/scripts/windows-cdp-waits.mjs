import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { ownedCdpSelection } from "./windows-cdp-host.mjs";
const execute = promisify(execFile);
const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
function projectChain(value) {
  if (
    !value ||
    !["observed", "partial", "denied", "unavailable", "thread_changed", "failed"].includes(value.state) ||
    typeof value.cycle !== "boolean" ||
    !Array.isArray(value.nodes) ||
    value.nodes.length > 16
  )
    throw new Error("invalid chain");
  const nodes = value.nodes.map((node) => {
    if (
      !integer(node?.type, 0, 64) ||
      !integer(node.status, 0, 64) ||
      !["app", "browser", "other", "not_thread"].includes(node.processRole) ||
      !["app_ui", "sample", "other", "not_thread"].includes(node.threadRole)
    )
      throw new Error("invalid node");
    const { type, status, processRole, threadRole } = node;
    return { type, status, processRole, threadRole };
  });
  return { state: value.state, cycle: value.cycle, nodes };
}
// Built-in WCT API; unsupported waits can produce a single node, not proof of no deadlock.
// https://learn.microsoft.com/en-us/windows/win32/api/wct/nf-wct-getthreadwaitchain
export async function observeWindowsCdpWaits(identity, port, { platform = process.platform, run = execute } = {}) {
  if (platform !== "win32") return { state: "unsupported" };
  let selection;
  try {
    selection = ownedCdpSelection(identity, port);
  } catch {
    return { state: "invalid_owner" };
  }
  const script = `${selection}
if ($listener -ne 'owned') { @{state='listener_unavailable'} | ConvertTo-Json -Compress; exit 0 }
$listenerIds = @($ports | Select-Object -ExpandProperty OwningProcess -Unique)
if ($listenerIds.Count -ne 1) { @{state='listener_ambiguous'} | ConvertTo-Json -Compress; exit 0 }
$listenerPid = [int]$listenerIds[0]
$listenerRow = if ($listenerPid -eq $ownedPid) { $owner } else { @($rows | Where-Object { $_.ProcessId -eq $listenerPid })[0] }
if (-not $listenerRow) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
$appProcess = [System.Diagnostics.Process]::GetProcessById($ownedPid)
$browserProcess = [System.Diagnostics.Process]::GetProcessById($listenerPid)
$appLease = $appProcess.Handle
$browserLease = $browserProcess.Handle
$check = Get-CimInstance Win32_Process -Filter "ProcessId=$listenerPid"
if (-not $check -or $check.CreationDate -ne $listenerRow.CreationDate) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
[StructLayout(LayoutKind.Explicit, Size=280)] public struct DevboxWaitNode {
  [FieldOffset(0)] public int Type;
  [FieldOffset(4)] public int Status;
  [FieldOffset(8)] public uint Pid;
  [FieldOffset(12)] public uint Tid;
}
public class DevboxWaitView { public int type; public int status; public string processRole; public string threadRole; }
public class DevboxWaitChain { public string state; public bool cycle; public DevboxWaitView[] nodes = new DevboxWaitView[0]; }
public static class DevboxWaits {
  [DllImport("advapi32.dll", SetLastError=true)] static extern IntPtr OpenThreadWaitChainSession(uint flags, IntPtr callback);
  [DllImport("advapi32.dll")] static extern void CloseThreadWaitChainSession(IntPtr session);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetThreadWaitChain(IntPtr session, UIntPtr context, uint flags, uint thread, ref uint count, [Out] DevboxWaitNode[] nodes, out bool cycle);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenThread(uint access, bool inherit, uint thread);
  [DllImport("kernel32.dll")] static extern uint GetProcessIdOfThread(IntPtr thread);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  public static DevboxWaitChain Read(uint thread, uint expected, uint app, uint browser, uint ui) {
    var result = new DevboxWaitChain { state="unavailable" };
    IntPtr threadLease = OpenThread(0x800, false, thread);
    if (threadLease == IntPtr.Zero) { result.state="denied"; return result; }
    try {
      if (GetProcessIdOfThread(threadLease) != expected) { result.state="thread_changed"; return result; }
      IntPtr session = OpenThreadWaitChainSession(0, IntPtr.Zero);
      if (session == IntPtr.Zero) return result;
      try {
        uint count = 16; bool cycle;
        var nodes = new DevboxWaitNode[16];
        bool ok = GetThreadWaitChain(session, UIntPtr.Zero, 5, thread, ref count, nodes, out cycle);
        int error = Marshal.GetLastWin32Error();
        if (!ok && error != 234) { result.state = error == 5 ? "denied" : "failed"; return result; }
        result.state = ok ? "observed" : "partial";
        result.cycle = cycle;
        var output = new List<DevboxWaitView>();
        for (int i=0; i<Math.Min(count,16u); i++) {
          var node=nodes[i]; bool isThread=node.Type==8;
          output.Add(new DevboxWaitView {
            type=node.Type, status=node.Status,
            processRole=!isThread ? "not_thread" : node.Pid==app ? "app" : node.Pid==browser ? "browser" : "other",
            threadRole=!isThread ? "not_thread" : node.Tid==ui ? "app_ui" : node.Tid==thread ? "sample" : "other"
          });
        }
        result.nodes=output.ToArray();
        return result;
      } finally { CloseThreadWaitChainSession(session); }
    } finally { CloseHandle(threadLease); }
  }
}
'@
$debugAccess = $false
try { [System.Diagnostics.Process]::EnterDebugMode(); $debugAccess = $true } catch {}
[uint32]$windowPid = 0
[uint32]$uiThread = [DevboxWaits]::GetWindowThreadProcessId($appProcess.MainWindowHandle, [ref]$windowPid)
if ($windowPid -ne $ownedPid) { $uiThread = 0 }
$threads = @($browserProcess.Threads | Sort-Object Id | Select-Object -First 64)
$before = @{}
foreach ($thread in $threads) { $before[$thread.Id] = $thread.TotalProcessorTime.TotalMilliseconds }
$appBefore = $appProcess.TotalProcessorTime.TotalMilliseconds
$browserBefore = $browserProcess.TotalProcessorTime.TotalMilliseconds
$watch = [System.Diagnostics.Stopwatch]::StartNew()
Start-Sleep -Milliseconds 250
$appProcess.Refresh()
$browserProcess.Refresh()
$afterThreads = @{}
foreach ($current in $browserProcess.Threads) { $afterThreads[$current.Id] = $current }
$appCpu = [Math]::Max(0, [Math]::Round($appProcess.TotalProcessorTime.TotalMilliseconds-$appBefore))
$browserCpu = [Math]::Max(0, [Math]::Round($browserProcess.TotalProcessorTime.TotalMilliseconds-$browserBefore))
$sampleMs = $watch.ElapsedMilliseconds
$output = @()
$index = 0
foreach ($thread in $threads) {
  $state = -1; $reason = -1; $cpu = 0
  try {
    $current = $afterThreads[$thread.Id]
    if (-not $current) { throw "thread ended" }
    $state = [int]$current.ThreadState
    if ($current.ThreadState -eq [System.Diagnostics.ThreadState]::Wait) { $reason = [int]$current.WaitReason }
    $cpu = [Math]::Max(0, [Math]::Round($current.TotalProcessorTime.TotalMilliseconds-$before[$thread.Id]))
  } catch {}
  $output += @{index=$index;state=$state;waitReason=$reason;cpuMs=$cpu;chain=[DevboxWaits]::Read([uint32]$thread.Id,[uint32]$listenerPid,[uint32]$ownedPid,[uint32]$listenerPid,$uiThread)}
  $index++
}
$appUi = if ($uiThread -ne 0) { [DevboxWaits]::Read($uiThread,[uint32]$ownedPid,[uint32]$ownedPid,[uint32]$listenerPid,$uiThread) } else { $null }
$again = Get-CimInstance Win32_Process -Filter "ProcessId=$ownedPid"
$againBrowser = Get-CimInstance Win32_Process -Filter "ProcessId=$listenerPid"
if (-not $again -or -not $againBrowser -or $again.CreationDate.ToUniversalTime().ToString('o') -ne $expected -or $againBrowser.CreationDate -ne $listenerRow.CreationDate) { @{state='identity_changed'} | ConvertTo-Json -Compress; exit 0 }
@{state='observed';sampleMs=$sampleMs;appCpuMs=$appCpu;listenerCpuMs=$browserCpu;listenerWorkingSetMiB=[Math]::Floor($browserProcess.WorkingSet64/1048576);listenerThreadCount=$browserProcess.Threads.Count;debugAccess=$debugAccess;appUi=$appUi;threads=$output;truncated=($browserProcess.Threads.Count -gt 64)} | ConvertTo-Json -Depth 8 -Compress
`;
  try {
    const { stdout } = await run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
      timeout: 15000,
      maxBuffer: 131072,
      windowsHide: true,
      encoding: "utf8",
    });
    if (typeof stdout !== "string" || Buffer.byteLength(stdout) > 131072) return { state: "invalid_response" };
    const value = JSON.parse(stdout.replace(/^\uFEFF/, "").trim());
    if (["identity_changed", "product_exited", "listener_unavailable", "listener_ambiguous"].includes(value?.state))
      return { state: value.state };
    if (
      value?.state !== "observed" ||
      !integer(value.sampleMs, 0, 15000) ||
      !integer(value.appCpuMs, 0, 1000000000) ||
      !integer(value.listenerCpuMs, 0, 1000000000) ||
      !integer(value.listenerWorkingSetMiB, 0, 16777216) ||
      !integer(value.listenerThreadCount, 0, 10000) ||
      typeof value.debugAccess !== "boolean" ||
      typeof value.truncated !== "boolean" ||
      !Array.isArray(value.threads) ||
      value.threads.length > 64
    )
      return { state: "invalid_response" };
    const threads = value.threads.map((thread) => {
      if (
        !integer(thread?.index, 0, 63) ||
        !integer(thread.state, -1, 64) ||
        !integer(thread.waitReason, -1, 64) ||
        !integer(thread.cpuMs, 0, 1000000000)
      )
        throw new Error("invalid thread");
      const { index, state, waitReason, cpuMs } = thread;
      return { index, state, waitReason, cpuMs, chain: projectChain(thread.chain) };
    });
    const appUi = value.appUi === null ? null : projectChain(value.appUi);
    const { sampleMs, appCpuMs, listenerCpuMs, listenerWorkingSetMiB, listenerThreadCount, debugAccess, truncated } =
      value;
    return {
      state: "observed",
      sampleMs,
      appCpuMs,
      listenerCpuMs,
      listenerWorkingSetMiB,
      listenerThreadCount,
      debugAccess,
      appUi,
      threads,
      truncated,
    };
  } catch (error) {
    return { state: error?.message?.startsWith("invalid ") ? "invalid_response" : "probe_failed" };
  }
}
