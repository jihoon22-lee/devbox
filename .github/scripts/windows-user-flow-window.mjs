// UI Automation is scoped to the PID, creation time and image this fixture owns.
import assert from "node:assert/strict";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { allWindowsProcesses } from "./windows-packaged-smoke.mjs";
export function captureWindowOwner(identity, fixtureRoot) {
  const current = allWindowsProcesses().find(
    (p) =>
      p.Pid === identity.Pid && p.Created === identity.Created && p.Path.toLowerCase() === identity.Path.toLowerCase(),
  );
  assert.ok(current, "Owned window process identity changed");
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `[Diagnostics.Process]::GetProcessById(${identity.Pid}).StartTime.ToUniversalTime().ToString('o')`,
    ],
    { encoding: "utf8", timeout: 10000 },
  );
  assert.equal(result.status, 0, "Owned window start time unavailable");
  const started = result.stdout.trim();
  assert.match(started, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$/);
  return { identity, fixtureRoot, started };
}
export function nativeWindowAction(
  owner,
  action,
  { controlId, controlName, filePath, windowName, width, height } = {},
) {
  const args = [
    "-NoProfile",
    "-NonInteractive",
    "-File",
    path.resolve(".github/scripts/windows-installer-ui.ps1"),
    "-TargetProcessId",
    String(owner.identity.Pid),
    "-ExpectedExecutable",
    owner.identity.Path,
    "-ExpectedStartTimeUtc",
    owner.started,
    "-FixtureRoot",
    owner.fixtureRoot,
    "-Action",
    action,
  ];
  if (controlId) args.push("-ControlId", controlId);
  if (controlName) args.push("-ControlName", controlName);
  if (filePath) args.push("-FilePath", filePath);
  if (windowName) args.push("-WindowName", windowName);
  if (width !== undefined) args.push("-Width", String(width));
  if (height !== undefined) args.push("-Height", String(height));
  const result = spawnSync("powershell.exe", args, { encoding: "utf8", timeout: 20000 });
  assert.equal(result.status, 0, `Owned ${action} UI Automation failed: ${(result.stderr ?? "").slice(0, 600)}`);
  return action === "Inspect" ? JSON.parse(result.stdout.replace(/^\uFEFF/, "")) : undefined;
}

export function ownedProductCohort(identity) {
  const processes = allWindowsProcesses();
  assert.ok(
    processes.some((p) => p.Pid === identity.Pid && p.Created === identity.Created && p.Path === identity.Path),
  );
  const owned = new Set([identity.Pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const process of processes)
      if (owned.has(process.ParentPid) && !owned.has(process.Pid)) {
        owned.add(process.Pid);
        changed = true;
      }
  }
  return processes.filter((p) => owned.has(p.Pid));
}
export async function measureWarmOwnedWindow(owner, cdp) {
  nativeWindowAction(owner, "Minimize");
  const before = performance.now();
  nativeWindowAction(owner, "Activate");
  await cdp.evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
  captureWindowOwner(owner.identity, owner.fixtureRoot);
  return performance.now() - before;
}
