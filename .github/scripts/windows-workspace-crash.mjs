import assert from "node:assert/strict";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createdUtcExpression } from "./windows-process-identity.mjs";
export async function crashOwnedWorkspace({ owner, executable, child, waitForExit, run = spawnSync }) {
  assert.ok(Number.isSafeInteger(owner?.Pid) && owner.Pid > 0 && child?.pid === owner.Pid);
  for (const key of ["Created", "Name", "Path"]) assert.ok(typeof owner[key] === "string" && owner[key].length > 0);
  assert.equal(path.win32.resolve(owner.Path).toLowerCase(), path.win32.resolve(executable).toLowerCase());
  assert.equal(path.win32.basename(owner.Path).toLowerCase(), "devbox-workspace.exe");
  const encoded = (value) => Buffer.from(value, "utf8").toString("base64");
  const command =
    `$decode=[Text.Encoding]::UTF8; ` +
    `$expectedCreated=$decode.GetString([Convert]::FromBase64String('${encoded(owner.Created)}')); ` +
    `$expectedName=$decode.GetString([Convert]::FromBase64String('${encoded(owner.Name)}')); ` +
    `$expectedPath=$decode.GetString([Convert]::FromBase64String('${encoded(owner.Path)}')); ` +
    `$item=Get-CimInstance Win32_Process -Filter 'ProcessId = ${owner.Pid}' -ErrorAction Stop; ` +
    `$acted=$false; if($item -and (${createdUtcExpression("$item")}) -ceq $expectedCreated -and ` +
    `[string]$item.Name -ceq $expectedName -and [string]$item.ExecutablePath -ieq $expectedPath){` +
    `Stop-Process -Id ${owner.Pid} -Force -ErrorAction Stop; $acted=$true}; ` +
    `[bool]$acted | ConvertTo-Json -Compress`;
  const result = run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
    encoding: "utf8",
    timeout: 10000,
  });
  assert.ok(result.status === 0 && result.stdout.trim() === "true", "Owned crash identity could not be confirmed");
  await waitForExit();
}
