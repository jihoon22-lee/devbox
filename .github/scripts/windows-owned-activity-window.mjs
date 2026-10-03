// A disposable hosted fixture owns every HWND/input target; never samples a user's window.
import assert from "node:assert/strict";
import { spawn, execFile } from "node:child_process";
import { readFile, mkdir } from "node:fs/promises";
import { promisify } from "node:util";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
const execute = promisify(execFile),
  quote = (value) => `'${String(value).replaceAll("'", "''")}'`;
export async function createOwnedActivityWindow(directory) {
  assert.equal(process.platform, "win32");
  assert.equal(process.env.GITHUB_ACTIONS, "true");
  assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
  await mkdir(directory, { recursive: true });
  const marker = `DevboxOwnedActivity${randomUUID().replaceAll("-", "")}`,
    receipt = path.join(directory, "window.json");
  const script = `$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Windows.Forms;$f=New-Object Windows.Forms.Form;$f.Text=${quote(marker)};$f.Width=480;$f.Height=240;$f.KeyPreview=$true;$f.Add_KeyDown({if($_.KeyCode -eq 'F6'){$f.Text=${quote(marker + "-input")}}});$f.Add_Shown({$p=Get-CimInstance Win32_Process -Filter "ProcessId=$PID";@{Pid=$PID;Created=$p.CreationDate.ToUniversalTime().ToString('o');Window=$f.Handle.ToInt64()}|ConvertTo-Json -Compress|Set-Content -LiteralPath ${quote(receipt)} -Encoding utf8});[Windows.Forms.Application]::Run($f)`;
  const child = spawn("powershell.exe", ["-NoProfile", "-Sta", "-Command", script], { stdio: "ignore" });
  let owner;
  for (let i = 0; i < 100; i++) {
    try {
      owner = JSON.parse((await readFile(receipt, "utf8")).replace(/^\uFEFF/u, ""));
      break;
    } catch {}
    await delay(100);
  }
  assert.equal(owner?.Pid, child.pid);
  assert.match(owner.Created, /^\d{4}-\d{2}-\d{2}T/u);
  assert.ok(owner.Window > 0);
  const checked = `$ErrorActionPreference='Stop';$p=Get-CimInstance Win32_Process -Filter 'ProcessId=${owner.Pid}';if(-not $p -or $p.CreationDate.ToUniversalTime().ToString('o') -cne ${quote(owner.Created)}){throw 'Owned fixture identity changed'};$lease=[Diagnostics.Process]::GetProcessById(${owner.Pid});[void]$lease.Handle;`;
  return {
    marker,
    owner,
    async input() {
      const input = `${checked}Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class OwnedInput {
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
}
'@
[void][OwnedInput]::SetForegroundWindow([IntPtr]${owner.Window});Start-Sleep -Milliseconds 250;[uint32]$pidOwner=0;$hwnd=[OwnedInput]::GetForegroundWindow();[void][OwnedInput]::GetWindowThreadProcessId($hwnd,[ref]$pidOwner);if($hwnd.ToInt64() -ne ${owner.Window} -or $pidOwner -ne ${owner.Pid}){throw 'Owned foreground unavailable'};$again=Get-CimInstance Win32_Process -Filter 'ProcessId=${owner.Pid}';if($lease.HasExited -or $again.CreationDate.ToUniversalTime().ToString('o') -cne ${quote(owner.Created)}){throw 'Owned input identity changed'};[OwnedInput]::keybd_event(117,0,0,[UIntPtr]::Zero);[OwnedInput]::keybd_event(117,0,2,[UIntPtr]::Zero);[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()`;
      const { stdout } = await execute("powershell.exe", ["-NoProfile", "-Command", input], { windowsHide: true });
      const timestamp = Number(stdout.trim());
      assert.ok(Number.isSafeInteger(timestamp));
      return timestamp;
    },
    async close() {
      if (child.exitCode !== null) return;
      await execute(
        "powershell.exe",
        ["-NoProfile", "-Command", `${checked}if(-not $lease.CloseMainWindow()){throw 'Owned fixture close rejected'}`],
        { windowsHide: true },
      );
      for (let i = 0; i < 100 && child.exitCode === null; i++) await delay(100);
      assert.notEqual(child.exitCode, null, "Owned activity fixture must exit");
    },
  };
}
